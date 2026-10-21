pub mod auth;
pub mod log_stream;
pub mod logging;
pub mod origin_guard;

use crate::log_stream::spawn_log_stream;
use auth::{Auth, Credentials, Features};
use axum::{
    Extension, Json, Router,
    body::Bytes,
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use origin_guard::OriginPolicy;
use std::path::{Component, Path as StdPath, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use tower_http::cors::{AllowOrigin, CorsLayer};
use yui_core::collab::{apply_app_state_delta, verify_role_action};
use yui_core::packing::{
    CollabHeader, Compression, pack_app_state, pack_collab_packet, unpack_app_state,
    unpack_collab_packet,
};
use yui_core::role::UserRole;
use yui_core::state::{Annotation, AppState, AppStateDelta, Atom, Molecule};
use yui_pty::{TerminalCommand, spawn_pty_session};

pub struct AppStateShared {
    pub app_state: Mutex<AppState>,
    /// 秘密（Host の 1 つと、期限つきの招待）
    pub credentials: Arc<Credentials>,
    pub tx: broadcast::Sender<Vec<u8>>,
    pub log_root: PathBuf,
    pub workspace_root: PathBuf,
    /// 開く危険な API（ターミナル、ファイル）
    pub features: Features,
}

#[tokio::main]
async fn main() {
    // 標準出力とファイル（YUI_LOG_DIR / OS の標準の場所）に、動作ログを出す
    let log_files = logging::init(std::env::var("YUI_LOG_DIR").ok().as_deref());
    if let Some((_, dir)) = &log_files {
        println!("YUI Log files: {}", dir.display());
    }

    // ブラウザ版（`trunk build --release` の `dist/`）を、バックエンドが配信する（YUI_WEB_ROOT）
    let web_root = match web_root_from_env() {
        Ok(root) => root,
        Err(e) => {
            eprintln!("YUI_WEB_ROOT: {e}");
            std::process::exit(2);
        }
    };
    // `Origin` / `Host` の許可（YUI_ALLOWED_ORIGINS / YUI_ALLOWED_HOSTS。既定は、自分のオリジンと loopback の名前だけ）
    let policy = OriginPolicy::from_env();
    for o in &policy.allowed_origins {
        println!("YUI Also allowing the Origin {o}");
    }
    for h in &policy.allowed_hosts {
        println!("YUI Also allowing the Host {h}");
    }
    // 待ち受けるアドレスと、開く API（architecture.md 7.4節）
    let env = |name: &str| std::env::var(name).ok();
    let behind_proxy = auth::env_flag(env("YUI_BEHIND_TLS_PROXY").as_deref());
    let addr = match auth::resolve_bind(env("YUI_BIND").as_deref(), behind_proxy) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let features = auth::features_for(
        &addr,
        auth::env_flag(env("YUI_ENABLE_TERMINAL").as_deref()),
        auth::env_flag(env("YUI_ENABLE_FS").as_deref()),
    );
    // Host の秘密（YUI_TOKEN か、起動ごとの乱数）
    let token = match auth::host_token(env("YUI_TOKEN").as_deref()) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let shared_state = create_shared_state_with(Arc::new(Credentials::new(&token)), features);
    let app = create_router_with_web(shared_state, web_root.as_deref(), policy);

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Cannot listen on {addr}: {e}");
            std::process::exit(2);
        }
    };
    let url = auth::display_url(&addr);
    println!("YUI Backend Server is running on {url}");
    if !features.terminal || !features.fs {
        println!(
            "YUI Disabled on this address: {}{}(YUI_ENABLE_TERMINAL=1 / YUI_ENABLE_FS=1 to open)",
            if features.terminal {
                ""
            } else {
                "the terminal "
            },
            if features.fs { "" } else { "file access " }
        );
    }
    // デスクトップのアプリは、このファイル（0600）から URL と秘密を読む。ブラウザは、下の URL（`#` 以降は、サーバーに送られない）。
    match auth::connection_file_path() {
        Some(path) => match auth::write_connection_file(&path, &url, &token) {
            Ok(()) => println!(
                "YUI Connection info for the desktop app: {}",
                path.display()
            ),
            Err(e) => eprintln!("Could not write {}: {e}", path.display()),
        },
        None => eprintln!("No config folder: the desktop app needs --token / YUI_TOKEN instead"),
    }
    if let Some(root) = &web_root {
        println!(
            "YUI The web build in {} is served. Open it with the token: {}",
            root.display(),
            auth::browser_url(&url, &token)
        );
    } else {
        println!(
            "YUI Every client needs the token (the file above; send it as 'Authorization: Bearer <token>'). \
             For the browser build (trunk serve), open its address followed by #token=<token>."
        );
    }
    // 接続元（認証の失敗の回数制限に使う）が分かるよう、接続の情報つきで配る
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await
    .unwrap();
}

/// テスト用の秘密（Host / Editor / Viewer）
#[cfg(test)]
pub fn test_credentials() -> Arc<Credentials> {
    // 招待の API の試験では、Editor / Viewer の秘密も、固定の期限なしの招待として使えるよう、
    // ここでは、Host の秘密だけを登録し、Editor / Viewer は、期限の長い招待として作る
    let creds = Credentials::new(TEST_HOST_TOKEN);
    creds.register_test_invite(TEST_EDITOR_TOKEN, UserRole::Editor);
    creds.register_test_invite(TEST_VIEWER_TOKEN, UserRole::Viewer);
    Arc::new(creds)
}

#[cfg(test)]
pub const TEST_HOST_TOKEN: &str = "test-host-token-0123456789";
#[cfg(test)]
pub const TEST_EDITOR_TOKEN: &str = "test-editor-token-0123456789";
#[cfg(test)]
pub const TEST_VIEWER_TOKEN: &str = "test-viewer-token-0123456789";

/// テスト用の共有状態（上の 3 つの秘密を登録し、すべての API を開く）
#[cfg(test)]
pub fn create_shared_state() -> Arc<AppStateShared> {
    create_shared_state_with(test_credentials(), Features::all())
}

pub fn create_shared_state_with(
    credentials: Arc<Credentials>,
    features: Features,
) -> Arc<AppStateShared> {
    let (tx, _rx) = broadcast::channel(100);
    let mut initial_state = AppState::default();
    generate_initial_1aon(&mut initial_state);

    let log_root = std::env::var("YUI_LOG_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("./logs"));

    let workspace_root = std::env::var("YUI_WORKSPACE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));

    Arc::new(AppStateShared {
        app_state: Mutex::new(initial_state),
        credentials,
        tx,
        log_root,
        workspace_root,
        features,
    })
}

/// 環境変数 `YUI_WEB_ROOT`（ブラウザ版のビルド結果のフォルダ。`index.html` を含むこと）。
/// 設定されていなければ `None`（配信しない）。設定されているのに使えないフォルダなら、エラー。
pub fn web_root_from_env() -> Result<Option<PathBuf>, String> {
    web_root_from(std::env::var_os("YUI_WEB_ROOT"))
}

fn web_root_from(value: Option<std::ffi::OsString>) -> Result<Option<PathBuf>, String> {
    match value {
        None => Ok(None),
        Some(v) if v.is_empty() => Ok(None),
        Some(v) => {
            let root = PathBuf::from(v);
            if !root.join("index.html").is_file() {
                return Err(format!(
                    "'{}' has no index.html. Build the web build first (scripts/build-web.sh) and point YUI_WEB_ROOT at its folder.",
                    root.display()
                ));
            }
            Ok(Some(root))
        }
    }
}

pub fn create_router(shared_state: Arc<AppStateShared>) -> Router {
    create_router_with_web(shared_state, None, OriginPolicy::default())
}

/// `web_root` があれば、そのフォルダ（ブラウザ版のビルド結果）を `/` から配信する（`/` は `index.html`）。
/// 見つからないパスは 404（ブラウザ版は 1 ページだけなので、ページ内の経路の書き換えは要らない）。
pub fn create_router_with_web(
    shared_state: Arc<AppStateShared>,
    web_root: Option<&StdPath>,
    policy: OriginPolicy,
) -> Router {
    let router = api_router(shared_state, policy);
    match web_root {
        None => router,
        Some(root) => {
            use tower_http::services::ServeDir;
            // `x.wasm.br` / `x.wasm.gz` が隣にあれば、ブラウザが受け付ける方式のものを返す（`scripts/build-web.sh` が作る）
            router.fallback_service(ServeDir::new(root).precompressed_br().precompressed_gzip())
        }
    }
}

fn api_router(shared_state: Arc<AppStateShared>, policy: OriginPolicy) -> Router {
    // 別のオリジンのページに `/api/` を使わせるのは、許可リスト（YUI_ALLOWED_ORIGINS）にあるものだけ。
    // 許可リストが空なら、CORS のヘッダーを付けない（ブラウザは、別のオリジンへの応答を読ませない）。
    let cors = if policy.allowed_origins.is_empty() {
        None
    } else {
        let origins: Vec<axum::http::HeaderValue> = policy
            .allowed_origins
            .iter()
            .filter_map(|o| o.parse().ok())
            .collect();
        Some(
            CorsLayer::new()
                .allow_origin(AllowOrigin::list(origins))
                .allow_methods(tower_http::cors::Any)
                .allow_headers(tower_http::cors::Any),
        )
    };
    let router = Router::new()
        .route("/api/state", get(get_app_state).post(update_app_state))
        .route("/api/structure", get(get_structure).post(update_structure))
        .route("/api/model/1aon/atoms", get(get_1aon_atoms))
        .route("/api/trajectory/frame/{index}", get(get_trajectory_frame))
        .route("/api/invites", get(list_invites).post(create_invite))
        .route("/api/invites/{id}", axum::routing::delete(revoke_invite))
        .route("/api/collab", get(collab_handler))
        .route("/api/logs", get(logs_handler))
        .route("/api/terminal", get(terminal_handler))
        .route("/api/fs/list", get(fs_list_handler))
        .route("/api/fs/read", get(fs_read_handler))
        .route("/api/assets/NotoSansJP.ttf", get(get_font))
        .with_state(shared_state.clone())
        // 内側: 認証（秘密 → ロール）。外側: Origin / Host の検査と記録（先に走る）
        .layer(axum::middleware::from_fn_with_state(
            Arc::new(auth::AuthState {
                credentials: shared_state.credentials.clone(),
                limiter: auth::RateLimiter::default(),
            }),
            auth::guard,
        ))
        .layer(axum::middleware::from_fn_with_state(
            Arc::new(policy),
            origin_guard::guard,
        ));
    match cors {
        Some(cors) => router.layer(cors),
        None => router,
    }
}

/// 現在の全体状態（AppState）を MessagePack バイナリで取得する
async fn get_app_state(
    State(state): State<Arc<AppStateShared>>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let app = state.app_state.lock().unwrap();
    let bytes = pack_app_state(&app, app.version as u8, Compression::None).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Serialization error: {}", e),
        )
    })?;
    Ok(([(header::CONTENT_TYPE, "application/msgpack")], bytes))
}

