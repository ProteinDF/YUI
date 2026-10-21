// SPDX-FileCopyrightText: The Yui development team
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashSet;
use std::time::Instant;
use yui_core::interaction::{
    InteractionKind, resolve_interaction_atom_ids, resolve_interaction_endpoints,
};
use yui_core::structure_loader::parse_structure_bytes;

#[test]
fn test_1hls_pdb_interactions() {
    let pdb_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/1hls.pdb");
    let bytes = std::fs::read(pdb_path).expect("failed to read 1hls.pdb");

    let start = Instant::now();
    let (group, _sec_map, interactions) =
        parse_structure_bytes(&bytes, "1hls.pdb").expect("failed to parse 1hls.pdb");
    let elapsed = start.elapsed();
    println!("1hls.pdb parsed in {:?}", elapsed);

    for (i, flat) in yui_core::atom_group::iter_flat_atoms(&group)
        .take(10)
        .enumerate()
    {
        println!(
            "flat[{i}]: id={}, m={}, c={}, r_key={}, r_name={}, a_key={}, a_name={}",
            flat.id,
            flat.model_key,
            flat.chain_key,
            flat.residue_key,
            flat.residue_name,
            flat.atom_key,
            flat.atom.name
        );
    }

    // 1. 各種別の件数集計
    let disulfide_count = interactions.count_by_kind(InteractionKind::Disulfide);
    let salt_bridge_count = interactions.count_by_kind(InteractionKind::SaltBridge);
    let hbond_count = interactions.count_by_kind(InteractionKind::HydrogenBond);
    let ch_pi_count = interactions.count_by_kind(InteractionKind::ChPi);

    println!("=== 1HLS.pdb Interactions Count ===");
    println!("  Total: {}", interactions.len());
    println!("  Disulfide: {disulfide_count}");
    println!("  SaltBridge: {salt_bridge_count}");
    println!("  HydrogenBond: {hbond_count}");
    println!("  ChPi: {ch_pi_count}");

    // 塩橋のパスの例を出力
    for inter in interactions.filter_by_kind(InteractionKind::SaltBridge) {
        println!(
            "  SaltBridge atoms: {:?}, role: {:?}",
            inter.atoms, inter.donor_acceptor_role
        );
    }

    // 2. ジスルフィド結合の検証 (TASK 事実5: 3本, A6-A11, A7-B7, A20-B19, 距離 2.02 ± 0.05 Å)
    assert_eq!(
        disulfide_count, 3,
        "1HLS should have exactly 3 disulfide bonds"
    );

    let disulfides = interactions.filter_by_kind(InteractionKind::Disulfide);
    let mut expected_pairs = HashSet::from([
        ("A6".to_string(), "A11".to_string()),
        ("A7".to_string(), "B7".to_string()),
        ("A20".to_string(), "B19".to_string()),
    ]);

    for ds in disulfides {
        assert_eq!(ds.atoms.len(), 2);
        let dist = ds.distance.expect("disulfide must have measured distance");
        assert!(
            (dist - 2.02).abs() <= 0.05,
            "SG-SG distance should be 2.02 ± 0.05 Å, got {dist:.3}"
        );

        // 鎖IDと残基キーの抽出（パス例: "/model_1/A/6/SG" または "/1/A/6/SG"）
        let segs0: Vec<&str> = ds.atoms[0].split('/').filter(|s| !s.is_empty()).collect();
        let segs1: Vec<&str> = ds.atoms[1].split('/').filter(|s| !s.is_empty()).collect();
        let cr0 = format!("{}{}", segs0[1], segs0[2]);
        let cr1 = format!("{}{}", segs1[1], segs1[2]);

        let found = expected_pairs.remove(&(cr0.clone(), cr1.clone()))
            || expected_pairs.remove(&(cr1.clone(), cr0.clone()));
        assert!(
            found,
            "Unexpected disulfide bond between residues {cr0} and {cr1}"
        );
    }
    assert!(
        expected_pairs.is_empty(),
        "All expected disulfide bonds must be detected: missing {expected_pairs:?}"
    );

    // 3. 水素結合の検証 (1本以上)
    assert!(
        hbond_count >= 1,
        "At least 1 hydrogen bond should be detected, got {hbond_count}"
    );

    // 4. すべての相互作用について両端座標および原子ID集合が解決できること (Noneが0件)
    let mut unresolved_endpoints = 0;
    let mut unresolved_atom_ids = 0;
    for (idx, inter) in interactions.iter().enumerate() {
        if let Some((p0, p1)) = resolve_interaction_endpoints(&group, inter) {
            assert!(
                p0.iter().all(|c| !c.is_nan()) && p1.iter().all(|c| !c.is_nan()),
                "endpoints coordinates must be finite"
            );
        } else {
            eprintln!(
                "Unresolved endpoint for interaction #{idx}: kind={:?}, atoms={:?}",
                inter.kind, inter.atoms
            );
            unresolved_endpoints += 1;
        }

        if let Some((ids0, ids1)) = resolve_interaction_atom_ids(&group, inter) {
            assert!(
                !ids0.is_empty() && !ids1.is_empty(),
                "atom IDs must not be empty"
            );
        } else {
            eprintln!(
                "Unresolved atom IDs for interaction #{idx}: kind={:?}, atoms={:?}",
                inter.kind, inter.atoms
            );
            unresolved_atom_ids += 1;
        }
    }

    assert_eq!(
        unresolved_endpoints, 0,
        "All interaction endpoints must be resolved (0 None allowed)"
    );
    assert_eq!(
        unresolved_atom_ids, 0,
        "All interaction atom IDs must be resolved (0 None allowed)"
    );
}

