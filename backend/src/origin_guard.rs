//! `Origin` / `Host` の検査と、認証の失敗の記録（architecture.md 7.4節、フェーズ6j-i）
//!
//! ブラウザは WebSocket に同一オリジンのルールを適用しないので、`127.0.0.1` にしかバインドしていなくても、
//! 利用者が見ている Web ページから `backend` に接続できてしまう。ここで、(1) `Host` が自分の名前でない要求
//! （DNS リバインディング）と、(2) `Origin` が自分のオリジンでも許可リストにもない要求（別のサイトのページ）を断る。
//! `Origin` を付けない要求（ネイティブのアプリ、コマンドラインのツール）は、そのまま通す。

use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, Request, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::sync::Arc;

/// `Host` として受け付ける、自分の名前（ポートは見ない）
const LOOPBACK_HOSTS: [&str; 3] = ["127.0.0.1", "localhost", "[::1]"];

/// 受け付ける `Origin` / `Host` の方針
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OriginPolicy {
    /// 同一オリジンのほかに受け付ける `Origin`（`scheme://host[:port]`、小文字）。環境変数 `YUI_ALLOWED_ORIGINS`
    pub allowed_origins: Vec<String>,
    /// loopback のほかに受け付ける `Host` の名前（ポートなし、小文字）。環境変数 `YUI_ALLOWED_HOSTS`
    pub allowed_hosts: Vec<String>,
}

/// 検査で断る理由
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    HostNotAllowed,
    OriginNotAllowed,
}

impl Rejection {
    fn message(self) -> &'static str {
        match self {
            Rejection::HostNotAllowed => {
                "Forbidden: this Host is not allowed (see YUI_ALLOWED_HOSTS)"
            }
            Rejection::OriginNotAllowed => {
                "Forbidden: this Origin is not allowed (see YUI_ALLOWED_ORIGINS)"
            }
        }
    }
}

fn split_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|s| s.trim().trim_end_matches('/').to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

/// `Host` ヘッダーの値から、ポートを除いた名前（小文字）を取り出す
fn host_name(host: &str) -> String {
    let host = host.trim().to_ascii_lowercase();
    if host.starts_with('[') {
        // [::1]:8080
        return match host.find(']') {
            Some(end) => host[..=end].to_string(),
            None => host,
        };
    }
    host.split(':').next().unwrap_or("").to_string()
}

impl OriginPolicy {
    pub fn new(allowed_origins: &str, allowed_hosts: &str) -> Self {
        Self {
            allowed_origins: split_list(allowed_origins),
            allowed_hosts: split_list(allowed_hosts),
        }
    }

    /// 環境変数 `YUI_ALLOWED_ORIGINS` と `YUI_ALLOWED_HOSTS`（どちらもコンマ区切り）から作る
    pub fn from_env() -> Self {
        let get = |name: &str| std::env::var(name).unwrap_or_default();
        Self::new(&get("YUI_ALLOWED_ORIGINS"), &get("YUI_ALLOWED_HOSTS"))
    }

    /// `Host` と `Origin` を検査する。どちらも付いていない要求は、通す（ブラウザは、必ず `Host` を付ける）。
    pub fn check(&self, host: Option<&str>, origin: Option<&str>) -> Result<(), Rejection> {
        if let Some(host) = host {
            let name = host_name(host);
            let known =
                LOOPBACK_HOSTS.contains(&name.as_str()) || self.allowed_hosts.contains(&name);
            if !known {
                return Err(Rejection::HostNotAllowed);
            }
        }
        let Some(origin) = origin else {
            return Ok(());
        };
        let origin = origin.trim().trim_end_matches('/').to_ascii_lowercase();
        if self.allowed_origins.contains(&origin) {
            return Ok(());
        }
        // 同一オリジン: Origin の `scheme://` を除いた部分が、Host と同じ
        let same_origin = host.is_some_and(|h| {
            origin
                .split_once("://")
                .is_some_and(|(_, authority)| authority == h.trim().to_ascii_lowercase())
        });
        if same_origin {
            Ok(())
        } else {
            Err(Rejection::OriginNotAllowed)
        }
    }
}

