//! 認証（秘密の発行と照合）、バインドアドレス、危険な API の門（architecture.md 7.4節、フェーズ6j-ii）
//!
//! - 起動時に、Host のロールの秘密（OS の乱数、64 文字の 16 進）を作る（`YUI_TOKEN` で外から与えてもよい）。
//! - 秘密は、REST では `Authorization: Bearer <秘密>`、WebSocket では同じヘッダー（ネイティブ）か、
//!   サブプロトコル `yui.token.<秘密>`（ブラウザは、WebSocket にヘッダーを付けられない）で受け取る。
//!   URL には入れない（履歴、ログ、Referer に残るため）。
//! - ロールは、要求（接続）を受け付けたときに決まり、接続の間は変わらない。
//! - `127.0.0.1` 以外にバインドするときは、TLS を終端するプロキシの後ろであると宣言し、ターミナルとファイルの API は、
//!   明示的に有効にしたときだけ開く。

use axum::{
    body::Body,
    extract::{ConnectInfo, State, ws::WebSocketUpgrade},
    http::{HeaderMap, Request, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use yui_core::role::{MIN_TOKEN_LEN, RoleResolver, TokenStore, UserRole};

/// 既定のバインドアドレス
pub const DEFAULT_BIND: &str = "127.0.0.1:8080";
/// WebSocket のサブプロトコルに秘密を載せるときの接頭辞
pub const WS_PROTOCOL_PREFIX: &str = "yui.token.";
/// 認証を要らない `/api/` の接頭辞（同梱のフォント。秘密を含まない）
const PUBLIC_PREFIXES: [&str; 1] = ["/api/assets/"];

/// 危険な API を開くか
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Features {
    /// `/api/terminal`（シェル）
    pub terminal: bool,
    /// `/api/fs/*`（`YUI_WORKSPACE_ROOT` の下のファイルの一覧と読み込み）
    pub fs: bool,
}

impl Features {
    pub fn all() -> Self {
        Self {
            terminal: true,
            fs: true,
        }
    }
}

/// 要求の認証の結果（`guard` が、要求の拡張に入れる）
#[derive(Debug, Clone)]
pub struct Auth {
    pub role: UserRole,
    /// 招待の秘密で入ったときの、その招待の ID（取り消し・期限切れで、接続を閉じるため）
    pub invite_id: Option<String>,
    /// 秘密を WebSocket のサブプロトコルで受け取ったときの、そのサブプロトコル（応答で選び返す）
    pub ws_protocol: Option<String>,
}

impl Auth {
    /// サブプロトコルで秘密を受け取った接続には、そのサブプロトコルを選び返す（ブラウザは、選び返されないと接続を閉じる）
    pub fn select_protocol(&self, ws: WebSocketUpgrade) -> WebSocketUpgrade {
        match &self.ws_protocol {
            Some(p) => ws.protocols([p.clone()]),
            None => ws,
        }
    }
}

/// 要求から、秘密（と、サブプロトコルで受け取ったときのそのサブプロトコル）を取り出す
pub fn token_from_headers(headers: &HeaderMap) -> Option<(String, Option<String>)> {
    if let Some(v) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        let v = v.trim();
        if v.len() > 7 && v[..7].eq_ignore_ascii_case("bearer ") {
            return Some((v[7..].trim().to_string(), None));
        }
    }
    let protocols = headers
        .get(header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|v| v.to_str().ok())?;
    protocols.split(',').map(str::trim).find_map(|p| {
        p.strip_prefix(WS_PROTOCOL_PREFIX)
            .map(|t| (t.to_string(), Some(p.to_string())))
    })
}

/// 認証の状態（`guard` に渡す）
pub struct AuthState {
    pub credentials: Arc<Credentials>,
    pub limiter: RateLimiter,
}

/// `/api/` の認証。秘密がなければ 401、知らない秘密なら 403（失敗は、接続元ごとに数え、多すぎれば 429）。
/// 通れば、`Auth` を要求に入れる。
pub async fn guard(
    State(state): State<Arc<AuthState>>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let path = request.uri().path();
    if PUBLIC_PREFIXES.iter().any(|p| path.starts_with(p)) {
        return next.run(request).await;
    }
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip())
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
    let now = now_secs();
    if let Err(retry_after) = state.limiter.check(peer, now) {
        return too_many_attempts(retry_after);
    }
    let Some((token, ws_protocol)) = token_from_headers(request.headers()) else {
        // 秘密がない要求（設定していないクライアントの再接続など）は、数えない
        return (
            StatusCode::UNAUTHORIZED,
            "Unauthorized: send the token as 'Authorization: Bearer <token>'",
        )
            .into_response();
    };
    let Some(identity) = state.credentials.authenticate_at(&token, now) else {
        if state.limiter.record_failure(peer, now) {
            tracing::warn!(
                "Too many failed authentications from {peer}: locked out for {LOCKOUT_SECS} seconds"
            );
        }
        return (StatusCode::FORBIDDEN, "Forbidden: invalid token").into_response();
    };
    request.extensions_mut().insert(Auth {
        role: identity.role,
        invite_id: identity.invite_id,
        ws_protocol,
    });
    next.run(request).await
}