#[test]
fn test_1hls_cif_interactions() {
    let cif_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/1HLS.cif");
    let bytes = std::fs::read(cif_path).expect("failed to read 1HLS.cif");

    let t0 = Instant::now();
    let (group, _sec_map, interactions) =
        parse_structure_bytes(&bytes, "1HLS.cif").expect("failed to parse 1HLS.cif");
    let total_elapsed = t0.elapsed();

    // 検出単体の時間を測定するために、group のモデルごとに detect_all だけを実行
    let t_detect = Instant::now();
    let mut count_detected = 0;
    for (_m_key, model) in group.groups() {
        let m_inter = yui_core::interaction::InteractionSet::detect_all(model, None, None);
        count_detected += m_inter.len();
    }
    let detect_elapsed = t_detect.elapsed();
    let base_load_elapsed = total_elapsed.saturating_sub(detect_elapsed);

    println!("=== 1HLS.cif (20 models) Timing Breakdown ===");
    println!("  Total load (with detection): {:?}", total_elapsed);
    println!(
        "  Estimated base load (without detection): {:?}",
        base_load_elapsed
    );
    println!(
        "  Interaction detection alone (20 models): {:?}",
        detect_elapsed
    );
    assert_eq!(count_detected, interactions.len());

    let disulfide_count = interactions.count_by_kind(InteractionKind::Disulfide);
    let salt_bridge_count = interactions.count_by_kind(InteractionKind::SaltBridge);
    let hbond_count = interactions.count_by_kind(InteractionKind::HydrogenBond);
    let ch_pi_count = interactions.count_by_kind(InteractionKind::ChPi);

    println!("=== 1HLS.cif (20 models) Interactions Count ===");
    println!("  Total: {}", interactions.len());
    println!("  Disulfide: {disulfide_count}");
    println!("  SaltBridge: {salt_bridge_count}");
    println!("  HydrogenBond: {hbond_count}");
    println!("  ChPi: {ch_pi_count}");

    for inter in interactions.filter_by_kind(InteractionKind::SaltBridge) {
        println!(
            "  [cif] SaltBridge atoms: {:?}, role: {:?}",
            inter.atoms, inter.donor_acceptor_role
        );
    }

    // 20モデル × 3本 = 60本のジスルフィド結合
    assert_eq!(
        disulfide_count, 60,
        "1HLS.cif (20 models) should have 20 * 3 = 60 disulfide bonds"
    );

    // すべての相互作用について両端座標および原子IDが解決できること
    for inter in interactions.iter() {
        assert!(
            resolve_interaction_endpoints(&group, inter).is_some(),
            "All endpoints in 1HLS.cif must be resolved"
        );
        assert!(
            resolve_interaction_atom_ids(&group, inter).is_some(),
            "All atom IDs in 1HLS.cif must be resolved"
        );
    }
}
