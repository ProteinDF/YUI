use crate::state::AppState;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// ベースディレクトリを基準に、パスを絶対パスへ解決する。
/// パスがすでに絶対パスである場合はそのまま返す。
pub fn resolve_to_absolute(base_dir: &Path, path: &str) -> String {
    let p = Path::new(path);
    if p.is_absolute() {
        path.to_string()
    } else {
        base_dir.join(p).to_string_lossy().into_owned()
    }
}

/// ベースディレクトリを基準に、絶対パスを相対パスへ逆算する。
/// 逆算できない場合（別ドライブ等）や、パスが相対パスの場合はそのまま返す。
pub fn convert_to_relative(base_dir: &Path, absolute_path: &str) -> String {
    let path = Path::new(absolute_path);
    if path.is_relative() {
        return absolute_path.to_string();
    }

    // 共通の親ディレクトリを特定するため、まず両者をコンポーネントに分解する。
    // canonicalize はシンボリックリンクの解決などで不整合が起きる（片方だけ存在しない場合など）ため、
    // 単純な論理パスのコンポーネント比較を行う。
    let base_comps: Vec<_> = base_dir.components().collect();
    let path_comps: Vec<_> = path.components().collect();

    let mut common_len = 0;
    for (a, b) in base_comps.iter().zip(path_comps.iter()) {
        if a == b {
            common_len += 1;
        } else {
            break;
        }
    }

    if common_len == 0 {
        return absolute_path.to_string();
    }

    let mut rel_path = PathBuf::new();
    for _ in 0..(base_comps.len() - common_len) {
        rel_path.push("..");
    }
    for comp in &path_comps[common_len..] {
        rel_path.push(comp);
    }

    rel_path.to_string_lossy().into_owned()
}

/// YAML文字列から `version` フィールドを覗き見るヘルパー。
/// 厳密なAppStateデシリアライズ前にフォーマット互換性を検証するために使用する。
fn peek_yaml_state_format_version(yaml_str: &str) -> Result<u32, String> {
    let value: serde_yaml::Value =
        serde_yaml::from_str(yaml_str).map_err(|e| format!("Failed to parse YAML content: {e}"))?;
    let map = value
        .as_mapping()
        .ok_or_else(|| "Malformed AppState YAML: not a mapping".to_string())?;
    for (k, v) in map {
        if k.as_str() == Some("version") {
            return v.as_u64().map(|n| n as u32).ok_or_else(|| {
                "Malformed AppState YAML: version field is not a number".to_string()
            });
        }
    }
    Err("Malformed AppState YAML: missing version field".to_string())
}

fn validate_yaml_format_version(yaml_str: &str) -> Result<(), String> {
    let found_version = peek_yaml_state_format_version(yaml_str)?;
    if found_version != crate::state::ATOM_GROUP_FORMAT_VERSION {
        return Err(format!(
            "Unsupported or legacy atom_group format (state.version = {found_version}, expected {}). \
             This file was likely saved before Phase 6e (real proteindf-bridge AtomGroup integration) \
             and cannot be loaded. Please regenerate or re-export it with a current version of YUI.",
            crate::state::ATOM_GROUP_FORMAT_VERSION
        ));
    }
    Ok(())
}

/// YAMLファイルから AppState をインポートする。
/// YAML内の外部リソースパスは、YAMLファイルが存在するディレクトリからの相対パスとして解釈され、絶対パスに変換される。
pub fn import_app_state_from_yaml<P: AsRef<Path>>(yaml_path: P) -> Result<AppState, String> {
    let yaml_path = yaml_path.as_ref();
    let base_dir = yaml_path
        .parent()
        .ok_or_else(|| "Invalid YAML path: no parent directory".to_string())?;

    let mut file = File::open(yaml_path).map_err(|e| format!("Failed to open YAML file: {}", e))?;
    let mut content = String::new();
    file.read_to_string(&mut content)
        .map_err(|e| format!("Failed to read YAML file: {}", e))?;

    validate_yaml_format_version(&content)?;

    let mut state: AppState =
        serde_yaml::from_str(&content).map_err(|e| format!("Failed to parse YAML: {}", e))?;

    // パス解決
    if let Some(ref pdb) = state.data_paths.pdb_path {
        state.data_paths.pdb_path = Some(resolve_to_absolute(base_dir, pdb));
    }
    if let Some(ref cube) = state.data_paths.cube_path {
        state.data_paths.cube_path = Some(resolve_to_absolute(base_dir, cube));
    }

    Ok(state)
}

/// AppState を YAMLファイルにエクスポートする。
/// 外部リソースパスは、出力先YAMLファイルを基準とする相対パスに逆算される。
pub fn export_app_state_to_yaml<P: AsRef<Path>>(
    yaml_path: P,
    state: &AppState,
) -> Result<(), String> {
    let yaml_path = yaml_path.as_ref();
    let base_dir = yaml_path
        .parent()
        .ok_or_else(|| "Invalid YAML path: no parent directory".to_string())?;

    let mut exported_state = state.clone();

    // 相対パスへの逆算
    if let Some(ref pdb) = exported_state.data_paths.pdb_path {
        exported_state.data_paths.pdb_path = Some(convert_to_relative(base_dir, pdb));
    }
    if let Some(ref cube) = exported_state.data_paths.cube_path {
        exported_state.data_paths.cube_path = Some(convert_to_relative(base_dir, cube));
    }

    let yaml_content = serde_yaml::to_string(&exported_state)
        .map_err(|e| format!("Failed to serialize to YAML: {}", e))?;

    if let Some(parent) = yaml_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directories: {}", e))?;
    }

    let mut file =
        File::create(yaml_path).map_err(|e| format!("Failed to create YAML file: {}", e))?;
    file.write_all(yaml_content.as_bytes())
        .map_err(|e| format!("Failed to write YAML file: {}", e))?;

    Ok(())
}

