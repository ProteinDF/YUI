// SPDX-FileCopyrightText: The Yui development team
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::Path;

use crate::atom_group::{AtomGroup, SecondaryStructureKind, SecondaryStructureMap};
use crate::state::AppState;
use proteindf_bridge::InteractionSet;
use proteindf_bridge::secondary_structure::{SsCode, calc_secondary_structure};

/// ファイルのバイト列とファイル名（拡張子判定用）から実構造ファイルをパースし、
/// `AppState.atom_group`/`secondary_structure_map`/`interactions`へ設定した上で
/// `sync_molecule_from_atom_group()`を呼ぶ。
///
/// 読み込み後に表示設定（`representations`）が1つもなければ、既定の表示設定
/// （`representation::default_representations()`）を追加する。表示設定が既にあれば変更しない。
/// 読み込みに失敗した場合は、状態を変更しない。
///
/// 対応フォーマットはファイル名の拡張子で判定する（`.pdb`/`.ent` → PDB形式、`.cif`/`.mmcif` → mmCIF形式、`.mol2` → Mol2形式）。
pub fn load_structure_into_state(
    state: &mut AppState,
    bytes: &[u8],
    filename_hint: &str,
) -> Result<(), String> {
    let (atom_group, sec_map, interactions) = parse_structure_bytes(bytes, filename_hint)?;
    let models: Vec<&str> = crate::atom_group::iter_models(&atom_group)
        .map(|(k, _)| k)
        .collect();
    state.active_model = if models.len() > 1 {
        models.first().map(|k| k.to_string())
    } else {
        None
    };
    state.atom_group = Some(atom_group);
    state.secondary_structure_map = sec_map;
    state.interactions = interactions;
    state.sync_molecule_from_atom_group();
    // 表示設定が1つもなければ、既定の表示設定（カートゥーン + 球棒）を追加する
    crate::representation::ensure_default_representations(&mut state.representations);
    Ok(())
}

/// CUBE ファイルの原子を、構造として読み込む（構造を開いていないときに、CUBE の原子を見るため）。
///
/// 原子は一つの残基に並べ、結合は距離から求める。二次構造はない。CUBE に原子がなければ `Ok(false)` で、
/// 状態を変更しない。読み込みに失敗した場合も変更しない。
pub fn load_cube_atoms_into_state(
    state: &mut AppState,
    cube: &crate::cube_iso::CubeSet,
) -> Result<bool, String> {
    let Some(raw) = cube.atom_group()? else {
        return Ok(false);
    };
    let mut group = normalize_mol2_atomgroup(&raw)?;
    group
        .setup()
        .map_err(|e| format!("Failed to setup the topology of the CUBE atoms: {e}"))?;
    let mut interactions = InteractionSet::new();
    for (_, model) in group.groups() {
        for item in InteractionSet::detect_all(model, None, None).interactions {
            interactions.push(item);
        }
    }
    state.active_model = None;
    state.atom_group = Some(group.into());
    state.secondary_structure_map = SecondaryStructureMap::new();
    state.interactions = interactions;
    state.sync_molecule_from_atom_group();
    crate::representation::ensure_default_representations(&mut state.representations);
    Ok(true)
}

/// MOL2 形式の平坦な `AtomGroup`（分子直下に原子と結合が並ぶ構造）を、
/// YUI の標準 4 階層構造（model_1 -> 鎖 A -> 残基 1 -> 原子）へ正規化する。
///
/// 残基名は分子名（先頭3文字の大文字、または空なら "UNL"）とする。
/// 結合（BOND 節）は残基グループ内に保持され、原子キーに対する相対パスとして解決される。
fn normalize_mol2_atomgroup(
    raw_group: &proteindf_bridge::AtomGroup,
) -> Result<proteindf_bridge::AtomGroup, String> {
    let res_name = if raw_group.name.trim().is_empty() {
        "UNL".to_string()
    } else {
        let trimmed = raw_group.name.trim();
        let upper: String = trimmed
            .chars()
            .take(3)
            .flat_map(|c| c.to_uppercase())
            .collect();
        if upper.is_empty() {
            "UNL".to_string()
        } else {
            upper
        }
    };

    let mut res = proteindf_bridge::AtomGroup::with_name(&res_name);
    for (atom_key, atom) in raw_group.atoms() {
        res.set_atom(atom_key, atom.clone());
    }
    res.set_bonds(raw_group.bonds().to_vec());

    let mut chain_a = proteindf_bridge::AtomGroup::with_name("A");
    chain_a.set_group("1", res);

    let mut model_1 = proteindf_bridge::AtomGroup::with_name("model_1");
    model_1.set_group("A", chain_a);

    let mut root = proteindf_bridge::AtomGroup::with_name(&raw_group.name);
    root.set_group("model_1", model_1);
    root.set_path("/".to_string());

    Ok(root)
}