/// 全体状態（AppState）を MessagePack バイナリで更新する
async fn update_app_state(
    State(state): State<Arc<AppStateShared>>,
    Extension(auth): Extension<Auth>,
    body: Bytes,
) -> Result<StatusCode, (StatusCode, String)> {
    require_host(&auth)?;
    let (_version, _compression, new_state) = unpack_app_state(&body).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            format!("Invalid AppState msgpack: {}", e),
        )
    })?;
    let mut app = state.app_state.lock().unwrap();
    *app = new_state;
    Ok(StatusCode::OK)
}

/// 現在の平坦化分子データ（Molecule）を JSON で取得する
async fn get_structure(State(state): State<Arc<AppStateShared>>) -> Json<Molecule> {
    let app = state.app_state.lock().unwrap();
    Json(app.molecule.clone())
}

/// 平坦化分子データ（Molecule）を JSON で更新する
async fn update_structure(
    State(state): State<Arc<AppStateShared>>,
    Extension(auth): Extension<Auth>,
    Json(mol): Json<Molecule>,
) -> Result<StatusCode, (StatusCode, String)> {
    require_host(&auth)?;
    let mut app = state.app_state.lock().unwrap();
    app.molecule = mol;
    Ok(StatusCode::OK)
}

/// 招待を作る要求（`POST /api/invites`）
#[derive(Debug, serde::Deserialize)]
struct CreateInviteRequest {
    /// `editor` か `viewer`
    role: String,
    /// 有効な秒数（省略時は 1 時間。60 秒〜7 日）
    ttl_secs: Option<u64>,
}

/// 招待を作る応答。秘密は、ここでしか分からない
#[derive(Debug, serde::Serialize)]
struct CreateInviteResponse {
    token: String,
    #[serde(flatten)]
    invite: auth::InviteInfo,
}

/// 招待（Editor / Viewer の、期限つきの秘密）を作る。Host だけ。
async fn create_invite(
    State(state): State<Arc<AppStateShared>>,
    Extension(auth): Extension<Auth>,
    Json(request): Json<CreateInviteRequest>,
) -> Result<Json<CreateInviteResponse>, (StatusCode, String)> {
    require_host(&auth)?;
    let role = auth::parse_invite_role(&request.role).ok_or((
        StatusCode::BAD_REQUEST,
        "role must be 'editor' or 'viewer'".to_string(),
    ))?;
    let ttl = request.ttl_secs.unwrap_or(auth::INVITE_TTL_DEFAULT);
    use auth::InviteError;
    match state
        .credentials
        .create_invite_at(role, ttl, auth::now_secs())
    {
        Ok((token, invite)) => {
            tracing::info!("Invitation {} created ({})", invite.id, invite.role);
            Ok(Json(CreateInviteResponse { token, invite }))
        }
        Err(InviteError::TtlOutOfRange) => Err((
            StatusCode::BAD_REQUEST,
            format!(
                "ttl_secs must be between {} and {}",
                auth::INVITE_TTL_MIN,
                auth::INVITE_TTL_MAX
            ),
        )),
        Err(InviteError::TooManyActive) => Err((
            StatusCode::TOO_MANY_REQUESTS,
            format!(
                "at most {} invitations can be active",
                auth::INVITE_MAX_ACTIVE
            ),
        )),
        Err(InviteError::RoleNotInvitable) => Err((
            StatusCode::BAD_REQUEST,
            "role must be 'editor' or 'viewer'".to_string(),
        )),
        Err(InviteError::NoRandomSource) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "no secure random source".to_string(),
        )),
    }
}

/// 有効な招待の一覧（秘密は含まない）。Host だけ。
async fn list_invites(
    State(state): State<Arc<AppStateShared>>,
    Extension(auth): Extension<Auth>,
) -> Result<Json<Vec<auth::InviteInfo>>, (StatusCode, String)> {
    require_host(&auth)?;
    Ok(Json(state.credentials.list_invites_at(auth::now_secs())))
}

/// 招待を取り消す。その招待で開いている接続も、数秒のうちに閉じる。Host だけ。
async fn revoke_invite(
    State(state): State<Arc<AppStateShared>>,
    Extension(auth): Extension<Auth>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    require_host(&auth)?;
    if state.credentials.revoke(&id) {
        tracing::info!("Invitation {id} revoked");
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "no such invitation".to_string()))
    }
}

/// 状態を書き換える API は、Host のロールだけ
fn require_host(auth: &Auth) -> Result<(), (StatusCode, String)> {
    if auth.role == UserRole::Host {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            "Forbidden: this action requires the Host role".to_string(),
        ))
    }
}

fn generate_initial_1aon(state: &mut AppState) {
    let mut atoms = Vec::with_capacity(60000);
    let mut id_counter = 1u32;

    let rings = [
        (7, -30.0f32, 45.0f32, "GroEL-RingA", 'A'),
        (7, 10.0f32, 45.0f32, "GroEL-RingB", 'B'),
        (7, 45.0f32, 30.0f32, "GroES-RingC", 'C'),
    ];

    for (num_subunits, z_offset, ring_radius, _name, chain_prefix) in rings.iter() {
        for s in 0..*num_subunits {
            let angle = (s as f32) * (2.0 * std::f32::consts::PI / (*num_subunits as f32));
            let sub_x = ring_radius * angle.cos();
            let sub_y = ring_radius * angle.sin();
            let chain_id = format!("{}{}", chain_prefix, s);
            let atoms_per_subunit = 2760;

            for a in 0..atoms_per_subunit {
                let theta = (a as f32) * 0.15;
                let radius_offset = 5.0 + (a as f32) * 0.005;
                let x = sub_x + radius_offset * theta.cos();
                let y = sub_y + radius_offset * theta.sin();
                let z = z_offset + (a as f32) * 0.02 - 25.0;

                let (element, color, radius) = match a % 4 {
                    0 => ("C".to_string(), [0.2, 0.7, 0.3, 1.0], 1.7),
                    1 => ("N".to_string(), [0.2, 0.3, 0.9, 1.0], 1.55),
                    2 => ("O".to_string(), [0.9, 0.2, 0.2, 1.0], 1.52),
                    _ => ("S".to_string(), [0.9, 0.8, 0.2, 1.0], 1.8),
                };

                atoms.push(Atom {
                    id: id_counter,
                    element,
                    position: [x, y, z],
                    radius,
                    color,
                    residue_name: "GLY".to_string(),
                    residue_index: (a / 15) + 1,
                    chain_id: chain_id.clone(),
                });
                id_counter += 1;
            }
        }
    }

    state.molecule = Molecule {
        atoms,
        bonds: vec![],
    };
    state.annotations = vec![
        Annotation {
            id: "ann-1".to_string(),
            text: "GroEL Upper Ring".to_string(),
            position: [0.0, 0.0, 15.0],
            color: [1.0, 1.0, 1.0, 1.0],
            visible: true,
            target_atom_id: None,
        },
        Annotation {
            id: "ann-2".to_string(),
            text: "GroES Lid".to_string(),
            position: [0.0, 0.0, 48.0],
            color: [0.0, 1.0, 1.0, 1.0],
            visible: true,
            target_atom_id: None,
        },
    ];
}

async fn get_1aon_atoms(State(state): State<Arc<AppStateShared>>) -> Json<Molecule> {
    let app = state.app_state.lock().unwrap();
    Json(app.molecule.clone())
}

async fn get_trajectory_frame(
    Path(index): Path<u32>,
    State(state): State<Arc<AppStateShared>>,
) -> Result<Json<Vec<[f32; 3]>>, (StatusCode, String)> {
    let num_atoms = {
        let app = state.app_state.lock().unwrap();
        app.molecule.atoms.len()
    };

    if num_atoms == 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            "No active molecule loaded.".to_string(),
        ));
    }

    let file_path = "demo_trajectory.xyz";

    if !std::path::Path::new(file_path).exists() {
        let _ = yui_core::trajectory::create_dummy_trajectory_file(file_path, 50, num_atoms).await;
    }

    match yui_core::trajectory::load_frame_from_file(file_path, index, num_atoms).await {
        Ok(frame_data) => Ok(Json(frame_data.positions)),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to parse frame: {}", e),
        )),
    }
}

async fn collab_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppStateShared>>,
    Extension(auth): Extension<Auth>,
) -> Response {
    let role = auth.role;
    let invite_id = auth.invite_id.clone();
    auth.select_protocol(ws)
        .on_upgrade(move |socket| handle_socket(socket, state, role, invite_id))
}