fn too_many_attempts(retry_after: u64) -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(header::RETRY_AFTER, retry_after.to_string())],
        "Too many failed authentications; try again later",
    )
        .into_response()
}

/// 現在の時刻（UNIX 時刻の秒）
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 招待の有効期間（秒）の下限・上限・既定
pub const INVITE_TTL_MIN: u64 = 60;
pub const INVITE_TTL_MAX: u64 = 7 * 24 * 3600;
pub const INVITE_TTL_DEFAULT: u64 = 3600;
/// 同時に有効な招待の数の上限
pub const INVITE_MAX_ACTIVE: usize = 50;

/// 招待（Host が発行する、Editor / Viewer の期限つきの秘密）
#[derive(Debug, Clone)]
struct Invite {
    id: String,
    token: String,
    role: UserRole,
    expires_at: u64,
}

/// 発行した招待の情報（一覧では、秘密は含まない）
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct InviteInfo {
    pub id: String,
    pub role: String,
    pub expires_at: u64,
    /// あと何秒有効か（応答を作った時点。クライアントが、時計を使わずに表示できるように）
    pub expires_in_secs: u64,
}

/// 招待を作れなかった理由
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InviteError {
    /// Host の招待は作れない（Editor か Viewer だけ）
    RoleNotInvitable,
    TtlOutOfRange,
    TooManyActive,
    NoRandomSource,
}

/// 認証できた相手
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub role: UserRole,
    /// 招待の秘密で入ったとき、その招待の ID
    pub invite_id: Option<String>,
}

/// 秘密（Host の 1 つと、期限つきの招待）の表。招待は、メモリにだけ置く（再起動で、すべて無効になる）。
pub struct Credentials {
    host: TokenStore,
    invites: Mutex<Vec<Invite>>,
    /// 取り消した招待の ID を、開いている接続に知らせる（すぐに閉じるため）
    revoked: tokio::sync::broadcast::Sender<String>,
}

impl Credentials {
    pub fn new(host_token: &str) -> Self {
        Self {
            host: host_token_store(host_token),
            invites: Mutex::new(Vec::new()),
            revoked: tokio::sync::broadcast::channel(64).0,
        }
    }

    /// 招待が取り消されたとき、その ID が流れてくる受け口（接続の側が、先に作っておく）
    pub fn subscribe_revoked(&self) -> tokio::sync::broadcast::Receiver<String> {
        self.revoked.subscribe()
    }

    /// 招待の期限（UNIX 秒）。もう無い（取り消された）なら `None`
    pub fn invite_expires_at(&self, id: &str) -> Option<u64> {
        let invites = self.invites.lock().ok()?;
        invites.iter().find(|i| i.id == id).map(|i| i.expires_at)
    }

    /// 秘密からロールを決める。Host の秘密、次に、期限内の招待の順。どちらも、定数時間で比べる。
    pub fn authenticate_at(&self, token: &str, now: u64) -> Option<Identity> {
        if let Some(role) = self.host.resolve(token) {
            return Some(Identity {
                role,
                invite_id: None,
            });
        }
        let invites = self.invites.lock().ok()?;
        let mut found = None;
        for invite in invites.iter() {
            if yui_core::role::constant_time_eq(&invite.token, token) && invite.expires_at > now {
                found = Some(Identity {
                    role: invite.role,
                    invite_id: Some(invite.id.clone()),
                });
            }
        }
        found
    }

