use yui_core::state::AppState;

/// コラボレーションURL（WebSocket または HTTP）から初期状態取得用の REST エンドポイント URL を導出する。
pub fn derive_http_state_url(collab_url: &str) -> String {
    let mut url_str = if let Some(stripped) = collab_url.strip_prefix("ws://") {
        format!("http://{stripped}")
    } else if let Some(stripped) = collab_url.strip_prefix("wss://") {
        format!("https://{stripped}")
    } else {
        collab_url.to_string()
    };

    if url_str.ends_with("/api/collab") {
        url_str.truncate(url_str.len() - "/api/collab".len());
        format!("{}/api/state", url_str.trim_end_matches('/'))
    } else if url_str.ends_with("/api/collab/") {
        url_str.truncate(url_str.len() - "/api/collab/".len());
        format!("{}/api/state", url_str.trim_end_matches('/'))
    } else {
        format!("{}/api/state", url_str.trim_end_matches('/'))
    }
}

/// コラボレーション URL（またはベース URL）から、`backend` の HTTP のベース URL（末尾の `/` と `/api/collab` なし）を導出する。
pub fn derive_http_base_url(base_url: &str) -> String {
    let mut base = if let Some(stripped) = base_url.strip_prefix("ws://") {
        format!("http://{stripped}")
    } else if let Some(stripped) = base_url.strip_prefix("wss://") {
        format!("https://{stripped}")
    } else {
        base_url.to_string()
    };
    for suffix in ["/api/collab/", "/api/collab"] {
        if let Some(stripped) = base.strip_suffix(suffix) {
            base = stripped.to_string();
            break;
        }
    }
    base.trim_end_matches('/').to_string()
}

/// コラボレーションURL（またはベースURL）と相対パスから、
/// ファイルシステム一覧取得用 HTTP REST URL（`/api/fs/list?path=...`）を導出する。
pub fn derive_http_fs_list_url(base_url: &str, path: &str) -> String {
    let mut http_base = if let Some(stripped) = base_url.strip_prefix("ws://") {
        format!("http://{stripped}")
    } else if let Some(stripped) = base_url.strip_prefix("wss://") {
        format!("https://{stripped}")
    } else {
        base_url.to_string()
    };

    if http_base.ends_with("/api/collab") {
        http_base.truncate(http_base.len() - "/api/collab".len());
    } else if http_base.ends_with("/api/collab/") {
        http_base.truncate(http_base.len() - "/api/collab/".len());
    }

    let http_base = http_base.trim_end_matches('/');
    let encoded_path = percent_encode(path);
    format!("{http_base}/api/fs/list?path={encoded_path}")
}

/// コラボレーションURL（またはベースURL）と相対パスから、
/// ファイルシステム読み込み用 HTTP REST URL（`/api/fs/read?path=...`）を導出する。
#[allow(dead_code)]
pub fn derive_http_fs_read_url(base_url: &str, path: &str) -> String {
    let mut http_base = if let Some(stripped) = base_url.strip_prefix("ws://") {
        format!("http://{stripped}")
    } else if let Some(stripped) = base_url.strip_prefix("wss://") {
        format!("https://{stripped}")
    } else {
        base_url.to_string()
    };

    if http_base.ends_with("/api/collab") {
        http_base.truncate(http_base.len() - "/api/collab".len());
    } else if http_base.ends_with("/api/collab/") {
        http_base.truncate(http_base.len() - "/api/collab/".len());
    }

    let http_base = http_base.trim_end_matches('/');
    let encoded_path = percent_encode(path);
    format!("{http_base}/api/fs/read?path={encoded_path}")
}

/// REST 経由で取得したリモートの初期 `AppState` をローカル状態にマージする。
///
/// 構造データ（`atom_group`/`molecule`）および表現・アノテーション・カメラ設定をローカルへ反映し、
/// 3Dビューポートの描画元となる分子平坦化データを最新状態に同期する。
pub fn merge_initial_state(local: &mut AppState, remote: AppState) {
    local.version = remote.version;
    local.camera = remote.camera;
    local.representations = remote.representations;
    local.annotations = remote.annotations;
    local.laser_pointer = remote.laser_pointer;
    local.data_paths = remote.data_paths;
    local.cube_view = remote.cube_view;
    local.trajectory = remote.trajectory;

    if let Some(group) = remote.atom_group {
        local.atom_group = Some(group);
        local.secondary_structure_map = remote.secondary_structure_map;
        local.interactions = remote.interactions;
        local.active_model = remote.active_model;
        local.sync_molecule_from_atom_group();
    } else if !remote.molecule.atoms.is_empty() {
        local.molecule = remote.molecule;
    }
}