fn header_str(headers: &HeaderMap, name: header::HeaderName) -> Option<&str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

/// `/api/` の前に置く。`Origin` / `Host` を検査し、認証の失敗（401 / 403）とターミナルの接続を、ログに残す
/// （URL の問い合わせ部分には、トークンが入るので、ログには、パスだけを書く）。
pub async fn guard(
    State(policy): State<Arc<OriginPolicy>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let path = request.uri().path().to_string();
    let host = header_str(request.headers(), header::HOST);
    let origin = header_str(request.headers(), header::ORIGIN);
    if let Err(why) = policy.check(host, origin) {
        tracing::warn!(
            "Rejected {path}: {why:?} (Host: {}, Origin: {})",
            host.unwrap_or("-"),
            origin.unwrap_or("-")
        );
        return (StatusCode::FORBIDDEN, why.message()).into_response();
    }
    let response = next.run(request).await;
    match response.status() {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            tracing::warn!("Rejected {path}: {}", response.status());
        }
        StatusCode::SWITCHING_PROTOCOLS if path == "/api/terminal" => {
            tracing::info!("Terminal session opened");
        }
        _ => {}
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> OriginPolicy {
        OriginPolicy::default()
    }

    #[test]
    fn test_requests_without_origin_pass_for_our_own_host() {
        for host in [
            "127.0.0.1:8080",
            "localhost:8080",
            "LOCALHOST",
            "[::1]:8080",
        ] {
            assert_eq!(policy().check(Some(host), None), Ok(()), "{host}");
        }
        // Host もない要求（テストや HTTP/1.0）
        assert_eq!(policy().check(None, None), Ok(()));
    }

    #[test]
    fn test_other_hosts_are_refused_unless_allowed() {
        // DNS リバインディング: 別の名前が 127.0.0.1 に解決されても、Host で断る
        assert_eq!(
            policy().check(Some("evil.example:8080"), None),
            Err(Rejection::HostNotAllowed)
        );
        let p = OriginPolicy::new("", "yui.lab.example, Other.Example");
        assert_eq!(p.check(Some("yui.lab.example:8080"), None), Ok(()));
        assert_eq!(p.check(Some("other.example"), None), Ok(()));
        assert_eq!(
            p.check(Some("third.example"), None),
            Err(Rejection::HostNotAllowed)
        );
    }

    #[test]
    fn test_same_origin_passes_and_other_sites_are_refused() {
        let p = policy();
        assert_eq!(
            p.check(Some("127.0.0.1:8080"), Some("http://127.0.0.1:8080")),
            Ok(())
        );
        assert_eq!(
            p.check(Some("localhost:8080"), Some("http://localhost:8080/")),
            Ok(())
        );
        for origin in [
            "https://evil.example",
            "http://127.0.0.1:9999",
            "http://localhost:8080", // Host が 127.0.0.1 のとき、名前が違うオリジンは別のオリジン
            "null",
        ] {
            assert_eq!(
                p.check(Some("127.0.0.1:8080"), Some(origin)),
                Err(Rejection::OriginNotAllowed),
                "{origin}"
            );
        }
        // Origin だけがあり、Host がないときは、同一オリジンとは言えない
        assert_eq!(
            p.check(None, Some("https://evil.example")),
            Err(Rejection::OriginNotAllowed)
        );
    }

    #[test]
    fn test_allow_list_origins_pass() {
        let p = OriginPolicy::new("http://127.0.0.1:8085, https://yui.lab.example/", "");
        assert_eq!(
            p.check(Some("127.0.0.1:8080"), Some("http://127.0.0.1:8085")),
            Ok(())
        );
        assert_eq!(
            p.check(Some("127.0.0.1:8080"), Some("HTTPS://YUI.LAB.EXAMPLE")),
            Ok(())
        );
        assert_eq!(
            p.check(Some("127.0.0.1:8080"), Some("https://evil.example")),
            Err(Rejection::OriginNotAllowed)
        );
    }
}