/// 共同作業の接続。ロールは、接続を受け付けたときに決まった `role` で、パケットのヘッダーの `role_token` は見ない。
async fn handle_socket(
    socket: WebSocket,
    state: Arc<AppStateShared>,
    role: UserRole,
    invite_id: Option<String>,
) {
    use futures_util::{SinkExt, StreamExt};

    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.tx.subscribe();

    // 接続時に現在の状態のスナップショット差分（初期同期）をクライアントへ送信
    {
        let initial_delta = {
            let app = state.app_state.lock().unwrap();
            AppStateDelta {
                camera: Some(app.camera.clone()),
                scene: Some(app.scene.clone()),
                representations: Some(app.representations.clone()),
                annotations: Some(app.annotations.clone()),
                laser_pointer: Some(app.laser_pointer.clone()),
                named_selections: Some(app.named_selections.clone()),
            }
        };
        let header = CollabHeader {
            sender_id: [0u8; 16],
            role_token: 1, // Server as Host
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        };
        if let Ok(packet) = pack_collab_packet(&header, &initial_delta) {
            let _ = sender.send(Message::Binary(packet.into())).await;
        }
    }

    let mut send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if sender.send(Message::Binary(msg.into())).await.is_err() {
                break;
            }
        }
    });

    let state_clone = state.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(Message::Binary(bin_data))) = receiver.next().await {
            // pack_collab_packet ベースの差分同期デコード
            if let Ok((header, delta)) = unpack_collab_packet(&bin_data) {
                // ヘッダーの `role_token`（古いクライアントが、固定の番号を入れる）は、使わない
                let _ = header.role_token;

                // ロール権限チェック
                if verify_role_action(role, &delta) {
                    {
                        let mut app = state_clone.app_state.lock().unwrap();
                        apply_app_state_delta(&mut app, &delta);
                    }
                    // 検証済みパケットを他クライアントへブロードキャスト
                    let _ = state_clone.tx.send(bin_data.to_vec());
                } else {
                    tracing::warn!(
                        "Rejecting collab packet: unauthorized action for role {:?}",
                        role
                    );
                }
            } else {
                tracing::warn!("Received malformed collaboration packet");
            }
        }
    });

    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
        // 招待で入った接続は、取り消されたり期限が切れたりしたら、閉じる
        _ = invite_gone(state.credentials.clone(), invite_id) => {
            send_task.abort();
            recv_task.abort();
        }
    };
}

/// 招待で開いた接続が、取り消し・期限切れになるまで待つ（招待でない接続では、終わらない）。
/// 取り消しは、通知を受けてすぐ。期限切れは、期限の時刻まで眠って起きる（間隔をおいた確認はしない）。
async fn invite_gone(credentials: Arc<Credentials>, invite_id: Option<String>) {
    use tokio::sync::broadcast::error::RecvError;
    let Some(id) = invite_id else {
        return std::future::pending().await;
    };
    // 先に受け口を作る（確認と待ちの間の取り消しを、取りこぼさないため）
    let mut revoked = credentials.subscribe_revoked();
    loop {
        let Some(expires_at) = credentials.invite_expires_at(&id) else {
            tracing::info!("Closing a connection: its invitation ended");
            return;
        };
        let wait = expires_at.saturating_sub(auth::now_secs());
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_secs(wait)) => {
                if !credentials.invite_active_at(&id, auth::now_secs()) {
                    tracing::info!("Closing a connection: its invitation expired");
                    return;
                }
            }
            got = revoked.recv() => match got {
                Ok(gone) if gone == id => {
                    tracing::info!("Closing a connection: its invitation was revoked");
                    return;
                }
                // ほかの招待の取り消し、または取りこぼし: 状態を見直して、待ち直す
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => return std::future::pending().await,
            },
        }
    }
}

async fn get_font() -> Result<impl axum::response::IntoResponse, (StatusCode, String)> {
    let font_path = "frontend/assets/NotoSansJP.ttf";
    match tokio::fs::read(font_path).await {
        Ok(bytes) => {
            let mut headers = axum::http::HeaderMap::new();
            headers.insert(
                axum::http::header::CONTENT_TYPE,
                axum::http::HeaderValue::from_static("font/ttf"),
            );
            Ok((headers, bytes))
        }
        Err(e) => Err((StatusCode::NOT_FOUND, format!("Font not found: {}", e))),
    }
}

/// パスが許可されたルートディレクトリ（`root_dir`）配下であることを検証して解決する。
///
/// パストラバーサル（`..` による脱出、絶対パス等）を防止し、
/// 正規化（canonicalize）後の実パスが `root_dir` 配下に存在することを保証する。
pub fn resolve_secure_path(root_dir: &StdPath, requested_path: &str) -> Result<PathBuf, String> {
    let req_path = StdPath::new(requested_path);

    // 1. 不正コンポーネント（親ディレクトリ脱出、絶対パス、Windowsドライブ文字等）の静的検査
    for comp in req_path.components() {
        match comp {
            Component::ParentDir => {
                return Err("Parent directory traversal ('..') is strictly forbidden".to_string());
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(
                    "Absolute path is not permitted; must be relative to root directory"
                        .to_string(),
                );
            }
            _ => {}
        }
    }

    // 2. root_dir の準備と正規化
    if !root_dir.exists() {
        std::fs::create_dir_all(root_dir)
            .map_err(|e| format!("Failed to create root directory: {e}"))?;
    }
    let canonical_root = root_dir
        .canonicalize()
        .map_err(|e| format!("Failed to canonicalize root directory: {e}"))?;

    // 3. 結合パスの構築
    let target = canonical_root.join(req_path);

    // 4. 実在する場合の canonicalize 照合（シンボリックリンクによる脱出も防止）
    if target.exists() {
        let canonical_target = target
            .canonicalize()
            .map_err(|e| format!("Failed to canonicalize target path: {e}"))?;

        if !canonical_target.starts_with(&canonical_root) {
            return Err("Target path resolves outside allowed root directory".to_string());
        }
        return Ok(canonical_target);
    }

    // 5. 未作成ファイルの場合: 親ディレクトリの正規化と境界検査
    if let Some(parent) = target.parent()
        && parent.exists()
    {
        let canonical_parent = parent
            .canonicalize()
            .map_err(|e| format!("Failed to canonicalize parent directory: {e}"))?;

        if !canonical_parent.starts_with(&canonical_root) {
            return Err(
                "Target parent directory resolves outside allowed root directory".to_string(),
            );
        }
        let file_name = target
            .file_name()
            .ok_or_else(|| "Target must have a valid file name".to_string())?;
        return Ok(canonical_parent.join(file_name));
    }

    Ok(target)
}

/// ログストリーミング用クエリパラメータ
#[derive(Debug, serde::Deserialize)]
pub struct LogQueryParams {
    pub path: String,
    pub lines: Option<usize>,
}

/// ログファイル監視 WebSocket アップグレードハンドラ
async fn logs_handler(
    Query(params): Query<LogQueryParams>,
    State(state): State<Arc<AppStateShared>>,
    Extension(auth): Extension<Auth>,
    ws: Result<WebSocketUpgrade, axum::extract::ws::rejection::WebSocketUpgradeRejection>,
) -> Response {
    // 1. 認証は `auth::guard` が済ませている。ログビューワーは読み取り専用なので、Viewer / Editor / Host いずれも許可

    // 2. パストラバーサル防止と許可ディレクトリ配下の検証
    let secure_path = match resolve_secure_path(&state.log_root, &params.path) {
        Ok(p) => p,
        Err(err) => {
            return (
                StatusCode::FORBIDDEN,
                format!("Forbidden: path traversal or outside allowed root: {err}"),
            )
                .into_response();
        }
    };

    // 3. WebSocket Upgrade の抽出結果検証
    let ws = match ws {
        Ok(ws) => auth.select_protocol(ws),
        Err(rejection) => return rejection.into_response(),
    };

    let initial_lines = params.lines.unwrap_or(100);
    let credentials = state.credentials.clone();
    let invite_id = auth.invite_id.clone();
    ws.on_upgrade(move |socket| {
        handle_logs_socket(socket, secure_path, initial_lines, credentials, invite_id)
    })
}

async fn handle_logs_socket(
    mut socket: WebSocket,
    target_path: PathBuf,
    initial_lines: usize,
    credentials: Arc<Credentials>,
    invite_id: Option<String>,
) {
    let mut rx = spawn_log_stream(target_path, initial_lines);
    let ended = invite_gone(credentials, invite_id);
    tokio::pin!(ended);

    loop {
        tokio::select! {
            _ = &mut ended => break,
            msg = rx.recv() => {
                let Some(msg) = msg else { break };
                if let Ok(json_text) = serde_json::to_string(&msg)
                    && socket.send(Message::Text(json_text.into())).await.is_err()
                {
                    break;
                }
            }
        }
    }
}

/// ターミナルセッション接続用クエリパラメータ
#[derive(Debug, serde::Deserialize)]
pub struct TerminalQueryParams {
    pub rows: Option<u16>,
    pub cols: Option<u16>,
}

/// ターミナル WebSocket アップグレードハンドラ
///
/// 任意コマンド実行権限を伴うため、アクセス制御は厳格に Host ロール（token=1）のみに限定する。
/// （architecture.md 13.2節、7.1節）
async fn terminal_handler(
    Query(params): Query<TerminalQueryParams>,
    State(state): State<Arc<AppStateShared>>,
    Extension(auth): Extension<Auth>,
    ws: Result<WebSocketUpgrade, axum::extract::ws::rejection::WebSocketUpgradeRejection>,
) -> Response {
    // 1. 認証は `auth::guard` が済ませている
    if !state.features.terminal {
        return (
            StatusCode::FORBIDDEN,
            "Forbidden: the terminal is disabled on this backend (set YUI_ENABLE_TERMINAL=1)",
        )
            .into_response();
    }

    // ターミナルセッションはホストマシン上での任意コマンド実行を許可するため、
    // 既定では Host ロールのみに限定する（Editor / Viewer は明示的に拒絶）
    if auth.role != UserRole::Host {
        return (
            StatusCode::FORBIDDEN,
            "Forbidden: terminal session requires Host role",
        )
            .into_response();
    }

    // 2. WebSocket Upgrade の抽出結果検証
    let ws = match ws {
        Ok(ws) => auth.select_protocol(ws),
        Err(rejection) => return rejection.into_response(),
    };

    let rows = params.rows.unwrap_or(24);
    let cols = params.cols.unwrap_or(80);

    ws.on_upgrade(move |socket| handle_terminal_socket(socket, rows, cols))
}