/// YAML文字列から AppState をインポートする（メモリ内パース）
pub fn import_app_state_from_yaml_str(yaml_str: &str) -> Result<AppState, String> {
    validate_yaml_format_version(yaml_str)?;
    serde_yaml::from_str(yaml_str).map_err(|e| format!("Failed to parse YAML string: {e}"))
}

/// AppState を YAML文字列へエクスポートする（メモリ内シリアライズ）
pub fn export_app_state_to_yaml_str(state: &AppState) -> Result<String, String> {
    serde_yaml::to_string(state).map_err(|e| format!("Failed to serialize to YAML string: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::DataPaths;

    #[test]
    fn test_relative_absolute_resolution() {
        let base_dir = Path::new("/home/user/work/yui-project");
        let rel_path = "data/1aon.pdb";
        let resolved = resolve_to_absolute(base_dir, rel_path);

        assert_eq!(resolved, "/home/user/work/yui-project/data/1aon.pdb");

        let abs_path = "/home/user/work/yui-project/data/1aon.pdb";
        let relative = convert_to_relative(base_dir, abs_path);
        assert_eq!(relative, "data/1aon.pdb");

        let abs_path_up = "/home/user/work/other-project/data/model.pdb";
        let relative_up = convert_to_relative(base_dir, abs_path_up);
        assert_eq!(relative_up, "../other-project/data/model.pdb");
    }

    #[test]
    fn test_yaml_export_import() {
        let temp_dir = std::env::temp_dir().join("yui_test");
        std::fs::create_dir_all(&temp_dir).unwrap();

        let yaml_file_path = temp_dir.join("test_state.yaml");
        let mock_pdb_path = temp_dir.join("data").join("test.pdb");

        std::fs::create_dir_all(mock_pdb_path.parent().unwrap()).unwrap();
        std::fs::write(&mock_pdb_path, "").unwrap();

        use crate::atom_group::AtomGroupExt;
        let mut state = AppState::default();
        let (group, _sec_map) = crate::atom_group::AtomGroup::create_test_peptide();
        state.atom_group = Some(group);
        state.representations = vec![crate::representation::Representation::new(
            "rep1",
            crate::selector::Selector::PathWildcard("/A/6".to_string()),
            crate::representation::RepresentationKind::Cartoon,
            crate::representation::ColorScheme::Chain,
        )];
        state.data_paths = DataPaths {
            pdb_path: Some(mock_pdb_path.to_str().unwrap().to_string()),
            cube_path: None,
        };

        export_app_state_to_yaml(&yaml_file_path, &state).unwrap();

        let mut file = File::open(&yaml_file_path).unwrap();
        let mut yaml_str = String::new();
        file.read_to_string(&mut yaml_str).unwrap();
        assert!(yaml_str.contains("pdb_path: data/test.pdb"));
        assert!(yaml_str.contains("TestPeptide"));
        assert!(yaml_str.contains("rep1"));

        let imported_state = import_app_state_from_yaml(&yaml_file_path).unwrap();

        assert_eq!(
            imported_state.version,
            crate::state::ATOM_GROUP_FORMAT_VERSION
        );
        assert_eq!(imported_state.atom_group, state.atom_group);
        assert_eq!(imported_state.representations, state.representations);
        assert_eq!(
            imported_state.data_paths.pdb_path.unwrap(),
            mock_pdb_path.to_str().unwrap().to_string()
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_import_yaml_unsupported_version() {
        let legacy_yaml = r#"
version: 1
camera:
  eye: [0.0, 0.0, 50.0]
  target: [0.0, 0.0, 0.0]
  up: [0.0, 1.0, 0.0]
  fov_y: 45.0
  aspect_ratio: 1.0
scene:
  background_color: [0.0706, 0.0745, 0.0863, 1.0]
named_selections: {}
atom_group: null
secondary_structure_map: {}
molecule:
  atoms: []
  bonds: []
representations: []
annotations: []
data_paths:
  pdb_path: null
  cube_path: null
trajectory:
  frames: []
  current_frame: 0
  is_playing: false
laser_pointer:
  active: false
  position: [0.0, 0.0, 0.0]
"#;
        let result = import_app_state_from_yaml_str(legacy_yaml);
        assert!(result.is_err(), "importing legacy YAML version must fail");
        let err = result.err().unwrap();
        assert!(
            err.contains("Unsupported or legacy atom_group format"),
            "error message should indicate legacy format: {err}"
        );
        assert!(
            err.contains("state.version = 1"),
            "error message should mention found version: {err}"
        );
    }
}
