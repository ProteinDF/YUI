// SPDX-FileCopyrightText: The Yui development team
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOpenHandler {
    TextEditor,
    StructureViewer,
    WorkspaceStateImport,
    /// 複数フレームの XYZ を、表示中の構造の軌跡として開く
    Trajectory,
}

/// ファイルの右クリックメニュー（Open with）の対象: ファイルのパスと、開き方の候補
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenWithTarget {
    pub path: String,
    pub handlers: Vec<FileOpenHandler>,
}

impl FileOpenHandler {
    /// コマンドの引数に使う識別子
    pub fn id(&self) -> &'static str {
        match self {
            Self::TextEditor => "text_editor",
            Self::StructureViewer => "structure_viewer",
            Self::WorkspaceStateImport => "workspace_state",
            Self::Trajectory => "trajectory",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        [
            Self::TextEditor,
            Self::StructureViewer,
            Self::WorkspaceStateImport,
            Self::Trajectory,
        ]
        .into_iter()
        .find(|h| h.id() == id)
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::TextEditor => "Text Editor",
            Self::StructureViewer => "Structure Viewer",
            Self::WorkspaceStateImport => "Workspace State",
            Self::Trajectory => "Trajectory (XYZ)",
        }
    }
}

/// ファイル名の拡張子から候補ハンドラを判定する。**先頭の要素が既定（推奨）ハンドラ。**
/// 候補が2つ以上ある場合、呼び出し側はユーザーに選択させる。
pub fn candidate_handlers_for_extension(filename: &str) -> Vec<FileOpenHandler> {
    let ext = Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "pdb" | "ent" | "cif" | "mmcif" | "mol2" => {
            vec![
                FileOpenHandler::StructureViewer,
                FileOpenHandler::TextEditor,
            ]
        }
        "xyz" => vec![FileOpenHandler::Trajectory, FileOpenHandler::TextEditor],
        "yaml" | "yml" => vec![
            FileOpenHandler::WorkspaceStateImport,
            FileOpenHandler::TextEditor,
        ],
        _ => vec![FileOpenHandler::TextEditor],
    }
}

/// ツリーでファイルをクリックしたとき、開き方を毎回たずねるか。
/// たずねないファイルは、先頭の（既定の）ハンドラで、すぐに開く。右クリックの「Open with」では、
/// どのファイルでも、候補の一覧から選べる。
pub fn asks_on_click(filename: &str) -> bool {
    let ext = Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    matches!(ext.as_str(), "xyz" | "yaml" | "yml")
}

/// backend経由でファイル内容を取得する（RESTエンドポイント /api/fs/read を呼び出す）
pub async fn fetch_file_bytes_backend(url: String) -> Result<Vec<u8>, String> {
    let resp = crate::backend_auth::get(&url)
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP status error: {}", resp.status()));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("Failed to read response body: {e}"))?;
    Ok(bytes.to_vec())
}

/// ローカルファイルシステムから内容を取得する（ネイティブ限定）
#[cfg(not(target_arch = "wasm32"))]
pub async fn fetch_file_bytes_local(path: std::path::PathBuf) -> Result<Vec<u8>, String> {
    tokio::fs::read(&path)
        .await
        .map_err(|e| format!("Failed to read local file: {e}"))
}

/// デコードとディスパッチのアクション（I/Oを伴わない純粋な判定）
#[derive(Debug, PartialEq, Eq)]
pub enum DispatchContentAction {
    TextEditor {
        filename: String,
        content: Result<String, String>,
    },
    StructureViewer {
        filename: String,
        bytes: Vec<u8>,
    },
    WorkspaceStateImport {
        filename: String,
        content: Result<String, String>,
    },
    Trajectory {
        filename: String,
        bytes: Vec<u8>,
    },
}

