// SPDX-FileCopyrightText: The Yui development team
// SPDX-License-Identifier: GPL-3.0-or-later

use yui_core::atom_group::{AtomGroupExt, SecondaryStructureKind};
use yui_core::state::AppState;
use yui_core::structure_loader::{load_structure_into_state, parse_structure_bytes};

#[test]
fn test_load_real_pdb_1hls() {
    let pdb_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/1hls.pdb");
    let bytes = std::fs::read(pdb_path).expect("failed to read 1hls.pdb");

    // 1. parse_structure_bytes の単体動作検証
    let (group, sec_map, interactions) =
        parse_structure_bytes(&bytes, "1hls.pdb").expect("failed to parse 1hls.pdb");

    let atom_count = group.atom_count();
    println!("=== 1HLS.pdb atom_count: {atom_count} ===");
    assert_eq!(atom_count, 782, "1HLS should have 782 atoms");
    assert!(
        !interactions.is_empty(),
        "interactions should be detected for 1hls.pdb"
    );

    // 二次構造の検証と内訳集計
    assert!(
        !sec_map.is_empty(),
        "SecondaryStructureMap should not be empty"
    );
    let mut helix_count = 0;
    let mut sheet_count = 0;
    let mut loop_count = 0;
    for kind in sec_map.values() {
        match kind {
            SecondaryStructureKind::Helix => helix_count += 1,
            SecondaryStructureKind::Sheet => sheet_count += 1,
            SecondaryStructureKind::Loop => loop_count += 1,
        }
    }
    println!(
        "=== 1HLS.pdb SecondaryStructure breakdown: Helix={helix_count}, Sheet={sheet_count}, Loop={loop_count} (Total={}) ===",
        sec_map.len()
    );
    assert_eq!(helix_count, 18, "1HLS should have 18 helix residues");
    assert_eq!(sheet_count, 0, "1HLS has no beta sheet residues");
    assert_eq!(loop_count, 33, "1HLS should have 33 loop residues");

    // 2. load_structure_into_state による AppState 同期検証
    let mut state = AppState::default();
    load_structure_into_state(&mut state, &bytes, "1hls.pdb")
        .expect("failed to load structure into state");

    assert!(state.atom_group.is_some());
    assert_eq!(state.secondary_structure_map.len(), sec_map.len());
    assert_eq!(state.interactions.len(), interactions.len());
    assert_eq!(state.molecule.atoms.len(), 782);
    assert!(
        !state.molecule.bonds.is_empty(),
        "Molecule.bonds should be populated via setup()"
    );
}

#[test]
fn test_load_real_mmcif_1hls() {
    let cif_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/1HLS.cif");
    let bytes = std::fs::read(cif_path).expect("failed to read 1HLS.cif");

    let (group, sec_map, interactions) =
        parse_structure_bytes(&bytes, "1HLS.cif").expect("failed to parse 1HLS.cif");

    let atom_count = group.atom_count();
    println!("=== 1HLS.cif atom_count: {atom_count} ===");
    // 1HLS.cif contains all 20 NMR ensemble models (782 atoms * 20 = 15640 atoms)
    assert_eq!(
        atom_count, 15640,
        "1HLS mmCIF (all 20 models) should have 15640 atoms"
    );
    assert!(!sec_map.is_empty());
    assert!(!interactions.is_empty());

    let mut state = AppState::default();
    load_structure_into_state(&mut state, &bytes, "1HLS.cif")
        .expect("failed to load mmCIF into state");

    assert_eq!(state.interactions.len(), interactions.len());
    assert_eq!(state.molecule.atoms.len(), 15640);
    assert!(!state.molecule.bonds.is_empty());
}

#[test]
fn test_unsupported_file_extension() {
    let dummy_bytes = b"HEADER    DUMMY";
    let result = parse_structure_bytes(dummy_bytes, "model.xyz");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("Unsupported structure file format"),
        "error should mention unsupported format: {err}"
    );

    let mut state = AppState::default();
    let load_res = load_structure_into_state(&mut state, dummy_bytes, "model.xyz");
    assert!(load_res.is_err());
}

#[test]
fn test_invalid_utf8_handling() {
    let invalid_bytes = vec![0xFF, 0xFE, 0xFD];
    let result = parse_structure_bytes(&invalid_bytes, "broken.pdb");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("Invalid UTF-8 sequence"),
        "error should mention invalid UTF-8: {err}"
    );
}

#[test]
fn test_invalid_pdb_content_handling() {
    let invalid_content = b"THIS IS NOT A VALID PDB FILE AT ALL";
    // PDB parser may return empty AtomGroup or error; setup on empty AtomGroup
    let mut state = AppState::default();
    let _ = load_structure_into_state(&mut state, invalid_content, "empty.pdb");
    // Main requirement: must not panic!
}