/// バイト列をパースして `(AtomGroup, SecondaryStructureMap, InteractionSet)` を返す（`AppState`には触れない、
/// テストしやすくするための分離）。
pub fn parse_structure_bytes(
    bytes: &[u8],
    filename_hint: &str,
) -> Result<(AtomGroup, SecondaryStructureMap, InteractionSet), String> {
    let content = std::str::from_utf8(bytes)
        .map_err(|e| format!("Invalid UTF-8 sequence in structure file '{filename_hint}': {e}"))?;

    let ext = Path::new(filename_hint)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();

    let is_mol2 = ext == "mol2";
    let (mut bridge_group, has_explicit_bonds) = match ext.as_str() {
        "pdb" | "ent" => {
            let mut pdb = proteindf_bridge::Pdb::new(None);
            pdb.parse_str(content).map_err(|e| {
                format!("Failed to parse PDB structure from '{filename_hint}': {e}")
            })?;
            let group = pdb.get_atomgroup(None, None).map_err(|e| {
                format!("Failed to extract AtomGroup from PDB '{filename_hint}': {e}")
            })?;
            (group, false)
        }
        "cif" | "mmcif" => {
            let mut cif = proteindf_bridge::format::SimpleMmcif::new();
            cif.load_from_str(content).map_err(|e| {
                format!("Failed to parse mmCIF structure from '{filename_hint}': {e}")
            })?;
            let group = cif.get_structure_atomgroup(None, None).map_err(|e| {
                format!("Failed to extract AtomGroup from mmCIF '{filename_hint}': {e}")
            })?;
            (group, false)
        }
        "mol2" => {
            let mut mol2 = proteindf_bridge::format::SimpleMol2::new();
            mol2.parse_str(content).map_err(|e| {
                format!("Failed to parse Mol2 structure from '{filename_hint}': {e}")
            })?;
            let raw_group = mol2.get_atomgroup();
            let has_bonds = raw_group.get_number_of_bonds() > 0;
            let normalized = normalize_mol2_atomgroup(raw_group)?;
            (normalized, has_bonds)
        }
        _ => {
            return Err(format!(
                "Unsupported structure file format for '{filename_hint}'. Supported formats: .pdb, .ent, .cif, .mmcif, .mol2"
            ));
        }
    };

    if !has_explicit_bonds {
        bridge_group.setup().map_err(|e| {
            format!("Failed to setup structure topology for '{filename_hint}': {e}")
        })?;
    }

    let mut sec_map = SecondaryStructureMap::new();
    let mut interactions = InteractionSet::new();
    for (model_key, model) in bridge_group.groups() {
        for (chain_key, chain) in model.groups() {
            if !is_mol2 {
                for ss in calc_secondary_structure(chain) {
                    let kind = match ss.code {
                        SsCode::Helix => SecondaryStructureKind::Helix,
                        SsCode::Strand => SecondaryStructureKind::Sheet,
                        SsCode::Loop => SecondaryStructureKind::Loop,
                    };
                    let path = format!("/{model_key}/{chain_key}/{}/", ss.residue_key);
                    sec_map.insert(path, kind);
                }
            }
        }

        let model_interactions = InteractionSet::detect_all(model, None, None);
        for item in model_interactions.interactions {
            interactions.push(item);
        }
    }

    Ok((bridge_group.into(), sec_map, interactions))
}