/// クライアントから送信される JSON 制御メッセージ
#[derive(Debug, serde::Deserialize)]
#[serde(tag = "type")]
enum ClientTerminalMessage {
    #[serde(rename = "input")]
    Input { data: String },
    #[serde(rename = "resize")]
    Resize { rows: u16, cols: u16 },
}

async fn handle_terminal_socket(socket: WebSocket, rows: u16, cols: u16) {
    use futures_util::{SinkExt, StreamExt};

    let session = match spawn_pty_session(rows, cols) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to spawn PTY session: {}", e);
            return;
        }
    };

    let (mut ws_sender, mut ws_receiver) = socket.split();
    let mut output_rx = session.output_rx;
    let input_tx = session.input_tx;

    // タスク 1: PTY からの出力を WebSocket へ転送 (TerminalOutput)
    let pty_to_ws = tokio::spawn(async move {
        while let Some(data) = output_rx.recv().await {
            if ws_sender.send(Message::Binary(data.into())).await.is_err() {
                break;
            }
        }
    });

    // タスク 2: WebSocket からの入力を PTY へ転送 (TerminalInput)
    let ws_to_pty = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_receiver.next().await {
            match msg {
                Message::Binary(bytes) => {
                    if input_tx
                        .send(TerminalCommand::Input(bytes.to_vec()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Message::Text(text) => {
                    // JSON 制御メッセージ（リサイズ等）を試行
                    if let Ok(client_msg) = serde_json::from_str::<ClientTerminalMessage>(&text) {
                        match client_msg {
                            ClientTerminalMessage::Input { data } => {
                                if input_tx
                                    .send(TerminalCommand::Input(data.into_bytes()))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                            ClientTerminalMessage::Resize { rows, cols } => {
                                if input_tx
                                    .send(TerminalCommand::Resize { rows, cols })
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                        }
                    } else {
                        // プレーンテキスト入力として扱う
                        if input_tx
                            .send(TerminalCommand::Input(text.as_bytes().to_vec()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
                Message::Close(_) => {
                    break;
                }
                _ => {}
            }
        }
    });

    // いずれかのタスクが終了したら終了
    tokio::select! {
        _ = pty_to_ws => {},
        _ = ws_to_pty => {},
    }
}

/// ファイルシステム操作用クエリパラメータ (Phase 6f-ii)
#[derive(Debug, serde::Deserialize)]
pub struct FsQueryParams {
    /// workspace_root からの相対パス。省略時はルート自身を指す。
    #[serde(default)]
    pub path: String,
}

/// ファイルシステムエントリ情報 (Phase 6f-ii)
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FsEntry {
    pub name: String,
    pub is_dir: bool,
}

/// ファイルシステム一覧取得ハンドラ (GET /api/fs/list)
async fn fs_list_handler(
    Query(params): Query<FsQueryParams>,
    State(state): State<Arc<AppStateShared>>,
) -> Response {
    // 1. 認証は `auth::guard` が済ませている
    if !state.features.fs {
        return (
            StatusCode::FORBIDDEN,
            "Forbidden: file access is disabled on this backend (set YUI_ENABLE_FS=1)",
        )
            .into_response();
    }

    // 2. パストラバーサル防止と許可ディレクトリ配下の検証
    let secure_path = match resolve_secure_path(&state.workspace_root, &params.path) {
        Ok(p) => p,
        Err(err) => {
            return (
                StatusCode::FORBIDDEN,
                format!("Forbidden: path traversal or outside allowed root: {err}"),
            )
                .into_response();
        }
    };

    // 3. 存在確認
    if !secure_path.exists() {
        return (
            StatusCode::NOT_FOUND,
            format!("Not found: path does not exist: {}", params.path),
        )
            .into_response();
    }

    // 4. ディレクトリ確認
    if !secure_path.is_dir() {
        return (
            StatusCode::BAD_REQUEST,
            format!("Bad Request: '{}' is not a directory", params.path),
        )
            .into_response();
    }

    // 5. 1階層分のみ列挙（再帰なし）、ソート
    let read_dir = match std::fs::read_dir(&secure_path) {
        Ok(rd) => rd,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to read directory: {e}"),
            )
                .into_response();
        }
    };

    let mut entries = Vec::new();
    for entry_res in read_dir {
        let Ok(entry) = entry_res else { continue };
        let Ok(meta) = entry.metadata() else { continue };
        let name = entry.file_name().to_string_lossy().to_string();
        entries.push(FsEntry {
            name,
            is_dir: meta.is_dir(),
        });
    }

    entries.sort_by(|a, b| a.name.cmp(&b.name));

    Json(entries).into_response()
}

/// ファイル読み込みハンドラ (GET /api/fs/read)
async fn fs_read_handler(
    Query(params): Query<FsQueryParams>,
    State(state): State<Arc<AppStateShared>>,
) -> Response {
    // 1. 認証は `auth::guard` が済ませている
    if !state.features.fs {
        return (
            StatusCode::FORBIDDEN,
            "Forbidden: file access is disabled on this backend (set YUI_ENABLE_FS=1)",
        )
            .into_response();
    }

    // 2. パストラバーサル防止と許可ディレクトリ配下の検証
    let secure_path = match resolve_secure_path(&state.workspace_root, &params.path) {
        Ok(p) => p,
        Err(err) => {
            return (
                StatusCode::FORBIDDEN,
                format!("Forbidden: path traversal or outside allowed root: {err}"),
            )
                .into_response();
        }
    };

    // 3. 存在確認
    if !secure_path.exists() {
        return (
            StatusCode::NOT_FOUND,
            format!("Not found: path does not exist: {}", params.path),
        )
            .into_response();
    }

    // 4. ファイル確認（ディレクトリだった場合は拒否）
    if secure_path.is_dir() {
        return (
            StatusCode::BAD_REQUEST,
            format!("Bad Request: '{}' is a directory, not a file", params.path),
        )
            .into_response();
    }

    // 5. 非同期読み込みとバイト列返却
    match tokio::fs::read(&secure_path).await {
        Ok(bytes) => ([(header::CONTENT_TYPE, "application/octet-stream")], bytes).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to read file: {e}"),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    use yui_core::role::RoleResolver;
    use yui_core::state::CameraState;
    use yui_core::{AtomGroup, AtomGroupExt};

    /// 招待で開いた接続は、取り消しですぐ閉じ、期限切れで期限の時刻に閉じる。Host の接続と、ほかの招待の取り消しでは、閉じない
    #[tokio::test]
    async fn test_invite_connections_end_with_the_invitation() {
        use std::time::{Duration, Instant};
        let creds = Arc::new(Credentials::new("h".repeat(40).as_str()));
        let now = auth::now_secs();
        creds.register_test_invite_until("a", UserRole::Viewer, now + 3600);
        creds.register_test_invite_until("b", UserRole::Editor, now + 3600);
        creds.register_test_invite_until("soon", UserRole::Viewer, now + 2);

        // Host の接続（招待なし）は、終わらない
        let host = invite_gone(creds.clone(), None);
        assert!(
            tokio::time::timeout(Duration::from_millis(200), host)
                .await
                .is_err()
        );

        // 取り消すと、すぐ（5 秒の確認を待たずに）閉じる。ほかの招待の取り消しでは、閉じない
        let a = tokio::spawn(invite_gone(creds.clone(), Some("a".into())));
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(creds.revoke("b"));
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!a.is_finished(), "revoking another invitation closed it");
        let t = Instant::now();
        assert!(creds.revoke("a"));
        tokio::time::timeout(Duration::from_secs(2), a)
            .await
            .expect("not closed after the revocation")
            .unwrap();
        assert!(
            t.elapsed() < Duration::from_millis(500),
            "{:?}",
            t.elapsed()
        );

        // 取り消された後に開こうとした接続は、すぐ終わる
        tokio::time::timeout(
            Duration::from_secs(1),
            invite_gone(creds.clone(), Some("a".into())),
        )
        .await
        .expect("a revoked invitation must end at once");

        // 期限切れは、期限の時刻に閉じる（2 秒の招待）
        let t = Instant::now();
        tokio::time::timeout(
            Duration::from_secs(5),
            invite_gone(creds.clone(), Some("soon".into())),
        )
        .await
        .expect("not closed at the expiry");
        assert!(
            t.elapsed() <= Duration::from_millis(2600),
            "{:?}",
            t.elapsed()
        );
    }

    #[tokio::test]
    async fn test_the_web_build_is_served_from_web_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("dist");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("index.html"), "<html>YUI-INDEX</html>").unwrap();
        std::fs::write(root.join("app_bg.wasm"), b"\0asm\x01\0\0\0").unwrap();
        std::fs::write(dir.path().join("secret.txt"), "TOP-SECRET").unwrap();

        let app =
            create_router_with_web(create_shared_state(), Some(&root), OriginPolicy::default());
        let get = |uri: &'static str| {
            let app = app.clone();
            async move {
                let res = app
                    .oneshot(
                        Request::builder()
                            .uri(uri)
                            .body(axum::body::Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                let status = res.status();
                let ctype = res
                    .headers()
                    .get(header::CONTENT_TYPE)
                    .map(|v| v.to_str().unwrap().to_string());
                let body = res.into_body().collect().await.unwrap().to_bytes();
                (status, ctype, String::from_utf8_lossy(&body).to_string())
            }
        };

        // `/` と `/index.html` は、ブラウザ版の入口
        for uri in ["/", "/index.html"] {
            let (status, ctype, body) = get(uri).await;
            assert_eq!(status, StatusCode::OK, "{uri}");
            assert!(body.contains("YUI-INDEX"), "{uri}: {body}");
            assert!(ctype.unwrap().starts_with("text/html"), "{uri}");
        }
        // wasm は、ストリーミングで読み込めるよう、application/wasm で返す
        let (status, ctype, _) = get("/app_bg.wasm").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(ctype.as_deref(), Some("application/wasm"));
        // 隣に圧縮したものがあれば、受け付けるブラウザには、それを（元の種類のまま）返す。受け付けなければ、元のまま
        std::fs::write(root.join("app_bg.wasm.gz"), b"GZ-BYTES").unwrap();
        std::fs::write(root.join("app_bg.wasm.br"), b"BR-BYTES").unwrap();
        let fetch = |accept: &'static str| {
            let app = app.clone();
            async move {
                let mut req = Request::builder().uri("/app_bg.wasm");
                if !accept.is_empty() {
                    req = req.header(header::ACCEPT_ENCODING, accept);
                }
                let res = app
                    .oneshot(req.body(axum::body::Body::empty()).unwrap())
                    .await
                    .unwrap();
                let h = |n| {
                    res.headers()
                        .get(n)
                        .map(|v| v.to_str().unwrap().to_string())
                };
                let (enc, ctype) = (h(header::CONTENT_ENCODING), h(header::CONTENT_TYPE));
                let body = res.into_body().collect().await.unwrap().to_bytes();
                (enc, ctype, body.to_vec())
            }
        };
        let (enc, ctype, body) = fetch("gzip, br").await;
        assert_eq!(
            (enc.as_deref(), body.as_slice()),
            (Some("br"), &b"BR-BYTES"[..])
        );
        assert_eq!(ctype.as_deref(), Some("application/wasm"));
        let (enc, _, body) = fetch("gzip").await;
        assert_eq!(
            (enc.as_deref(), body.as_slice()),
            (Some("gzip"), &b"GZ-BYTES"[..])
        );
        let (enc, _, body) = fetch("").await;
        assert_eq!((enc, body.as_slice()), (None, &b"\0asm\x01\0\0\0"[..]));
        // 知らないパスは 404
        let (status, _, body) = get("/some/page").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(!body.contains("YUI-INDEX"));
        // フォルダの外は配信しない（親のフォルダのファイルは返らない）
        for uri in ["/../secret.txt", "/%2e%2e/secret.txt", "/..%2fsecret.txt"] {
            let (_, _, body) = get(uri).await;
            assert!(
                !body.contains("TOP-SECRET"),
                "{uri} leaked the parent folder"
            );
        }
        // API は、秘密がなければ 401（静的な一式は、秘密なしで配信する）。知らない API のパスも、先に認証を求める
        let (status, _, _) = get("/api/state").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, _, body) = get("/api/nothing-here").await;
        assert!(!body.contains("YUI-INDEX"), "{status}");

        // 配信しない設定（既定）では、`/` に配るものはない（秘密がなければ、認証が先に断る）
        let plain = create_router(create_shared_state());
        let res = plain
            .oneshot(
                Request::builder()
                    .uri("/")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    /// すべての `/api/`（同梱のフォントを除く）は、秘密を求める。秘密がなければ 401、知らない秘密は 403。
    /// 状態を書き換える API は Host のロールだけ。秘密は、ヘッダー（ブラウザの WebSocket は、サブプロトコル）で渡す。
    #[tokio::test]
    async fn test_api_requires_a_secret_and_roles_are_enforced() {
        let app = || create_router(create_shared_state());
        let send = |method: &'static str, uri: &'static str, auth: Option<String>| {
            let app = app();
            async move {
                let mut b = Request::builder().method(method).uri(uri);
                if let Some(a) = auth {
                    b = b.header("authorization", a);
                }
                app.oneshot(b.body(axum::body::Body::empty()).unwrap())
                    .await
                    .unwrap()
                    .status()
            }
        };
        let bearer = |t: &str| Some(format!("Bearer {t}"));
        // 秘密なし → 401。知らない秘密と、以前の固定の番号 → 403
        assert_eq!(
            send("GET", "/api/state", None).await,
            StatusCode::UNAUTHORIZED
        );
        for bad in ["bad-token-0123456789", "1", "2", "3"] {
            assert_eq!(
                send("GET", "/api/state", bearer(bad)).await,
                StatusCode::FORBIDDEN,
                "{bad:?}"
            );
        }
        // URL の `?token=` は、もう秘密として扱わない
        assert_eq!(
            send("GET", "/api/state?token=1", None).await,
            StatusCode::UNAUTHORIZED
        );
        // 読む API は、どのロールも通る
        for t in [TEST_HOST_TOKEN, TEST_EDITOR_TOKEN, TEST_VIEWER_TOKEN] {
            assert_eq!(send("GET", "/api/state", bearer(t)).await, StatusCode::OK);
            assert_eq!(
                send("GET", "/api/structure", bearer(t)).await,
                StatusCode::OK
            );
        }
        // 状態を書き換える API は、Host だけ（中身が正しくても、Editor と Viewer は 403）
        let valid_state = pack_app_state(&AppState::default(), 0, Compression::None).unwrap();
        let valid_structure = serde_json::to_vec(&AppState::default().molecule).unwrap();
        for t in [TEST_EDITOR_TOKEN, TEST_VIEWER_TOKEN] {
            for (uri, ctype, body) in [
                ("/api/state", "application/msgpack", valid_state.clone()),
                (
                    "/api/structure",
                    "application/json",
                    valid_structure.clone(),
                ),
            ] {
                let res = app()
                    .oneshot(
                        Request::builder()
                            .method("POST")
                            .uri(uri)
                            .header("authorization", format!("Bearer {t}"))
                            .header("content-type", ctype)
                            .body(axum::body::Body::from(body))
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(res.status(), StatusCode::FORBIDDEN, "{uri} {t}");
            }
        }
        // 同梱のフォントは、秘密なしで取れる（認証の対象外。ファイルがなければ 404 でも、401 ではない）
        assert_ne!(
            send("GET", "/api/assets/NotoSansJP.ttf", None).await,
            StatusCode::UNAUTHORIZED
        );
    }

    /// Host は、Editor / Viewer の期限つきの招待を作り、一覧し、取り消せる。招待の秘密は、そのロールで通り、
    /// 取り消すと通らなくなる。招待の API は、Host だけ。
    #[tokio::test]
    async fn test_host_manages_invitations() {
        let app = create_router(create_shared_state());
        let call = |app: Router,
                    method: &'static str,
                    uri: String,
                    token: &'static str,
                    body: Option<&'static str>| async move {
            let mut b = Request::builder()
                .method(method)
                .uri(uri)
                .header("authorization", format!("Bearer {token}"));
            let body = match body {
                Some(j) => {
                    b = b.header("content-type", "application/json");
                    axum::body::Body::from(j)
                }
                None => axum::body::Body::empty(),
            };
            let res = app.oneshot(b.body(body).unwrap()).await.unwrap();
            let status = res.status();
            let bytes = res.into_body().collect().await.unwrap().to_bytes();
            (status, String::from_utf8_lossy(&bytes).to_string())
        };
        // Host が Viewer の招待を作る
        let (st, body) = call(
            app.clone(),
            "POST",
            "/api/invites".into(),
            TEST_HOST_TOKEN,
            Some(r#"{"role":"viewer","ttl_secs":600}"#),
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{body}");
        let made: serde_json::Value = serde_json::from_str(&body).unwrap();
        let token = made["token"].as_str().unwrap().to_string();
        let id = made["id"].as_str().unwrap().to_string();
        assert_eq!(token.len(), 64);
        assert_eq!(made["role"], "viewer");
        assert!(made["expires_at"].as_u64().unwrap() > auth::now_secs());
        // 招待の秘密は、Viewer として通る（読めるが、書けず、招待も作れない）
        let token: &'static str = Box::leak(token.into_boxed_str());
        let (st, _) = call(app.clone(), "GET", "/api/state".into(), token, None).await;
        assert_eq!(st, StatusCode::OK);
        let (st, _) = call(
            app.clone(),
            "POST",
            "/api/invites".into(),
            token,
            Some(r#"{"role":"viewer"}"#),
        )
        .await;
        assert_eq!(st, StatusCode::FORBIDDEN);
        let (st, _) = call(app.clone(), "GET", "/api/invites".into(), token, None).await;
        assert_eq!(st, StatusCode::FORBIDDEN);
        let (st, _) = call(
            app.clone(),
            "DELETE",
            format!("/api/invites/{id}"),
            token,
            None,
        )
        .await;
        assert_eq!(st, StatusCode::FORBIDDEN);
        // 一覧に出る（秘密は出ない）
        let (st, body) = call(
            app.clone(),
            "GET",
            "/api/invites".into(),
            TEST_HOST_TOKEN,
            None,
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        assert!(body.contains(&id) && !body.contains(token), "{body}");
        // 取り消すと、秘密は通らなくなる。2 度目は 404
        let (st, _) = call(
            app.clone(),
            "DELETE",
            format!("/api/invites/{id}"),
            TEST_HOST_TOKEN,
            None,
        )
        .await;
        assert_eq!(st, StatusCode::NO_CONTENT);
        let (st, _) = call(app.clone(), "GET", "/api/state".into(), token, None).await;
        assert_eq!(st, StatusCode::FORBIDDEN);
        let (st, _) = call(
            app.clone(),
            "DELETE",
            format!("/api/invites/{id}"),
            TEST_HOST_TOKEN,
            None,
        )
        .await;
        assert_eq!(st, StatusCode::NOT_FOUND);
        // 入力の検査: Host の招待、知らないロール、短すぎる・長すぎる期限
        for bad in [
            r#"{"role":"host"}"#,
            r#"{"role":"admin"}"#,
            r#"{"role":"viewer","ttl_secs":5}"#,
            r#"{"role":"viewer","ttl_secs":99999999}"#,
        ] {
            let (st, _) = call(
                app.clone(),
                "POST",
                "/api/invites".into(),
                TEST_HOST_TOKEN,
                Some(bad),
            )
            .await;
            assert_eq!(st, StatusCode::BAD_REQUEST, "{bad}");
        }
        // 既定の期限は 1 時間、Editor も作れる
        let (st, body) = call(
            app.clone(),
            "POST",
            "/api/invites".into(),
            TEST_HOST_TOKEN,
            Some(r#"{"role":"editor"}"#),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        let made: serde_json::Value = serde_json::from_str(&body).unwrap();
        let ttl = made["expires_at"].as_u64().unwrap() - auth::now_secs();
        assert!((3590..=3600).contains(&ttl), "{ttl}");
        assert_eq!(made["role"], "editor");
    }

    /// 同じ接続元から、知らない秘密が続けて断られると、正しい秘密も含めて、しばらく 429 になる。
    /// 秘密がない要求（401）は、数えない。
    #[tokio::test]
    async fn test_repeated_bad_tokens_lock_the_peer_out() {
        let app = create_router(create_shared_state());
        let send = |app: Router, auth: Option<&'static str>| async move {
            let mut b = Request::builder().uri("/api/state");
            if let Some(a) = auth {
                b = b.header("authorization", a);
            }
            let res = app
                .oneshot(b.body(axum::body::Body::empty()).unwrap())
                .await
                .unwrap();
            (res.status(), res.headers().get("retry-after").cloned())
        };
        let good = "Bearer test-host-token-0123456789";
        // 秘密がない要求は、何度でも 401（締め出しに数えない）
        for _ in 0..(auth::MAX_FAILURES + 5) {
            assert_eq!(send(app.clone(), None).await.0, StatusCode::UNAUTHORIZED);
        }
        assert_eq!(send(app.clone(), Some(good)).await.0, StatusCode::OK);
        // 知らない秘密は、上限まで 403
        for _ in 0..auth::MAX_FAILURES {
            assert_eq!(
                send(app.clone(), Some("Bearer wrong-token-0123456789"))
                    .await
                    .0,
                StatusCode::FORBIDDEN
            );
        }
        // そのあとは、正しい秘密でも 429（Retry-After つき）
        let (st, retry) = send(app.clone(), Some(good)).await;
        assert_eq!(st, StatusCode::TOO_MANY_REQUESTS);
        let retry: u64 = retry.unwrap().to_str().unwrap().parse().unwrap();
        assert!((1..=auth::LOCKOUT_SECS).contains(&retry), "{retry}");
        // 別のルーター（別の記録）は、影響を受けない
        let other = create_router(create_shared_state());
        assert_eq!(send(other, Some(good)).await.0, StatusCode::OK);
    }

    /// ブラウザは WebSocket にヘッダーを付けられないので、サブプロトコル `yui.token.<秘密>` で渡し、
    /// 受け付けたら、そのサブプロトコルを選び返す。
    #[tokio::test]
    async fn test_websocket_accepts_the_secret_as_a_subprotocol() {
        let app = create_router(create_shared_state());
        let ws = |protocol: String| {
            Request::builder()
                .uri("/api/collab")
                .header("connection", "upgrade")
                .header("upgrade", "websocket")
                .header("sec-websocket-version", "13")
                .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
                .header("sec-websocket-protocol", protocol)
                .body(axum::body::Body::empty())
                .unwrap()
        };
        // 知らない秘密は 403
        let res = app
            .clone()
            .oneshot(ws("yui.token.bad-token-0123456789".to_string()))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        // 正しい秘密は、認証を通る（`oneshot` では、アップグレード自体は完了しないので、426 まで進む）
        let res = app
            .oneshot(ws(format!("chat, yui.token.{TEST_HOST_TOKEN}")))
            .await
            .unwrap();
        assert_ne!(res.status(), StatusCode::FORBIDDEN);
        assert_ne!(res.status(), StatusCode::UNAUTHORIZED);
    }

    /// loopback 以外では、ターミナルとファイルの API は、有効にしない限り閉じている（秘密が正しくても）。
    #[tokio::test]
    async fn test_terminal_and_files_can_be_switched_off() {
        let dir = tempfile::tempdir().unwrap();
        let shared = Arc::new(AppStateShared {
            app_state: Mutex::new(AppState::default()),
            credentials: test_credentials(),
            tx: broadcast::channel(10).0,
            log_root: dir.path().to_path_buf(),
            workspace_root: dir.path().to_path_buf(),
            features: Features {
                terminal: false,
                fs: false,
            },
        });
        let app = create_router(shared);
        for uri in [
            "/api/terminal?as=host",
            "/api/fs/list?as=host",
            "/api/fs/read?path=a&as=host",
        ] {
            let res = app
                .clone()
                .oneshot(
                    auth_request(Request::builder(), uri)
                        .header("connection", "upgrade")
                        .header("upgrade", "websocket")
                        .header("sec-websocket-version", "13")
                        .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
                        .body(axum::body::Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::FORBIDDEN, "{uri}");
        }
    }

    /// 別のサイトのページ（`Origin` が違う）と、別の名前の `Host`（DNS リバインディング）は、`/api/` を使えない。
    /// ブラウザが付ける `Origin` がない要求（ネイティブのアプリ、コマンドラインのツール）と、
    /// 同一オリジンの要求は、通る。
    #[tokio::test]
    async fn test_api_refuses_other_origins_and_hosts() {
        let get = |app: Router,
                   uri: &'static str,
                   host: &'static str,
                   origin: Option<&'static str>| async move {
            let mut b = Request::builder()
                .uri(uri)
                .header("host", host)
                .header("authorization", format!("Bearer {TEST_HOST_TOKEN}"));
            if let Some(o) = origin {
                b = b.header("origin", o);
            }
            let res = app
                .oneshot(b.body(axum::body::Body::empty()).unwrap())
                .await
                .unwrap();
            (
                res.status(),
                res.headers().get("access-control-allow-origin").cloned(),
            )
        };
        let app = || create_router(create_shared_state());
        // Origin なし（ネイティブ）、同一オリジン
        let (st, acao) = get(app(), "/api/state", "127.0.0.1:8080", None).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(acao, None, "no CORS header without an allow-list");
        let (st, _) = get(
            app(),
            "/api/state",
            "127.0.0.1:8080",
            Some("http://127.0.0.1:8080"),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        // 別のサイトのページ
        let (st, acao) = get(
            app(),
            "/api/state",
            "127.0.0.1:8080",
            Some("https://evil.example"),
        )
        .await;
        assert_eq!(st, StatusCode::FORBIDDEN);
        assert_eq!(acao, None);
        // ターミナル（WebSocket の接続）も、別のサイトのページからは使えない（トークンの検査より前に断る）
        let (st, _) = get(
            app(),
            "/api/terminal?as=host&rows=24&cols=80",
            "127.0.0.1:8080",
            Some("https://evil.example"),
        )
        .await;
        assert_eq!(st, StatusCode::FORBIDDEN);
        // DNS リバインディング: 別の名前が 127.0.0.1 に解決されたとき
        let (st, _) = get(
            app(),
            "/api/state",
            "evil.example:8080",
            Some("http://evil.example:8080"),
        )
        .await;
        assert_eq!(st, StatusCode::FORBIDDEN);
        // 同じ Host でも、Origin を付けない（ブラウザ以外）なら、名前が合えば通る
        let (st, _) = get(app(), "/api/state", "localhost:8080", None).await;
        assert_eq!(st, StatusCode::OK);
    }

    /// 許可リストにある `Origin` は通り、CORS のヘッダーが付く。リストにないものは、まだ断る。
    #[tokio::test]
    async fn test_allow_list_origin_gets_cors_headers() {
        let policy = OriginPolicy::new("http://127.0.0.1:8085", "");
        let app = || create_router_with_web(create_shared_state(), None, policy.clone());
        let ask = |origin: &'static str| {
            Request::builder()
                .uri("/api/state")
                .header("host", "127.0.0.1:8080")
                .header("authorization", format!("Bearer {TEST_HOST_TOKEN}"))
                .header("origin", origin)
                .body(axum::body::Body::empty())
                .unwrap()
        };
        let res = app().oneshot(ask("http://127.0.0.1:8085")).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers().get("access-control-allow-origin").unwrap(),
            "http://127.0.0.1:8085"
        );
        let res = app().oneshot(ask("https://evil.example")).await.unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        assert!(res.headers().get("access-control-allow-origin").is_none());
    }

    #[test]
    fn test_web_root_setting() {
        assert_eq!(web_root_from(None), Ok(None));
        assert_eq!(web_root_from(Some("".into())), Ok(None));
        let dir = tempfile::tempdir().unwrap();
        let err = web_root_from(Some(dir.path().into())).unwrap_err();
        assert!(
            err.contains("no index.html") && err.contains("build-web.sh"),
            "{err}"
        );
        std::fs::write(dir.path().join("index.html"), "x").unwrap();
        assert_eq!(
            web_root_from(Some(dir.path().into())),
            Ok(Some(dir.path().to_path_buf()))
        );
    }

    #[tokio::test]
    async fn test_rest_state_and_structure_endpoints() {
        let shared = create_shared_state();
        let app = create_router(shared.clone());

        // 1. GET /api/state -> MessagePack
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .header("authorization", format!("Bearer {TEST_HOST_TOKEN}"))
                    .uri("/api/state")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let (_ver, _comp, state) = unpack_app_state(&body_bytes).expect("Failed to unpack state");
        assert_eq!(state.molecule.atoms.len(), 57960);

        // 2. POST /api/state -> Update with test peptide
        let mut new_state = AppState::default();
        let (peptide, _sec_map) = AtomGroup::create_test_peptide();
        new_state.molecule = peptide.to_flat_molecule();
        let packed_new = pack_app_state(&new_state, 0, Compression::None).unwrap();

        let post_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .header("authorization", format!("Bearer {TEST_HOST_TOKEN}"))
                    .method("POST")
                    .uri("/api/state")
                    .header("content-type", "application/msgpack")
                    .body(axum::body::Body::from(packed_new))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(post_response.status(), StatusCode::OK);

        // 3. GET /api/structure -> Verify updated
        let get_struct_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .header("authorization", format!("Bearer {TEST_HOST_TOKEN}"))
                    .uri("/api/structure")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_struct_res.status(), StatusCode::OK);
        let struct_bytes = get_struct_res
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes();
        let mol: Molecule = serde_json::from_slice(&struct_bytes).unwrap();
        assert_eq!(mol.atoms.len(), 9);
    }

    /// テストの URI にある `as=host|editor|viewer|bad`（どの秘密で呼ぶか）を取り除いて、
    /// `Authorization: Bearer` にする（`as` がなければ、秘密なし）
    fn auth_request(
        builder: axum::http::request::Builder,
        uri: &str,
    ) -> axum::http::request::Builder {
        let (path, query) = uri.split_once('?').unwrap_or((uri, ""));
        let mut who = None;
        let rest: Vec<&str> = query
            .split('&')
            .filter(|kv| match kv.strip_prefix("as=") {
                Some(w) => {
                    who = Some(w.to_string());
                    false
                }
                None => !kv.is_empty(),
            })
            .collect();
        let uri = if rest.is_empty() {
            path.to_string()
        } else {
            format!("{path}?{}", rest.join("&"))
        };
        let builder = builder.uri(uri);
        match who.as_deref() {
            Some("host") => builder.header("authorization", format!("Bearer {TEST_HOST_TOKEN}")),
            Some("editor") => {
                builder.header("authorization", format!("Bearer {TEST_EDITOR_TOKEN}"))
            }
            Some("viewer") => {
                builder.header("authorization", format!("Bearer {TEST_VIEWER_TOKEN}"))
            }
            Some(_) => builder.header("authorization", "Bearer bad-token-0123456789"),
            None => builder,
        }
    }

    #[test]
    fn test_collab_delta_verification_and_application() {
        let shared = create_shared_state();

        // 1. Host (role_token: 1) delta modifying representations -> Allowed
        let delta = AppStateDelta {
            camera: Some(CameraState {
                eye: [1.0, 2.0, 3.0],
                target: [0.0, 0.0, 0.0],
                up: [0.0, 1.0, 0.0],
                fov: 60.0,
            }),
            ..Default::default()
        };

        let host_role = shared
            .credentials
            .resolve(TEST_HOST_TOKEN)
            .expect("Host token missing");
        assert!(verify_role_action(host_role, &delta));

        {
            let mut app = shared.app_state.lock().unwrap();
            apply_app_state_delta(&mut app, &delta);
            assert_eq!(app.camera.fov, 60.0);
        }

        // 2. Viewer (role_token: 3) modifying annotations -> Rejected
        let viewer_delta = AppStateDelta {
            annotations: Some(vec![Annotation {
                id: "unauthorized".to_string(),
                text: "hack".to_string(),
                position: [0.0, 0.0, 0.0],
                color: [1.0, 0.0, 0.0, 1.0],
                visible: true,
                target_atom_id: None,
            }]),
            ..Default::default()
        };

        let viewer_role = shared
            .credentials
            .resolve(TEST_VIEWER_TOKEN)
            .expect("Viewer token missing");
        assert!(!verify_role_action(viewer_role, &viewer_delta));

        // 3. Invalid token (e.g. 999) -> None
        assert!(
            shared
                .credentials
                .resolve("no-such-token-0123456789")
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_logs_endpoint_streaming() {
        use crate::log_stream::LogStreamMessage;
        use std::io::Write;

        let temp_dir = tempfile::tempdir().unwrap();
        let log_file = temp_dir.path().join("server_test.log");

        {
            let mut f = std::fs::File::create(&log_file).unwrap();
            writeln!(f, "Log line A").unwrap();
        }

        let mut rx = spawn_log_stream(log_file.clone(), 10);

        // 1. Connected メッセージ
        let msg = rx.recv().await.unwrap();
        assert!(matches!(
            msg,
            LogStreamMessage::Connected {
                file_exists: true,
                ..
            }
        ));

        // 2. 初期行
        let msg_lines = rx.recv().await.unwrap();
        if let LogStreamMessage::Lines(lines) = msg_lines {
            assert_eq!(lines, vec!["Log line A"]);
        } else {
            panic!("Expected LogStreamMessage::Lines");
        }

        // 3. 追加行書き込み
        {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&log_file)
                .unwrap();
            writeln!(f, "Log line B").unwrap();
        }

        // 4. 追加行検知
        let msg_lines2 = rx.recv().await.unwrap();
        if let LogStreamMessage::Lines(lines) = msg_lines2 {
            assert_eq!(lines, vec!["Log line B"]);
        } else {
            panic!("Expected LogStreamMessage::Lines");
        }
    }

    #[test]
    fn test_resolve_secure_path() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();

        // 1. 正常な相対パス（未作成ファイル）
        let p1 = resolve_secure_path(root, "job.log").unwrap();
        assert_eq!(p1.file_name().unwrap(), "job.log");
        assert!(p1.starts_with(root.canonicalize().unwrap()));

        // 2. 正常なサブディレクトリ相対パス（実在ファイル）
        let sub_dir = root.join("subdir");
        std::fs::create_dir(&sub_dir).unwrap();
        let real_file = sub_dir.join("calc.log");
        std::fs::write(&real_file, "content").unwrap();

        let p2 = resolve_secure_path(root, "subdir/calc.log").unwrap();
        assert_eq!(p2, real_file.canonicalize().unwrap());

        // 3. パストラバーサル（'..'）の拒否
        assert!(resolve_secure_path(root, "../outside.log").is_err());
        assert!(resolve_secure_path(root, "subdir/../../outside.log").is_err());

        // 4. ルート絶対パスの拒否
        assert!(resolve_secure_path(root, "/etc/passwd").is_err());
    }

    #[tokio::test]
    async fn test_logs_endpoint_security_authorization_and_traversal() {
        let temp_dir = tempfile::tempdir().unwrap();
        let shared = Arc::new(AppStateShared {
            app_state: Mutex::new(AppState::default()),
            credentials: test_credentials(),
            tx: broadcast::channel(10).0,
            log_root: temp_dir.path().to_path_buf(),
            workspace_root: temp_dir.path().to_path_buf(),
            features: Features::all(),
        });
        let app = create_router(shared);

        let ws_request = |uri: &str| {
            auth_request(Request::builder(), uri)
                .header("connection", "upgrade")
                .header("upgrade", "websocket")
                .header("sec-websocket-version", "13")
                .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
                .body(axum::body::Body::empty())
                .unwrap()
        };

        // 1. トークンなし -> 401 Unauthorized
        let res_no_token = app
            .clone()
            .oneshot(ws_request("/api/logs?path=test.log"))
            .await
            .unwrap();
        assert_eq!(res_no_token.status(), StatusCode::UNAUTHORIZED);

        // 2. 無効なトークン（999） -> 403 Forbidden
        let res_invalid_token = app
            .clone()
            .oneshot(ws_request("/api/logs?path=test.log&as=bad"))
            .await
            .unwrap();
        assert_eq!(res_invalid_token.status(), StatusCode::FORBIDDEN);

        // 3. パストラバーサル（'..'） -> 403 Forbidden
        let res_traversal = app
            .clone()
            .oneshot(ws_request("/api/logs?path=../secret.log&as=host"))
            .await
            .unwrap();
        assert_eq!(res_traversal.status(), StatusCode::FORBIDDEN);

        // 4. 絶対パス -> 403 Forbidden
        let res_absolute = app
            .clone()
            .oneshot(ws_request("/api/logs?path=/etc/passwd&as=host"))
            .await
            .unwrap();
        assert_eq!(res_absolute.status(), StatusCode::FORBIDDEN);

        // 5. 正当なトークン（Host: 1, Editor: 2, Viewer: 3）かつ許可パス
        //    -> 認証・パス検証を完全に通過し、WebSocketUpgrade 抽出段階（HTTP oneshot のため UPGRADE_REQUIRED: 426）に到達する
        let res_valid = app
            .clone()
            .oneshot(ws_request("/api/logs?path=allowed.log&as=editor"))
            .await
            .unwrap();
        assert_eq!(res_valid.status(), StatusCode::UPGRADE_REQUIRED);
    }

    #[tokio::test]
    async fn test_terminal_endpoint_security_authorization() {
        let temp_dir = tempfile::tempdir().unwrap();
        let shared = Arc::new(AppStateShared {
            app_state: Mutex::new(AppState::default()),
            credentials: test_credentials(),
            tx: broadcast::channel(10).0,
            log_root: temp_dir.path().to_path_buf(),
            workspace_root: temp_dir.path().to_path_buf(),
            features: Features::all(),
        });
        let app = create_router(shared);

        let ws_request = |uri: &str| {
            auth_request(Request::builder(), uri)
                .header("connection", "upgrade")
                .header("upgrade", "websocket")
                .header("sec-websocket-version", "13")
                .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
                .body(axum::body::Body::empty())
                .unwrap()
        };

        // 1. トークンなし -> 401 Unauthorized
        let res_no_token = app
            .clone()
            .oneshot(ws_request("/api/terminal"))
            .await
            .unwrap();
        assert_eq!(res_no_token.status(), StatusCode::UNAUTHORIZED);

        // 2. 無効なトークン（999） -> 403 Forbidden
        let res_invalid_token = app
            .clone()
            .oneshot(ws_request("/api/terminal?as=bad"))
            .await
            .unwrap();
        assert_eq!(res_invalid_token.status(), StatusCode::FORBIDDEN);

        // 3. Viewer ロール（as=viewer） -> 403 Forbidden（ターミナルはHost専用）
        let res_viewer = app
            .clone()
            .oneshot(ws_request("/api/terminal?as=viewer"))
            .await
            .unwrap();
        assert_eq!(res_viewer.status(), StatusCode::FORBIDDEN);

        // 4. Editor ロール（as=editor） -> 403 Forbidden（ターミナルはHost専用）
        let res_editor = app
            .clone()
            .oneshot(ws_request("/api/terminal?as=editor"))
            .await
            .unwrap();
        assert_eq!(res_editor.status(), StatusCode::FORBIDDEN);

        // 5. Host ロール（as=host） -> 認証通過し WebSocketUpgrade 段階（UPGRADE_REQUIRED: 426）へ到達
        let res_host = app
            .clone()
            .oneshot(ws_request("/api/terminal?as=host&rows=24&cols=80"))
            .await
            .unwrap();
        assert_eq!(res_host.status(), StatusCode::UPGRADE_REQUIRED);
    }

    #[tokio::test]
    async fn test_terminal_pty_session() {
        let session = spawn_pty_session(24, 80).expect("Failed to spawn PTY session");
        let mut output_rx = session.output_rx;
        let input_tx = session.input_tx;

        // コマンドを送信（改行含む）
        input_tx
            .send(TerminalCommand::Input(
                b"echo YUI_TERMINAL_PTY_OK\n".to_vec(),
            ))
            .await
            .expect("Failed to send command to PTY");

        let mut collected_output = Vec::new();
        let timeout = tokio::time::sleep(tokio::time::Duration::from_secs(3));
        tokio::pin!(timeout);

        loop {
            tokio::select! {
                Some(chunk) = output_rx.recv() => {
                    collected_output.extend_from_slice(&chunk);
                    let text = String::from_utf8_lossy(&collected_output);
                    if text.contains("YUI_TERMINAL_PTY_OK") {
                        break;
                    }
                }
                _ = &mut timeout => {
                    let text = String::from_utf8_lossy(&collected_output);
                    panic!("Timeout waiting for PTY echo. Received so far: {:?}", text);
                }
            }
        }

        let output_text = String::from_utf8_lossy(&collected_output);
        assert!(output_text.contains("YUI_TERMINAL_PTY_OK"));
    }

    #[tokio::test]
    async fn test_fs_endpoints_security_authorization_and_traversal() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();

        // テスト用ファイルとサブディレクトリの作成
        let sub_dir = root.join("subdir");
        std::fs::create_dir(&sub_dir).unwrap();
        let file_a = root.join("alpha.txt");
        std::fs::write(&file_a, b"Alpha content").unwrap();
        let file_b = sub_dir.join("beta.md");
        std::fs::write(&file_b, b"Beta content").unwrap();

        let shared = Arc::new(AppStateShared {
            app_state: Mutex::new(AppState::default()),
            credentials: test_credentials(),
            tx: broadcast::channel(10).0,
            log_root: root.join("logs"),
            workspace_root: root.to_path_buf(),
            features: Features::all(),
        });
        let app = create_router(shared);

        let get_request = |uri: &str| {
            auth_request(Request::builder(), uri)
                .method("GET")
                .body(axum::body::Body::empty())
                .unwrap()
        };

        // 1. /api/fs/list 正常系（ルート直下一覧）
        // as=viewer (Viewer) も含め全ロール許可
        let res_list_root = app
            .clone()
            .oneshot(get_request("/api/fs/list?as=viewer"))
            .await
            .unwrap();
        assert_eq!(res_list_root.status(), StatusCode::OK);
        let body = res_list_root
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes();
        let entries: Vec<FsEntry> = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            entries,
            vec![
                FsEntry {
                    name: "alpha.txt".to_string(),
                    is_dir: false,
                },
                FsEntry {
                    name: "subdir".to_string(),
                    is_dir: true,
                },
            ]
        );

        // 2. /api/fs/list 正常系（サブディレクトリ一覧）
        let res_list_sub = app
            .clone()
            .oneshot(get_request("/api/fs/list?path=subdir&as=host"))
            .await
            .unwrap();
        assert_eq!(res_list_sub.status(), StatusCode::OK);
        let body_sub = res_list_sub.into_body().collect().await.unwrap().to_bytes();
        let entries_sub: Vec<FsEntry> = serde_json::from_slice(&body_sub).unwrap();
        assert_eq!(
            entries_sub,
            vec![FsEntry {
                name: "beta.md".to_string(),
                is_dir: false,
            }]
        );

        // 3. /api/fs/read 正常系（ファイル内容取得）
        let res_read = app
            .clone()
            .oneshot(get_request("/api/fs/read?path=alpha.txt&as=editor"))
            .await
            .unwrap();
        assert_eq!(res_read.status(), StatusCode::OK);
        let read_bytes = res_read.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(read_bytes.as_ref(), b"Alpha content");

        // 4. トークン省略 -> 401 Unauthorized
        let res_no_token = app
            .clone()
            .oneshot(get_request("/api/fs/list"))
            .await
            .unwrap();
        assert_eq!(res_no_token.status(), StatusCode::UNAUTHORIZED);

        let res_read_no_token = app
            .clone()
            .oneshot(get_request("/api/fs/read?path=alpha.txt"))
            .await
            .unwrap();
        assert_eq!(res_read_no_token.status(), StatusCode::UNAUTHORIZED);

        // 5. 無効なトークン（999） -> 403 Forbidden
        let res_invalid_token = app
            .clone()
            .oneshot(get_request("/api/fs/list?as=bad"))
            .await
            .unwrap();
        assert_eq!(res_invalid_token.status(), StatusCode::FORBIDDEN);

        let res_read_invalid_token = app
            .clone()
            .oneshot(get_request("/api/fs/read?path=alpha.txt&as=bad"))
            .await
            .unwrap();
        assert_eq!(res_read_invalid_token.status(), StatusCode::FORBIDDEN);

        // 6. パストラバーサル -> 403 Forbidden
        let res_traversal_list = app
            .clone()
            .oneshot(get_request("/api/fs/list?path=../outside&as=host"))
            .await
            .unwrap();
        assert_eq!(res_traversal_list.status(), StatusCode::FORBIDDEN);

        let res_traversal_read = app
            .clone()
            .oneshot(get_request("/api/fs/read?path=../outside.txt&as=host"))
            .await
            .unwrap();
        assert_eq!(res_traversal_read.status(), StatusCode::FORBIDDEN);

        // 7. 存在しないパス -> 404 Not Found
        let res_not_found_list = app
            .clone()
            .oneshot(get_request("/api/fs/list?path=nonexistent&as=host"))
            .await
            .unwrap();
        assert_eq!(res_not_found_list.status(), StatusCode::NOT_FOUND);

        let res_not_found_read = app
            .clone()
            .oneshot(get_request("/api/fs/read?path=nonexistent.txt&as=host"))
            .await
            .unwrap();
        assert_eq!(res_not_found_read.status(), StatusCode::NOT_FOUND);

        // 8. 400 Bad Request: ディレクトリを /api/fs/read で読もうとした場合
        let res_read_dir = app
            .clone()
            .oneshot(get_request("/api/fs/read?path=subdir&as=host"))
            .await
            .unwrap();
        assert_eq!(res_read_dir.status(), StatusCode::BAD_REQUEST);

        // 9. 400 Bad Request: ファイルを /api/fs/list で列挙しようとした場合
        let res_list_file = app
            .clone()
            .oneshot(get_request("/api/fs/list?path=alpha.txt&as=host"))
            .await
            .unwrap();
        assert_eq!(res_list_file.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_multi_frame_xyz_is_read_through_the_secured_fs_read_endpoint() {
        // frontend は、backend 上の軌跡ファイルを `/api/fs/read` で取得する（専用の API は足していない）。
        // パスの検証（ワークスペースの外は 403）と、権限の確認が、同じように効く。
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();
        let xyz = "2\nstep=0\nC 0 0 0\nO 1 0 0\n2\nstep=5\nC 0 1 0\nO 1 1 0\n";
        std::fs::write(root.join("run.xyz"), xyz).unwrap();
        std::fs::write(root.parent().unwrap().join("outside_run.xyz"), xyz).ok();

        let shared = Arc::new(AppStateShared {
            app_state: Mutex::new(AppState::default()),
            credentials: test_credentials(),
            tx: broadcast::channel(10).0,
            log_root: root.join("logs"),
            workspace_root: root.to_path_buf(),
            features: Features::all(),
        });
        let app = create_router(shared);
        let get = |uri: &str| {
            auth_request(Request::builder(), uri)
                .method("GET")
                .body(axum::body::Body::empty())
                .unwrap()
        };

        let ok = app
            .clone()
            .oneshot(get("/api/fs/read?path=run.xyz&as=viewer"))
            .await
            .unwrap();
        assert_eq!(ok.status(), StatusCode::OK);
        let body = axum::body::to_bytes(ok.into_body(), usize::MAX)
            .await
            .unwrap();
        let traj = yui_core::xyz_trajectory::XyzTrajectory::from_bytes("run.xyz", body.to_vec())
            .expect("the bytes are a trajectory");
        assert_eq!((traj.frame_count(), traj.num_atoms()), (2, 2));
        assert_eq!(traj.read_frame(1).unwrap().step, 5);

        // ワークスペースの外のファイルは拒否される
        let outside = app
            .clone()
            .oneshot(get("/api/fs/read?path=../outside_run.xyz&as=host"))
            .await
            .unwrap();
        assert_eq!(outside.status(), StatusCode::FORBIDDEN);
        let nested = app
            .clone()
            .oneshot(get("/api/fs/read?path=a/../../outside_run.xyz&as=host"))
            .await
            .unwrap();
        assert_eq!(nested.status(), StatusCode::FORBIDDEN);
        // 不明なトークン（ロールなし）は拒否される
        let nobody = app
            .oneshot(get("/api/fs/read?path=run.xyz&as=bad"))
            .await
            .unwrap();
        assert_eq!(nobody.status(), StatusCode::FORBIDDEN);
        let _ = std::fs::remove_file(root.parent().unwrap().join("outside_run.xyz"));
    }
}