/// 取得したバイト列とハンドラから、適用すべきディスパッチアクションを構築する純粋関数
pub fn determine_dispatch_action(
    path: &str,
    handler: FileOpenHandler,
    bytes: &[u8],
) -> DispatchContentAction {
    let filename = Path::new(path)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(path)
        .to_string();

    match handler {
        FileOpenHandler::TextEditor => {
            let content = std::str::from_utf8(bytes)
                .map(|s| s.to_string())
                .map_err(|e| format!("Invalid UTF-8 sequence: {e}"));
            DispatchContentAction::TextEditor { filename, content }
        }
        FileOpenHandler::StructureViewer => DispatchContentAction::StructureViewer {
            filename,
            bytes: bytes.to_vec(),
        },
        FileOpenHandler::Trajectory => DispatchContentAction::Trajectory {
            filename,
            bytes: bytes.to_vec(),
        },
        FileOpenHandler::WorkspaceStateImport => {
            let content = std::str::from_utf8(bytes)
                .map(|s| s.to_string())
                .map_err(|e| format!("Invalid UTF-8 sequence: {e}"));
            DispatchContentAction::WorkspaceStateImport { filename, content }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_candidate_handlers_structure() {
        assert_eq!(
            candidate_handlers_for_extension("protein.pdb"),
            vec![
                FileOpenHandler::StructureViewer,
                FileOpenHandler::TextEditor
            ]
        );
        assert_eq!(
            candidate_handlers_for_extension("PROTEIN.PDB"),
            vec![
                FileOpenHandler::StructureViewer,
                FileOpenHandler::TextEditor
            ]
        );
        assert_eq!(
            candidate_handlers_for_extension("structure.cif"),
            vec![
                FileOpenHandler::StructureViewer,
                FileOpenHandler::TextEditor
            ]
        );
        assert_eq!(
            candidate_handlers_for_extension("model.ent"),
            vec![
                FileOpenHandler::StructureViewer,
                FileOpenHandler::TextEditor
            ]
        );
        assert_eq!(
            candidate_handlers_for_extension("complex.mmcif"),
            vec![
                FileOpenHandler::StructureViewer,
                FileOpenHandler::TextEditor
            ]
        );
        assert_eq!(
            candidate_handlers_for_extension("ligand.mol2"),
            vec![
                FileOpenHandler::StructureViewer,
                FileOpenHandler::TextEditor
            ]
        );

        // 構造ファイルは、クリックではたずねずに 3D ビューで開く（テキストエディタは右クリックの「Open with」で選ぶ）
        assert!(!asks_on_click("protein.pdb"));
        assert!(!asks_on_click("notes.txt"));
        assert!(asks_on_click("run.xyz"));
        assert!(asks_on_click("state.YAML"));
    }

    #[test]
    fn test_candidate_handlers_yaml() {
        assert_eq!(
            candidate_handlers_for_extension("state.yaml"),
            vec![
                FileOpenHandler::WorkspaceStateImport,
                FileOpenHandler::TextEditor
            ]
        );
        assert_eq!(
            candidate_handlers_for_extension("config.YML"),
            vec![
                FileOpenHandler::WorkspaceStateImport,
                FileOpenHandler::TextEditor
            ]
        );
    }

    #[test]
    fn test_candidate_handlers_text_and_fallback() {
        assert_eq!(
            candidate_handlers_for_extension("README.md"),
            vec![FileOpenHandler::TextEditor]
        );
        assert_eq!(
            candidate_handlers_for_extension("notes.txt"),
            vec![FileOpenHandler::TextEditor]
        );
        assert_eq!(
            candidate_handlers_for_extension("Makefile"),
            vec![FileOpenHandler::TextEditor]
        );
        assert_eq!(
            candidate_handlers_for_extension(".gitignore"),
            vec![FileOpenHandler::TextEditor]
        );
    }

    #[test]
    fn test_determine_dispatch_action_text_editor() {
        let text_bytes = b"Hello, world!";
        let action =
            determine_dispatch_action("path/to/test.txt", FileOpenHandler::TextEditor, text_bytes);
        assert_eq!(
            action,
            DispatchContentAction::TextEditor {
                filename: "test.txt".to_string(),
                content: Ok("Hello, world!".to_string()),
            }
        );

        let invalid_utf8 = vec![0xFF, 0xFE, 0xFD];
        let err_action =
            determine_dispatch_action("bad.txt", FileOpenHandler::TextEditor, &invalid_utf8);
        match err_action {
            DispatchContentAction::TextEditor { filename, content } => {
                assert_eq!(filename, "bad.txt");
                assert!(content.is_err());
            }
            _ => panic!("Expected TextEditor variant"),
        }
    }

    #[test]
    fn test_determine_dispatch_action_structure_viewer() {
        let dummy_pdb = b"HEADER dummy";
        let action =
            determine_dispatch_action("1hls.pdb", FileOpenHandler::StructureViewer, dummy_pdb);
        assert_eq!(
            action,
            DispatchContentAction::StructureViewer {
                filename: "1hls.pdb".to_string(),
                bytes: dummy_pdb.to_vec(),
            }
        );
    }

    #[test]
    fn test_determine_dispatch_action_workspace_import() {
        let yaml_bytes = b"version: 2";
        let action = determine_dispatch_action(
            "state.yaml",
            FileOpenHandler::WorkspaceStateImport,
            yaml_bytes,
        );
        assert_eq!(
            action,
            DispatchContentAction::WorkspaceStateImport {
                filename: "state.yaml".to_string(),
                content: Ok("version: 2".to_string()),
            }
        );
    }

    #[test]
    fn test_xyz_opens_as_a_trajectory_by_default_or_as_text() {
        assert_eq!(
            candidate_handlers_for_extension("run.xyz"),
            vec![FileOpenHandler::Trajectory, FileOpenHandler::TextEditor]
        );
        assert_eq!(
            candidate_handlers_for_extension("RUN.XYZ"),
            vec![FileOpenHandler::Trajectory, FileOpenHandler::TextEditor]
        );
        assert_eq!(
            determine_dispatch_action("md/run.xyz", FileOpenHandler::Trajectory, b"3\n"),
            DispatchContentAction::Trajectory {
                filename: "run.xyz".to_string(),
                bytes: b"3\n".to_vec()
            }
        );
    }
}