    /// 招待を作る。Editor か Viewer だけ。期限は `INVITE_TTL_MIN`〜`INVITE_TTL_MAX` 秒。
    /// 戻り値は、秘密と、招待の情報（秘密は、ここでしか分からない）。
    pub fn create_invite_at(
        &self,
        role: UserRole,
        ttl_secs: u64,
        now: u64,
    ) -> Result<(String, InviteInfo), InviteError> {
        if role == UserRole::Host {
            return Err(InviteError::RoleNotInvitable);
        }
        if !(INVITE_TTL_MIN..=INVITE_TTL_MAX).contains(&ttl_secs) {
            return Err(InviteError::TtlOutOfRange);
        }
        let token = random_token().map_err(|_| InviteError::NoRandomSource)?;
        let id = random_token()
            .map_err(|_| InviteError::NoRandomSource)?
            .chars()
            .take(12)
            .collect::<String>();
        let mut invites = self
            .invites
            .lock()
            .map_err(|_| InviteError::NoRandomSource)?;
        invites.retain(|i| i.expires_at > now);
        if invites.len() >= INVITE_MAX_ACTIVE {
            return Err(InviteError::TooManyActive);
        }
        let info = InviteInfo {
            id: id.clone(),
            role: role_name(role).to_string(),
            expires_at: now + ttl_secs,
            expires_in_secs: ttl_secs,
        };
        invites.push(Invite {
            id,
            token: token.clone(),
            role,
            expires_at: info.expires_at,
        });
        Ok((token, info))
    }

    /// 有効な招待の一覧（秘密は含まない）
    pub fn list_invites_at(&self, now: u64) -> Vec<InviteInfo> {
        let Ok(mut invites) = self.invites.lock() else {
            return Vec::new();
        };
        invites.retain(|i| i.expires_at > now);
        invites
            .iter()
            .map(|i| InviteInfo {
                id: i.id.clone(),
                role: role_name(i.role).to_string(),
                expires_at: i.expires_at,
                expires_in_secs: i.expires_at.saturating_sub(now),
            })
            .collect()
    }

    /// 招待を取り消す。あったら `true`。
    pub fn revoke(&self, id: &str) -> bool {
        let Ok(mut invites) = self.invites.lock() else {
            return false;
        };
        let before = invites.len();
        invites.retain(|i| i.id != id);
        let removed = invites.len() != before;
        if removed {
            // 受け手がいなくても構わない（招待で開いた接続が、ないだけ）
            let _ = self.revoked.send(id.to_string());
        }
        removed
    }

    /// 試験用: 期限のない招待として、決まった秘密を登録する
    #[cfg(test)]
    pub fn register_test_invite(&self, token: &str, role: UserRole) {
        let mut invites = self.invites.lock().unwrap();
        invites.push(Invite {
            id: format!("test-{}", role_name(role)),
            token: token.to_string(),
            role,
            expires_at: u64::MAX / 2,
        });
    }

    /// 試験用: 決まった期限（UNIX 秒）の招待を登録する
    #[cfg(test)]
    pub fn register_test_invite_until(&self, id: &str, role: UserRole, expires_at: u64) {
        let mut invites = self.invites.lock().unwrap();
        invites.push(Invite {
            id: id.to_string(),
            token: format!("token-of-{id}"),
            role,
            expires_at,
        });
    }

    /// 招待が、まだ有効か（取り消されておらず、期限内）。接続の間、定期的に調べて、無効なら閉じる。
    pub fn invite_active_at(&self, id: &str, now: u64) -> bool {
        self.invites
            .lock()
            .map(|v| v.iter().any(|i| i.id == id && i.expires_at > now))
            .unwrap_or(false)
    }
}

impl RoleResolver for Credentials {
    fn resolve(&self, token: &str) -> Option<UserRole> {
        self.authenticate_at(token, now_secs()).map(|i| i.role)
    }
}

