// SPDX-FileCopyrightText: The Yui development team
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::{Path, PathBuf};

use crate::menu_bar;
use yui_core::atom_group::AtomGroup;
use yui_core::state::AppState;
use yui_core::structure_export::{StructureExportFormat, export_structure_to_string};

/// 保存の動作モード（上書きか、名前を付けて保存か）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceSaveMode {
    /// 直前のパスへの直接上書き保存
    Overwrite,
    /// ファイル選択ダイアログによる保存
    SaveAs,
}

/// 保存モードを判定する純粋関数。
///
/// `is_save_as` が true の場合、または `current_path` が `None` の場合は `SaveAs`、
/// それ以外（ネイティブで直前のパスが存在する通常の Save）は `Overwrite` を返す。
pub fn determine_workspace_save_mode(
    is_save_as: bool,
    current_path: Option<&Path>,
) -> WorkspaceSaveMode {
    if is_save_as || current_path.is_none() {
        WorkspaceSaveMode::SaveAs
    } else {
        WorkspaceSaveMode::Overwrite
    }
}

/// 読み込まれたワークスペースの状態
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedWorkspace {
    pub state: AppState,
    pub pane_views: crate::pane_view::PaneLayout,
}

/// YAML文字列から新しいワークスペース状態を復元する純粋関数。
pub fn apply_loaded_workspace_yaml(yaml_content: &str) -> Result<LoadedWorkspace, String> {
    let (imported_state, imported_views) = menu_bar::import_workspace_from_yaml(yaml_content)
        .map_err(|e| format!("Failed to parse workspace YAML: {e}"))?;
    Ok(LoadedWorkspace {
        state: imported_state,
        pane_views: imported_views,
    })
}

/// ファイル選択ダイアログを開き、YAMLワークスペースを読み込む非同期関数。
/// ユーザーによるキャンセル時は `Ok(None)` を返す。
pub async fn open_workspace_file_dialog()
-> Result<Option<(String, Option<PathBuf>, String)>, String> {
    let dialog = rfd::AsyncFileDialog::new().add_filter("YAML Workspace", &["yaml", "yml"]);
    let Some(handle) = dialog.pick_file().await else {
        return Ok(None);
    };
    let file_name = handle.file_name();

    #[cfg(not(target_arch = "wasm32"))]
    let path = Some(handle.path().to_path_buf());
    #[cfg(target_arch = "wasm32")]
    let path = None;

    let bytes = handle.read().await;
    let content = String::from_utf8(bytes)
        .map_err(|_| format!("'{file_name}' is not a valid UTF-8 YAML file"))?;

    Ok(Some((file_name, path, content)))
}

/// 「名前を付けて保存」ダイアログを開き、ワークスペースを保存する非同期関数。
/// ユーザーによるキャンセル時は `Ok(None)` を返す。
pub async fn save_workspace_file_as(
    suggested_name: Option<String>,
    yaml_content: String,
) -> Result<Option<(String, Option<PathBuf>)>, String> {
    let default_name = suggested_name.unwrap_or_else(|| "workspace.yaml".to_string());
    let dialog = rfd::AsyncFileDialog::new()
        .add_filter("YAML Workspace", &["yaml", "yml"])
        .set_file_name(&default_name);

    let handle = match dialog.save_file().await {
        Some(h) => h,
        None => return Ok(None),
    };

    let file_name = handle.file_name();

    #[cfg(not(target_arch = "wasm32"))]
    let path = Some(handle.path().to_path_buf());
    #[cfg(target_arch = "wasm32")]
    let path = None;

    if let Err(e) = handle.write(yaml_content.as_bytes()).await {
        return Err(format!("Failed to write file: {e}"));
    }

    Ok(Some((file_name, path)))
}

/// ワークスペースを保存する非同期関数。
/// 直前のパスがあれば直接上書き保存し、無ければ保存ダイアログを開く。
#[allow(unused_variables)]
pub async fn save_workspace_file(
    current_path: Option<PathBuf>,
    suggested_name: Option<String>,
    yaml_content: String,
) -> Result<Option<(String, Option<PathBuf>)>, String> {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(path) = current_path {
        match tokio::fs::write(&path, yaml_content.as_bytes()).await {
            Ok(()) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "workspace.yaml".to_string());
                return Ok(Some((name, Some(path))));
            }
            Err(e) => return Err(format!("Failed to save file: {e}")),
        }
    }

    save_workspace_file_as(suggested_name, yaml_content).await
}