/// パス文字列を URL クエリパラメータ用にパーセントエンコードする。
pub fn percent_encode(input: &str) -> String {
    let mut encoded = String::new();
    for byte in input.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                encoded.push(byte as char);
            }
            _ => {
                encoded.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    encoded
}

/// コラボレーションURL（またはベースURL）とログファイルパスから、
/// WebSocket ログストリーミング用 URL（`/api/logs?path=...`）を導出する。
pub fn derive_ws_logs_url(base_url: &str, log_path: &str) -> String {
    let mut ws_base = if let Some(stripped) = base_url.strip_prefix("http://") {
        format!("ws://{stripped}")
    } else if let Some(stripped) = base_url.strip_prefix("https://") {
        format!("wss://{stripped}")
    } else {
        base_url.to_string()
    };

    if ws_base.ends_with("/api/collab") {
        ws_base.truncate(ws_base.len() - "/api/collab".len());
    } else if ws_base.ends_with("/api/collab/") {
        ws_base.truncate(ws_base.len() - "/api/collab/".len());
    }

    let ws_base = ws_base.trim_end_matches('/');
    let encoded_path = percent_encode(log_path);
    format!("{ws_base}/api/logs?path={encoded_path}")
}

/// コラボレーションURL（またはベースURL）とサイズから、
/// WebSocket ターミナル用 URL（`/api/terminal?rows=...&cols=...`）を導出する。
pub fn derive_ws_terminal_url(base_url: &str, rows: u16, cols: u16) -> String {
    let mut ws_base = if let Some(stripped) = base_url.strip_prefix("http://") {
        format!("ws://{stripped}")
    } else if let Some(stripped) = base_url.strip_prefix("https://") {
        format!("wss://{stripped}")
    } else {
        base_url.to_string()
    };

    if ws_base.ends_with("/api/collab") {
        ws_base.truncate(ws_base.len() - "/api/collab".len());
    } else if ws_base.ends_with("/api/collab/") {
        ws_base.truncate(ws_base.len() - "/api/collab/".len());
    }

    let ws_base = ws_base.trim_end_matches('/');
    format!("{ws_base}/api/terminal?rows={rows}&cols={cols}")
}