fn role_name(role: UserRole) -> &'static str {
    match role {
        UserRole::Host => "host",
        UserRole::Editor => "editor",
        UserRole::Viewer => "viewer",
    }
}

/// 招待の `role` の文字列
pub fn parse_invite_role(name: &str) -> Option<UserRole> {
    match name.trim().to_ascii_lowercase().as_str() {
        "editor" => Some(UserRole::Editor),
        "viewer" => Some(UserRole::Viewer),
        _ => None,
    }
}

/// 認証の失敗の回数制限: 接続元ごとに、`WINDOW_SECS` 秒の間に `MAX_FAILURES` 回失敗したら、`LOCKOUT_SECS` 秒、
/// すべての要求を 429 にする（正しい秘密でも）。
pub const MAX_FAILURES: u32 = 10;
pub const WINDOW_SECS: u64 = 60;
pub const LOCKOUT_SECS: u64 = 60;
/// 覚えておく接続元の数の上限（超えたら、古いものから捨てる）
const MAX_TRACKED_PEERS: usize = 1024;

#[derive(Debug, Clone, Copy)]
struct Attempts {
    failures: u32,
    window_start: u64,
    locked_until: u64,
}

#[derive(Default)]
pub struct RateLimiter {
    peers: Mutex<HashMap<IpAddr, Attempts>>,
}

impl RateLimiter {
    /// 締め出されていなければ `Ok`。締め出し中なら、あと何秒かを返す。
    pub fn check(&self, peer: IpAddr, now: u64) -> Result<(), u64> {
        let peers = self.peers.lock().map_err(|_| 0u64)?;
        match peers.get(&peer) {
            Some(a) if a.locked_until > now => Err(a.locked_until - now),
            _ => Ok(()),
        }
    }

    /// 失敗を数える。この失敗で締め出しが始まったら `true`。
    pub fn record_failure(&self, peer: IpAddr, now: u64) -> bool {
        let Ok(mut peers) = self.peers.lock() else {
            return false;
        };
        if peers.len() >= MAX_TRACKED_PEERS && !peers.contains_key(&peer) {
            // 古い記録（窓も締め出しも終わったもの）を捨てる。それでも多ければ、いちばん古いものを捨てる。
            peers.retain(|_, a| a.locked_until > now || now < a.window_start + WINDOW_SECS);
            if peers.len() >= MAX_TRACKED_PEERS
                && let Some(oldest) = peers
                    .iter()
                    .min_by_key(|(_, a)| a.window_start)
                    .map(|(ip, _)| *ip)
            {
                peers.remove(&oldest);
            }
        }
        let entry = peers.entry(peer).or_insert(Attempts {
            failures: 0,
            window_start: now,
            locked_until: 0,
        });
        if now >= entry.window_start + WINDOW_SECS {
            entry.failures = 0;
            entry.window_start = now;
        }
        entry.failures += 1;
        if entry.failures >= MAX_FAILURES {
            entry.failures = 0;
            entry.window_start = now;
            entry.locked_until = now + LOCKOUT_SECS;
            return true;
        }
        false
    }
}

/// Host のロールの秘密を決める。`YUI_TOKEN` があればそれ（短すぎればエラー）、なければ乱数。
pub fn host_token(env_token: Option<&str>) -> Result<String, String> {
    match env_token.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) if t.len() < MIN_TOKEN_LEN => Err(format!(
            "YUI_TOKEN is too short ({} characters; at least {MIN_TOKEN_LEN})",
            t.len()
        )),
        Some(t)
            if t.chars()
                .any(|c| c.is_whitespace() || c == ',' || c.is_control()) =>
        {
            Err("YUI_TOKEN must not contain spaces, commas or control characters".to_string())
        }
        Some(t) => Ok(t.to_string()),
        None => random_token(),
    }
}