#[test]
fn test_multi_model_active_model_and_ranges() {
    use yui_core::atom_group::{calculate_model_atom_ranges, iter_models};

    // 1. 1hls.pdb (1モデル) の場合: active_model は None
    let pdb_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/1hls.pdb");
    let pdb_bytes = std::fs::read(pdb_path).expect("failed to read 1hls.pdb");
    let mut pdb_state = AppState::default();
    load_structure_into_state(&mut pdb_state, &pdb_bytes, "1hls.pdb").expect("load 1hls.pdb");
    assert_eq!(
        pdb_state.active_model, None,
        "Single-model structure (1hls.pdb) must have active_model == None"
    );

    // 2. 1HLS.cif (20モデル) の場合: active_model は最初のモデル
    let cif_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/1HLS.cif");
    let cif_bytes = std::fs::read(cif_path).expect("failed to read 1HLS.cif");
    let mut cif_state = AppState::default();
    load_structure_into_state(&mut cif_state, &cif_bytes, "1HLS.cif").expect("load 1HLS.cif");

    let group = cif_state
        .atom_group
        .as_ref()
        .expect("atom_group should be populated");
    let first_model_key = iter_models(group).next().map(|(k, _)| k.to_string());
    assert!(first_model_key.is_some());
    assert_eq!(
        cif_state.active_model, first_model_key,
        "Multi-model structure (1HLS.cif) active_model must match the first model"
    );

    // 3. モデルごとの原子 ID 範囲の検証
    let ranges = calculate_model_atom_ranges(group);
    assert_eq!(
        ranges.len(),
        20,
        "1HLS.cif must have exactly 20 model ranges"
    );

    let mut expected_start = 1u32;
    for (idx, range) in ranges.iter().enumerate() {
        assert_eq!(
            range.start_id, expected_start,
            "Model {idx} range must start at {expected_start}, got {}",
            range.start_id
        );
        assert_eq!(
            range.atom_count(),
            782,
            "Model {idx} ({}) must have exactly 782 atoms",
            range.model_key
        );
        assert_eq!(
            range.end_id,
            expected_start + 782 - 1,
            "Model {idx} end_id mismatch"
        );
        expected_start = range.end_id + 1;
    }
    assert_eq!(
        expected_start, 15641,
        "Total atoms traversed across 20 models must be 15,640"
    );
}

#[test]
fn test_load_sample_mol2() {
    let mol2_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/sample.mol2");
    let bytes = std::fs::read(mol2_path).expect("failed to read sample.mol2");

    // 1. parse_structure_bytes の検証
    let (group, sec_map, _interactions) =
        parse_structure_bytes(&bytes, "sample.mol2").expect("failed to parse sample.mol2");

    // iter_flat_atoms で9原子が見つかる
    let flat_atoms: Vec<_> = yui_core::atom_group::iter_flat_atoms(&group).collect();
    assert_eq!(
        flat_atoms.len(),
        9,
        "iter_flat_atoms should yield exactly 9 atoms for sample.mol2"
    );

    // 結合が BOND 節と同じ8本になる
    let mut bridge_inner = group.into_inner();
    let bond_records = bridge_inner.get_bond_list();
    assert_eq!(
        bond_records.len(),
        8,
        "sample.mol2 should have exactly 8 bonds as defined in @<TRIPOS>BOND"
    );

    // 二次構造は計算されず空であること
    assert!(
        sec_map.is_empty(),
        "Secondary structure map should be empty for mol2"
    );

    // 2. load_structure_into_state による AppState 同期検証
    let mut state = AppState::default();
    load_structure_into_state(&mut state, &bytes, "sample.mol2")
        .expect("failed to load mol2 into state");

    // AppState.molecule に9原子、8結合が入る
    assert_eq!(
        state.molecule.atoms.len(),
        9,
        "AppState.molecule should contain 9 atoms"
    );
    assert_eq!(
        state.molecule.bonds.len(),
        8,
        "AppState.molecule should contain 8 bonds"
    );
    assert_eq!(
        state.active_model, None,
        "Single model should have active_model == None"
    );
}

#[test]
fn test_load_adds_default_representations_only_when_empty() {
    use yui_core::representation::{
        ColorScheme, Representation, RepresentationKind, default_representations,
    };
    use yui_core::selector::Selector;

    let bytes = std::fs::read("tests/data/1hls.pdb").expect("read 1hls.pdb");

    // 表示設定が空の状態で読み込むと、既定の2つが追加される
    let mut state = AppState::default();
    assert!(state.representations.is_empty());
    load_structure_into_state(&mut state, &bytes, "1hls.pdb").expect("load 1hls.pdb");
    assert_eq!(state.representations, default_representations());

    // 表示設定がある状態で読み込んでも、追加・変更しない
    let user = Representation::new(
        "user_rep",
        Selector::All,
        RepresentationKind::Cpk,
        ColorScheme::Chain,
    );
    let mut state = AppState::default();
    state.representations.push(user.clone());
    load_structure_into_state(&mut state, &bytes, "1hls.pdb").expect("load 1hls.pdb");
    assert_eq!(state.representations, vec![user]);

    // 読み込みに失敗したときは、表示設定を追加しない
    let mut state = AppState::default();
    assert!(load_structure_into_state(&mut state, b"dummy", "model.xyz").is_err());
    assert!(state.representations.is_empty());
}

#[test]
fn test_cube_atoms_are_loaded_as_a_structure() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/sphere.cube"
    ))
    .unwrap();
    let cube = yui_core::cube_iso::CubeSet::parse(&text).unwrap();
    assert_eq!(cube.num_atoms(), 1);
    let mut state = AppState::default();
    assert!(yui_core::structure_loader::load_cube_atoms_into_state(&mut state, &cube).unwrap());
    assert_eq!(state.molecule.atoms.len(), 1);
    assert_eq!(state.molecule.atoms[0].element, "C");
    assert!(state.atom_group.is_some());
    assert!(!state.representations.is_empty());
}