/// ファイルパスまたはファイル名から構造エクスポート形式を解決する純粋関数。
///
/// 対応拡張子:
/// - PDB: `.pdb`, `.ent`
/// - mmCIF: `.cif`, `.mmcif`
pub fn resolve_structure_export_format(
    path_or_filename: &str,
) -> Result<StructureExportFormat, String> {
    StructureExportFormat::from_extension(
        Path::new(path_or_filename)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or(""),
    )
    .ok_or_else(|| {
        format!(
            "Unsupported structure export format for '{path_or_filename}'. Supported extensions: .pdb, .cif, .mmcif, .ent"
        )
    })
}

/// 構造ファイル（PDB / mmCIF）を保存する非同期関数。
///
/// 保存ダイアログを開き、ユーザーが選択したファイル拡張子に基づいて
/// 形式（PDB / mmCIF）を自動決定して書き出す。
pub async fn export_structure_file(
    suggested_name: Option<String>,
    atom_group: AtomGroup,
) -> Result<Option<(String, Option<PathBuf>)>, String> {
    let default_name = suggested_name.unwrap_or_else(|| "structure.pdb".to_string());
    let dialog = rfd::AsyncFileDialog::new()
        .add_filter("PDB Structure (*.pdb, *.ent)", &["pdb", "ent"])
        .add_filter("PDBx/mmCIF Structure (*.cif, *.mmcif)", &["cif", "mmcif"])
        .set_file_name(&default_name);

    let handle = match dialog.save_file().await {
        Some(h) => h,
        None => return Ok(None),
    };

    let file_name = handle.file_name();
    let format = resolve_structure_export_format(&file_name)?;
    let content = export_structure_to_string(&atom_group, format)?;

    #[cfg(not(target_arch = "wasm32"))]
    let path = Some(handle.path().to_path_buf());
    #[cfg(target_arch = "wasm32")]
    let path = None;

    if let Err(e) = handle.write(content.as_bytes()).await {
        return Err(format!("Failed to write structure file: {e}"));
    }

    Ok(Some((file_name, path)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pane_view::default_pane_views;

    #[test]
    fn test_resolve_structure_export_format() {
        assert_eq!(
            resolve_structure_export_format("model.pdb").unwrap(),
            StructureExportFormat::Pdb
        );
        assert_eq!(
            resolve_structure_export_format("/path/to/protein.ent").unwrap(),
            StructureExportFormat::Pdb
        );
        assert_eq!(
            resolve_structure_export_format("complex.PDB").unwrap(),
            StructureExportFormat::Pdb
        );
        assert_eq!(
            resolve_structure_export_format("structure.cif").unwrap(),
            StructureExportFormat::Mmcif
        );
        assert_eq!(
            resolve_structure_export_format("/path/to/archive.mmcif").unwrap(),
            StructureExportFormat::Mmcif
        );
        assert_eq!(
            resolve_structure_export_format("test.CIF").unwrap(),
            StructureExportFormat::Mmcif
        );

        assert!(resolve_structure_export_format("unknown.xyz").is_err());
        assert!(resolve_structure_export_format("no_extension").is_err());
        assert!(resolve_structure_export_format("").is_err());
    }

    #[test]
    fn test_determine_workspace_save_mode() {
        let dummy_path = PathBuf::from("/path/to/workspace.yaml");

        // SaveAsフラグがtrueなら常にSaveAs
        assert_eq!(
            determine_workspace_save_mode(true, Some(&dummy_path)),
            WorkspaceSaveMode::SaveAs
        );
        assert_eq!(
            determine_workspace_save_mode(true, None),
            WorkspaceSaveMode::SaveAs
        );

        // SaveAsフラグがfalseでパスがあればOverwrite
        assert_eq!(
            determine_workspace_save_mode(false, Some(&dummy_path)),
            WorkspaceSaveMode::Overwrite
        );

        // SaveAsフラグがfalseでもパスが無ければSaveAsにフォールバック
        assert_eq!(
            determine_workspace_save_mode(false, None),
            WorkspaceSaveMode::SaveAs
        );
    }

    #[test]
    fn test_apply_loaded_workspace_yaml_valid() {
        let state = AppState::default();
        let views = default_pane_views();
        let yaml_str =
            menu_bar::export_workspace_to_yaml(&state, &views).expect("export should succeed");

        let loaded = apply_loaded_workspace_yaml(&yaml_str).expect("import should succeed");

        assert_eq!(loaded.state, state);
        assert_eq!(loaded.pane_views.len(), views.len());
    }

    #[test]
    fn test_apply_loaded_workspace_yaml_invalid() {
        let invalid_yaml = "not a valid workspace yaml: [unclosed";
        let result = apply_loaded_workspace_yaml(invalid_yaml);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .contains("Failed to parse workspace YAML")
        );
    }
}