/// OS の暗号論的乱数から、256 ビット（16 進で 64 文字）の秘密を作る
pub fn random_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| format!("no secure random source: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// 起動時の秘密の表（Host の秘密だけ）を作る
pub fn host_token_store(token: &str) -> TokenStore {
    let mut store = TokenStore::new();
    store.register(token, UserRole::Host);
    store
}

/// 環境変数 `YUI_BIND`（既定 `127.0.0.1:8080`）から、待ち受けるアドレスを決める。
/// loopback 以外は、`YUI_BEHIND_TLS_PROXY=1`（TLS を終端するプロキシの後ろにある）の宣言がなければ、拒否する。
pub fn resolve_bind(bind: Option<&str>, behind_tls_proxy: bool) -> Result<SocketAddr, String> {
    let text = bind
        .map(str::trim)
        .filter(|b| !b.is_empty())
        .unwrap_or(DEFAULT_BIND);
    let text = text.replace("localhost:", "127.0.0.1:");
    let addr: SocketAddr = text
        .parse()
        .map_err(|e| format!("YUI_BIND '{text}' is not an address:port ({e})"))?;
    if !addr.ip().is_loopback() && !behind_tls_proxy {
        return Err(format!(
            "YUI_BIND {addr} is not a loopback address. backend speaks plain HTTP; put it behind a reverse proxy that \
             terminates TLS and set YUI_BEHIND_TLS_PROXY=1 to confirm (see architecture.md 7.4)."
        ));
    }
    Ok(addr)
}

/// どの API を開くか。loopback なら、すべて。それ以外は、`YUI_ENABLE_TERMINAL=1` / `YUI_ENABLE_FS=1` のものだけ。
pub fn features_for(addr: &SocketAddr, enable_terminal: bool, enable_fs: bool) -> Features {
    if addr.ip().is_loopback() {
        Features::all()
    } else {
        Features {
            terminal: enable_terminal,
            fs: enable_fs,
        }
    }
}

/// `1` / `true` / `yes` なら真（環境変数の真偽）
pub fn env_flag(value: Option<&str>) -> bool {
    matches!(
        value.map(|v| v.trim().to_ascii_lowercase()).as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}

/// 接続の情報を書くファイル（`<設定フォルダ>/yui/backend.json`）の場所
pub fn connection_file_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("yui").join("backend.json"))
}

/// 接続の情報（URL と秘密）を、利用者だけが読めるファイル（0600）に書く。同じコンピュータのデスクトップのアプリが読む。
pub fn write_connection_file(
    path: &std::path::Path,
    url: &str,
    token: &str,
) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let body = serde_json::json!({ "url": url, "token": token, "pid": std::process::id() });
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    #[cfg(unix)]
    {
        // すでにあったファイルのモードも、0600 にそろえる
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(body.to_string().as_bytes())
}

/// ブラウザ用の、秘密つきの URL（フラグメントは、サーバーに送られず、ログにも残らない）
pub fn browser_url(base: &str, token: &str) -> String {
    format!("{}/#token={token}", base.trim_end_matches('/'))
}