/// URLエンコードされた文字列をパーセントデコードする。
#[allow(dead_code)]
pub fn percent_decode(input: &str) -> String {
    let mut bytes = Vec::new();
    let input_bytes = input.as_bytes();
    let mut i = 0;
    while i < input_bytes.len() {
        if input_bytes[i] == b'%'
            && i + 2 < input_bytes.len()
            && let Ok(val) = u8::from_str_radix(
                std::str::from_utf8(&input_bytes[i + 1..i + 3]).unwrap_or(""),
                16,
            )
        {
            bytes.push(val);
            i += 3;
            continue;
        }
        bytes.push(input_bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&bytes).to_string()
}

/// ブラウザの URL クエリパラメータ文字列（例: `?collab=ws://...` または `?backend=http://...`）と、
/// ページのホスト（`location.host`: ホスト名とポート）・https かどうかから、WASM クライアントが
/// 接続すべきベース WebSocket URL を導出する。
///
/// クエリパラメータがなければ、ページを配信したサーバー自身（同じオリジン）の `/api/collab` に接続する。
/// バックエンドが `YUI_WEB_ROOT` でブラウザ版を配信するとき、開発用の `trunk serve`（`frontend/Trunk.toml` の
/// プロキシ）のとき、どちらもこれで届く。ホストが分からないとき（`file://` など）だけ、`127.0.0.1:8080`。
#[allow(dead_code)]
pub fn resolve_wasm_collab_url(search: &str, host: &str, secure: bool) -> String {
    let trimmed_search = search.trim_start_matches('?');
    for pair in trimmed_search.split('&') {
        if let Some((key, val)) = pair.split_once('=')
            && (key == "collab" || key == "backend")
        {
            let decoded = percent_decode(val);
            let trimmed = decoded.trim();
            if !trimmed.is_empty() {
                let mut ws_url = if let Some(stripped) = trimmed.strip_prefix("http://") {
                    format!("ws://{stripped}")
                } else if let Some(stripped) = trimmed.strip_prefix("https://") {
                    format!("wss://{stripped}")
                } else if trimmed.starts_with("ws://") || trimmed.starts_with("wss://") {
                    trimmed.to_string()
                } else {
                    format!("ws://{trimmed}")
                };
                if !ws_url.ends_with("/api/collab") {
                    ws_url = format!("{}/api/collab", ws_url.trim_end_matches('/'));
                }
                return ws_url;
            }
        }
    }

    if host.is_empty() {
        return "ws://127.0.0.1:8080/api/collab".to_string();
    }
    let scheme = if secure { "wss" } else { "ws" };
    format!("{scheme}://{host}/api/collab")
}

#[cfg(test)]
mod tests {
    use super::*;
    use proteindf_bridge::{Atom, Position};
    use yui_core::AtomGroupExt;
    use yui_core::atom_group::AtomGroup;
    use yui_core::state::AppState;

    #[test]
    fn test_derive_http_state_url() {
        assert_eq!(
            derive_http_state_url("ws://127.0.0.1:8080/api/collab"),
            "http://127.0.0.1:8080/api/state"
        );
        assert_eq!(
            derive_http_state_url("ws://127.0.0.1:8080/api/collab/"),
            "http://127.0.0.1:8080/api/state"
        );
        assert_eq!(
            derive_http_state_url("wss://example.com/api/collab"),
            "https://example.com/api/state"
        );
        assert_eq!(
            derive_http_state_url("http://127.0.0.1:8080"),
            "http://127.0.0.1:8080/api/state"
        );
        assert_eq!(
            derive_http_state_url("ws://localhost:9000"),
            "http://localhost:9000/api/state"
        );
    }

    #[test]
    fn test_merge_initial_state_with_atom_group() {
        let mut local = AppState::default();
        let (initial_peptide, sec_map) = AtomGroup::create_test_peptide();
        local.atom_group = Some(initial_peptide);
        local.secondary_structure_map = sec_map;
        local.sync_molecule_from_atom_group();
        assert!(!local.molecule.atoms.is_empty());

        // リモート状態（異なる1原子のAtomGroup）を作成
        let mut remote = AppState::default();
        let mut remote_group = AtomGroup::with_name("remote_model");
        let mut model = AtomGroup::with_name("1");
        let mut chain = AtomGroup::with_name("A");
        let mut residue = AtomGroup::with_name("MET");
        let mut atom =
            Atom::new_with_pos("C", Position::new(10.0, 20.0, 30.0)).expect("valid atom");
        atom.name = "CA".to_string();
        residue.set_atom("CA", atom);
        chain.set_group("1", residue);
        model.set_group("A", chain);
        remote_group.set_group("1", model);
        remote_group.set_path("/".to_string());
        remote.atom_group = Some(remote_group);
        remote.secondary_structure_map.insert(
            "/1/A/1/".to_string(),
            yui_core::atom_group::SecondaryStructureKind::Helix,
        );

        merge_initial_state(&mut local, remote);

        assert_eq!(local.molecule.atoms.len(), 1);
        assert_eq!(local.molecule.atoms[0].element, "C");
        assert_eq!(local.molecule.atoms[0].position, [10.0, 20.0, 30.0]);

        // remote の secondary_structure_map がローカルへ反映され、
        // local の古い peptide 由来のエントリで上書きされていないこと
        assert_eq!(local.secondary_structure_map.len(), 1);
        assert_eq!(
            local.secondary_structure_map.get("/1/A/1/"),
            Some(&yui_core::atom_group::SecondaryStructureKind::Helix)
        );
    }

    #[test]
    fn test_merge_initial_state_copies_interactions_and_active_model() {
        let mut local = AppState::default();
        let (initial_peptide, sec_map) = AtomGroup::create_test_peptide();
        local.atom_group = Some(initial_peptide);
        local.secondary_structure_map = sec_map;
        local.sync_molecule_from_atom_group();
        local.active_model = None;
        local.interactions = proteindf_bridge::InteractionSet::new();

        let mut remote = AppState::default();
        let mut remote_group = AtomGroup::with_name("remote_model");
        let mut model = AtomGroup::with_name("1");
        let mut chain = AtomGroup::with_name("A");
        let mut residue = AtomGroup::with_name("MET");
        let mut atom =
            Atom::new_with_pos("C", Position::new(10.0, 20.0, 30.0)).expect("valid atom");
        atom.name = "CA".to_string();
        residue.set_atom("CA", atom);
        chain.set_group("1", residue);
        model.set_group("A", chain);
        remote_group.set_group("1", model);
        remote_group.set_path("/".to_string());
        remote.atom_group = Some(remote_group);
        remote.active_model = Some("model_1".to_string());
        let interaction = proteindf_bridge::Interaction {
            kind: proteindf_bridge::InteractionKind::HydrogenBond,
            atoms: vec!["/1/A/1/CA".to_string(), "/1/A/1/CA".to_string()],
            distance: Some(2.85),
            angle: Some(165.0),
            donor_acceptor_role: None,
        };
        remote.interactions =
            proteindf_bridge::InteractionSet::with_interactions(vec![interaction]);

        merge_initial_state(&mut local, remote);

        // atom_group, active_model, interactions の3つがすべて remote の値になること
        assert!(local.atom_group.is_some());
        assert_eq!(local.molecule.atoms.len(), 1);
        assert_eq!(local.active_model, Some("model_1".to_string()));
        assert_eq!(local.interactions.interactions.len(), 1);
        assert_eq!(
            local.interactions.interactions[0].kind,
            proteindf_bridge::InteractionKind::HydrogenBond
        );
        assert_eq!(local.interactions.interactions[0].distance, Some(2.85));
    }

    #[test]
    fn test_derive_ws_logs_url() {
        assert_eq!(
            derive_ws_logs_url("ws://127.0.0.1:8080/api/collab", "job.log"),
            "ws://127.0.0.1:8080/api/logs?path=job.log"
        );
        assert_eq!(
            derive_ws_logs_url("http://127.0.0.1:8080", "logs/run 1.log"),
            "ws://127.0.0.1:8080/api/logs?path=logs/run%201.log"
        );
        assert_eq!(
            derive_ws_logs_url("wss://cluster.internal/api/collab/", "test.log"),
            "wss://cluster.internal/api/logs?path=test.log"
        );
    }

    #[test]
    fn test_derive_http_base_url() {
        assert_eq!(
            derive_http_base_url("ws://127.0.0.1:8080/api/collab"),
            "http://127.0.0.1:8080"
        );
        assert_eq!(
            derive_http_base_url("wss://x.example/api/collab/"),
            "https://x.example"
        );
        assert_eq!(derive_http_base_url("http://h:1/"), "http://h:1");
        assert_eq!(
            derive_http_base_url("ws://127.0.0.1:8080"),
            "http://127.0.0.1:8080"
        );
    }

    #[test]
    fn test_urls_never_carry_the_secret() {
        // 秘密は、ヘッダー（ブラウザの WebSocket はサブプロトコル）で渡す。URL に入れない
        for url in [
            derive_ws_logs_url("ws://h:1", "a.log"),
            derive_ws_terminal_url("ws://h:1", 24, 80),
            derive_http_fs_list_url("ws://h:1", ""),
            derive_http_fs_read_url("ws://h:1", "a"),
        ] {
            assert!(!url.contains("token"), "{url}");
        }
    }

    #[test]
    fn test_derive_ws_terminal_url() {
        assert_eq!(
            derive_ws_terminal_url("ws://127.0.0.1:8080/api/collab", 24, 80),
            "ws://127.0.0.1:8080/api/terminal?rows=24&cols=80"
        );
        assert_eq!(
            derive_ws_terminal_url("http://127.0.0.1:8080", 30, 100),
            "ws://127.0.0.1:8080/api/terminal?rows=30&cols=100"
        );
        assert_eq!(
            derive_ws_terminal_url("wss://cluster.internal/api/collab/", 24, 80),
            "wss://cluster.internal/api/terminal?rows=24&cols=80"
        );
    }

    #[test]
    fn test_percent_decode() {
        assert_eq!(percent_decode("hello%20world"), "hello world");
        assert_eq!(
            percent_decode("ws%3A%2F%2F127.0.0.1%3A8080"),
            "ws://127.0.0.1:8080"
        );
        assert_eq!(percent_decode("simple_text"), "simple_text");
    }

    #[test]
    fn test_resolve_wasm_collab_url() {
        // 既定は、ページを配信したサーバー自身（同じオリジン）。ポートも含める
        assert_eq!(
            resolve_wasm_collab_url("", "127.0.0.1:8080", false),
            "ws://127.0.0.1:8080/api/collab"
        );
        assert_eq!(
            resolve_wasm_collab_url("", "localhost:8085", false),
            "ws://localhost:8085/api/collab",
            "trunk serve forwards /api to the backend (frontend/Trunk.toml)"
        );
        assert_eq!(
            resolve_wasm_collab_url("", "192.168.1.50:8080", false),
            "ws://192.168.1.50:8080/api/collab"
        );
        // https のページは、wss。標準のポートは、ホスト名だけ
        assert_eq!(
            resolve_wasm_collab_url("", "yui.example.com", true),
            "wss://yui.example.com/api/collab"
        );
        // ホストが分からない（file:// など）ときだけ、これまでの固定の宛先
        assert_eq!(
            resolve_wasm_collab_url("", "", false),
            "ws://127.0.0.1:8080/api/collab"
        );

        // クエリパラメータ ?collab=... の指定
        assert_eq!(
            resolve_wasm_collab_url("?collab=ws://10.0.0.1:9000", "localhost", false),
            "ws://10.0.0.1:9000/api/collab"
        );
        assert_eq!(
            resolve_wasm_collab_url(
                "?collab=http://10.0.0.1:9000/api/collab",
                "localhost",
                false
            ),
            "ws://10.0.0.1:9000/api/collab"
        );
        assert_eq!(
            resolve_wasm_collab_url("?collab=https://secure.example.com", "localhost", false),
            "wss://secure.example.com/api/collab"
        );
        // パーセントエンコードされたクエリパラメータ
        assert_eq!(
            resolve_wasm_collab_url("?collab=ws%3A%2F%2F127.0.0.1%3A8080", "localhost", false),
            "ws://127.0.0.1:8080/api/collab"
        );

        // クエリパラメータ ?backend=... の指定
        assert_eq!(
            resolve_wasm_collab_url("?backend=http://127.0.0.1:8080", "localhost", false),
            "ws://127.0.0.1:8080/api/collab"
        );
    }

    #[test]
    fn test_derive_http_fs_list_and_read_url() {
        // ws:// URL からの変換
        assert_eq!(
            derive_http_fs_list_url("ws://127.0.0.1:8080/api/collab", ""),
            "http://127.0.0.1:8080/api/fs/list?path="
        );
        assert_eq!(
            derive_http_fs_list_url("ws://127.0.0.1:8080", "subdir/test.txt"),
            "http://127.0.0.1:8080/api/fs/list?path=subdir/test.txt"
        );
        assert_eq!(
            derive_http_fs_list_url("http://127.0.0.1:8080", "path with spaces"),
            "http://127.0.0.1:8080/api/fs/list?path=path%20with%20spaces"
        );

        // wss:// URL からの変換
        assert_eq!(
            derive_http_fs_read_url("wss://secure.example.com/api/collab/", "data/sample.pdb"),
            "https://secure.example.com/api/fs/read?path=data/sample.pdb"
        );
        assert_eq!(
            derive_http_fs_read_url("https://example.com:9000", "test.cif"),
            "https://example.com:9000/api/fs/read?path=test.cif"
        );
    }
}
