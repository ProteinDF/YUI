// SPDX-FileCopyrightText: The Yui development team
// SPDX-License-Identifier: GPL-3.0-or-later

use yui_core::atom_group::{AtomGroupExt, iter_chains, iter_models, iter_residues};
use yui_core::structure_export::{StructureExportFormat, export_structure_to_string};
use yui_core::structure_loader::parse_structure_bytes;

#[test]
fn test_export_1hls_pdb_round_trip() {
    let pdb_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/1hls.pdb");
    let bytes = std::fs::read(pdb_path).expect("failed to read 1hls.pdb");
    let (group, _sec_map, _interactions) =
        parse_structure_bytes(&bytes, "1hls.pdb").expect("failed to parse 1hls.pdb");

    assert_eq!(group.atom_count(), 782);
    let original_chains: usize = iter_models(&group)
        .map(|(_, m)| iter_chains(m).count())
        .sum();
    let original_residues: usize = iter_models(&group)
        .flat_map(|(_, m)| iter_chains(m))
        .map(|(_, c)| iter_residues(c).count())
        .sum();

    // PDB で書き出し
    let exported_pdb = export_structure_to_string(&group, StructureExportFormat::Pdb)
        .expect("export to PDB should succeed");

    // 再度読み込み
    let (reloaded_group, _reloaded_sec, _reloaded_inter) =
        parse_structure_bytes(exported_pdb.as_bytes(), "exported.pdb")
            .expect("reloading exported PDB should succeed");

    assert_eq!(
        reloaded_group.atom_count(),
        782,
        "atom count should match after PDB round trip"
    );
    let reloaded_chains: usize = iter_models(&reloaded_group)
        .map(|(_, m)| iter_chains(m).count())
        .sum();
    let reloaded_residues: usize = iter_models(&reloaded_group)
        .flat_map(|(_, m)| iter_chains(m))
        .map(|(_, c)| iter_residues(c).count())
        .sum();

    assert_eq!(
        reloaded_chains, original_chains,
        "chain count should match after PDB round trip"
    );
    assert_eq!(
        reloaded_residues, original_residues,
        "residue count should match after PDB round trip"
    );
}

#[test]
fn test_export_1hls_mmcif_round_trip() {
    let cif_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/1HLS.cif");
    let bytes = std::fs::read(cif_path).expect("failed to read 1HLS.cif");
    let (group, _sec_map, _interactions) =
        parse_structure_bytes(&bytes, "1HLS.cif").expect("failed to parse 1HLS.cif");

    assert_eq!(group.atom_count(), 15640);
    assert_eq!(iter_models(&group).count(), 20);

    // mmCIF で書き出し
    let exported_cif = export_structure_to_string(&group, StructureExportFormat::Mmcif)
        .expect("export to mmCIF should succeed");

    // 再度読み込み
    let (reloaded_group, _reloaded_sec, _reloaded_inter) =
        parse_structure_bytes(exported_cif.as_bytes(), "exported.cif")
            .expect("reloading exported mmCIF should succeed");

    assert_eq!(
        iter_models(&reloaded_group).count(),
        20,
        "model count should match 20 after mmCIF round trip"
    );
    assert_eq!(
        reloaded_group.atom_count(),
        15640,
        "atom count should match 15640 after mmCIF round trip"
    );
}

#[test]
fn test_cross_export_formats() {
    // 1hls.pdb (1モデル, 782原子) -> mmCIF -> 再読み込み
    let pdb_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/1hls.pdb");
    let pdb_bytes = std::fs::read(pdb_path).expect("failed to read 1hls.pdb");
    let (pdb_group, _, _) =
        parse_structure_bytes(&pdb_bytes, "1hls.pdb").expect("failed to parse 1hls.pdb");

    let exported_cif = export_structure_to_string(&pdb_group, StructureExportFormat::Mmcif)
        .expect("export pdb to mmcif should succeed");
    let (reloaded_cif, _, _) = parse_structure_bytes(exported_cif.as_bytes(), "from_pdb.cif")
        .expect("reloading cif from pdb should succeed");
    assert_eq!(
        reloaded_cif.atom_count(),
        782,
        "atom count should match when exporting 1hls.pdb to mmcif"
    );
    assert_eq!(
        iter_models(&reloaded_cif).count(),
        1,
        "model count should match when exporting 1hls.pdb to mmcif"
    );

    // 1HLS.cif (20モデル, 15640原子) -> PDB -> 再読み込み
    let cif_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/1HLS.cif");
    let cif_bytes = std::fs::read(cif_path).expect("failed to read 1HLS.cif");
    let (cif_group, _, _) =
        parse_structure_bytes(&cif_bytes, "1HLS.cif").expect("failed to parse 1HLS.cif");

    let exported_pdb = export_structure_to_string(&cif_group, StructureExportFormat::Pdb)
        .expect("export mmcif to pdb should succeed");
    let (reloaded_pdb, _, _) = parse_structure_bytes(exported_pdb.as_bytes(), "from_cif.pdb")
        .expect("reloading pdb from cif should succeed");
    assert_eq!(
        reloaded_pdb.atom_count(),
        15640,
        "atom count should match when exporting 1HLS.cif to pdb"
    );
    assert_eq!(
        iter_models(&reloaded_pdb).count(),
        20,
        "model count should be 20 when exporting 1HLS.cif to pdb"
    );
}

#[test]
fn test_sample_mol2_export_pdb_and_mmcif() {
    let mol2_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/sample.mol2");
    let bytes = std::fs::read(mol2_path).expect("failed to read sample.mol2");
    let (group, _, _) =
        parse_structure_bytes(&bytes, "sample.mol2").expect("failed to parse sample.mol2");

    assert_eq!(group.atom_count(), 9);

    // PDB では書き出せる
    let exported_pdb = export_structure_to_string(&group, StructureExportFormat::Pdb)
        .expect("export mol2 to PDB should succeed");
    let (reloaded_pdb, _, _) =
        parse_structure_bytes(exported_pdb.as_bytes(), "sample_from_mol2.pdb")
            .expect("reloading pdb from mol2 should succeed");
    assert_eq!(reloaded_pdb.atom_count(), 9);

    // mmCIF で書き出すと電荷についての分かりやすいエラーになる
    let mmcif_result = export_structure_to_string(&group, StructureExportFormat::Mmcif);
    assert!(
        mmcif_result.is_err(),
        "mmCIF export should fail for sample.mol2 due to partial charges"
    );
    let err = mmcif_result.unwrap_err();
    println!(
        "=== sample.mol2 mmCIF export error message: ===\n{err}\n================================================"
    );
    assert!(
        err.contains("cannot be exported to mmCIF"),
        "error should contain user-friendly message about partial charge: {err}"
    );
}