/// 待ち受けるアドレスのうち、URL に使う部分（`0.0.0.0` や `::` は、自分への接続としては、loopback で言う）
pub fn display_url(addr: &SocketAddr) -> String {
    let ip = match addr.ip() {
        IpAddr::V4(v4) if v4.is_unspecified() => "127.0.0.1".to_string(),
        IpAddr::V6(v6) if v6.is_unspecified() => "[::1]".to_string(),
        IpAddr::V6(v6) => format!("[{v6}]"),
        IpAddr::V4(v4) => v4.to_string(),
    };
    format!("http://{ip}:{}", addr.port())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(pairs: &[(header::HeaderName, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(k.clone(), HeaderValue::from_str(v).unwrap());
        }
        h
    }

    #[test]
    fn test_token_from_bearer_and_subprotocol() {
        let h = headers(&[(header::AUTHORIZATION, "Bearer abc123")]);
        assert_eq!(token_from_headers(&h), Some(("abc123".into(), None)));
        let h = headers(&[(header::AUTHORIZATION, "bearer  xyz ")]);
        assert_eq!(token_from_headers(&h).unwrap().0, "xyz");
        // ブラウザ: サブプロトコルの一覧のうち、接頭辞のあるもの
        let h = headers(&[(header::SEC_WEBSOCKET_PROTOCOL, "chat, yui.token.sekret99")]);
        assert_eq!(
            token_from_headers(&h),
            Some(("sekret99".into(), Some("yui.token.sekret99".into())))
        );
        // 秘密のないもの・形の違う Authorization
        assert_eq!(token_from_headers(&HeaderMap::new()), None);
        assert_eq!(
            token_from_headers(&headers(&[(header::AUTHORIZATION, "Basic abc")])),
            None
        );
        assert_eq!(
            token_from_headers(&headers(&[(header::SEC_WEBSOCKET_PROTOCOL, "chat")])),
            None
        );
    }

    #[test]
    fn test_host_token_from_env_or_random() {
        assert_eq!(
            host_token(Some("0123456789abcdef")).unwrap(),
            "0123456789abcdef"
        );
        assert!(host_token(Some("short")).is_err());
        assert!(host_token(Some("0123456789 abcdef")).is_err());
        assert!(host_token(Some("0123456789,abcdef")).is_err());
        let a = host_token(None).unwrap();
        let b = host_token(Some("  ")).unwrap();
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn test_bind_defaults_to_loopback_and_refuses_open_addresses() {
        assert_eq!(
            resolve_bind(None, false).unwrap().to_string(),
            "127.0.0.1:8080"
        );
        assert_eq!(
            resolve_bind(Some("localhost:9000"), false)
                .unwrap()
                .to_string(),
            "127.0.0.1:9000"
        );
        assert!(resolve_bind(Some("[::1]:8080"), false).is_ok());
        for open in ["0.0.0.0:8080", "192.168.1.5:8080", "[::]:8080"] {
            let err = resolve_bind(Some(open), false).unwrap_err();
            assert!(err.contains("YUI_BEHIND_TLS_PROXY"), "{open}: {err}");
            assert!(resolve_bind(Some(open), true).is_ok(), "{open}");
        }
        assert!(resolve_bind(Some("nonsense"), false).is_err());
    }

    #[test]
    fn test_dangerous_apis_are_closed_off_loopback_unless_enabled() {
        let lo: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let open: SocketAddr = "0.0.0.0:8080".parse().unwrap();
        assert_eq!(features_for(&lo, false, false), Features::all());
        assert_eq!(
            features_for(&open, false, false),
            Features {
                terminal: false,
                fs: false
            }
        );
        assert_eq!(
            features_for(&open, true, false),
            Features {
                terminal: true,
                fs: false
            }
        );
        assert!(
            env_flag(Some("1"))
                && env_flag(Some("TRUE"))
                && !env_flag(Some("0"))
                && !env_flag(None)
        );
    }

    #[test]
    fn test_connection_file_is_private_and_holds_url_and_token() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("yui").join("backend.json");
        write_connection_file(&path, "http://127.0.0.1:8080", "tok-0123456789abcdef").unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["url"], "http://127.0.0.1:8080");
        assert_eq!(v["token"], "tok-0123456789abcdef");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
            // 書き直しても、0600 のまま（以前に緩いモードで作られていても）
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            write_connection_file(&path, "http://127.0.0.1:8080", "tok-0123456789abcdef").unwrap();
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn test_urls() {
        assert_eq!(
            browser_url("http://127.0.0.1:8080/", "abc"),
            "http://127.0.0.1:8080/#token=abc"
        );
        assert_eq!(
            display_url(&"0.0.0.0:9".parse().unwrap()),
            "http://127.0.0.1:9"
        );
        assert_eq!(
            display_url(&"[::1]:8080".parse().unwrap()),
            "http://[::1]:8080"
        );
    }

    const HOST: &str = "host-secret-0123456789";

    #[test]
    fn test_invitations_have_roles_expiry_and_can_be_revoked() {
        let c = Credentials::new(HOST);
        let now = 1_000_000;
        assert_eq!(
            c.authenticate_at(HOST, now),
            Some(Identity {
                role: UserRole::Host,
                invite_id: None
            })
        );
        let (token, info) = c.create_invite_at(UserRole::Viewer, 600, now).unwrap();
        assert_eq!(info.role, "viewer");
        assert_eq!(info.expires_at, now + 600);
        let who = c.authenticate_at(&token, now + 599).unwrap();
        assert_eq!(who.role, UserRole::Viewer);
        assert_eq!(who.invite_id.as_deref(), Some(info.id.as_str()));
        // 期限の秒に、無効になる
        assert_eq!(c.authenticate_at(&token, now + 600), None);
        assert!(c.invite_active_at(&info.id, now + 599));
        assert!(!c.invite_active_at(&info.id, now + 600));
        // 取り消し
        let (token2, info2) = c.create_invite_at(UserRole::Editor, 600, now).unwrap();
        assert!(c.revoke(&info2.id));
        assert!(!c.revoke(&info2.id));
        assert_eq!(c.authenticate_at(&token2, now), None);
        assert!(!c.invite_active_at(&info2.id, now));
        // 一覧は、期限内のものだけで、秘密は含まない
        let list = c.list_invites_at(now + 10);
        assert_eq!(list.len(), 1);
        assert!(!format!("{list:?}").contains(&token));
        assert!(c.list_invites_at(now + 601).is_empty());
    }

    #[test]
    fn test_invitation_limits() {
        let c = Credentials::new(HOST);
        let now = 5;
        assert_eq!(
            c.create_invite_at(UserRole::Host, 600, now).unwrap_err(),
            InviteError::RoleNotInvitable
        );
        for ttl in [0, INVITE_TTL_MIN - 1, INVITE_TTL_MAX + 1] {
            assert_eq!(
                c.create_invite_at(UserRole::Viewer, ttl, now).unwrap_err(),
                InviteError::TtlOutOfRange,
                "{ttl}"
            );
        }
        for _ in 0..INVITE_MAX_ACTIVE {
            c.create_invite_at(UserRole::Viewer, 600, now).unwrap();
        }
        assert_eq!(
            c.create_invite_at(UserRole::Viewer, 600, now).unwrap_err(),
            InviteError::TooManyActive
        );
        // 期限が切れたものは、数えない
        assert!(c.create_invite_at(UserRole::Viewer, 600, now + 601).is_ok());
        assert_eq!(parse_invite_role(" Viewer "), Some(UserRole::Viewer));
        assert_eq!(parse_invite_role("editor"), Some(UserRole::Editor));
        assert_eq!(parse_invite_role("host"), None);
    }

    #[test]
    fn test_rate_limiter_locks_out_after_repeated_failures() {
        let limiter = RateLimiter::default();
        let a: IpAddr = "10.0.0.1".parse().unwrap();
        let b: IpAddr = "10.0.0.2".parse().unwrap();
        let t = 1000;
        for i in 1..MAX_FAILURES {
            assert!(!limiter.record_failure(a, t + u64::from(i)), "{i}");
            assert_eq!(limiter.check(a, t + u64::from(i)), Ok(()));
        }
        // 上限の失敗で、締め出しが始まる（その 1 回だけ `true`）
        assert!(limiter.record_failure(a, t + 10));
        assert_eq!(limiter.check(a, t + 11), Err(LOCKOUT_SECS - 1));
        // 別の接続元は、影響を受けない
        assert_eq!(limiter.check(b, t + 11), Ok(()));
        // 締め出しが終われば、また使える
        assert_eq!(limiter.check(a, t + 10 + LOCKOUT_SECS), Ok(()));
        // 窓が過ぎれば、失敗の数は、数え直し
        let c: IpAddr = "10.0.0.3".parse().unwrap();
        for _ in 0..MAX_FAILURES - 1 {
            limiter.record_failure(c, t);
        }
        assert!(!limiter.record_failure(c, t + WINDOW_SECS));
        assert_eq!(limiter.check(c, t + WINDOW_SECS), Ok(()));
    }

    #[test]
    fn test_rate_limiter_forgets_old_peers() {
        let limiter = RateLimiter::default();
        for i in 0..(MAX_TRACKED_PEERS as u32 + 50) {
            let ip = IpAddr::V4(Ipv4Addr::from(i + 1));
            limiter.record_failure(ip, 100 + u64::from(i % 3));
        }
        assert!(limiter.peers.lock().unwrap().len() <= MAX_TRACKED_PEERS);
    }
}
