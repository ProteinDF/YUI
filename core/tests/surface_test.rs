// SPDX-FileCopyrightText: The Yui development team
// SPDX-License-Identifier: GPL-3.0-or-later

use yui_core::state::AppState;
use yui_core::structure_loader::load_structure_into_state;
use yui_core::surface::{
    ScalarGrid, SurfaceAtom, SurfaceParams, build_molecular_surface,
    build_molecular_surface_from_atoms, extract_isosurface,
};

#[test]
fn test_single_atom_sphere() {
    let r_vdw = 1.5;
    let r_probe = 1.4;
    let expected_r = r_vdw + r_probe; // 2.9 Å

    let atom = SurfaceAtom {
        id: 1,
        position: [0.0, 0.0, 0.0],
        radius: r_vdw,
    };
    let params = SurfaceParams {
        grid_spacing: 0.25, // 高精度検証
        probe_radius: r_probe,
        isovalue: 0.5,
    };

    let mesh = build_molecular_surface_from_atoms(&[atom], &params);

    assert!(!mesh.positions.is_empty());
    assert!(!mesh.indices.is_empty());
    assert!(mesh.is_closed(), "Single atom mesh must be closed");

    // 理論値
    let pi = std::f32::consts::PI;
    let theory_area = 4.0 * pi * expected_r * expected_r;
    let theory_vol = (4.0 / 3.0) * pi * expected_r * expected_r * expected_r;

    let area = mesh.surface_area();
    let vol = mesh.volume();

    println!(
        "Single atom: R={}, Theory Area={}, Measured Area={}",
        expected_r, theory_area, area
    );
    println!("Theory Vol={}, Measured Vol={}", theory_vol, vol);

    let area_err = (area - theory_area).abs() / theory_area;
    let vol_err = (vol - theory_vol).abs() / theory_vol;

    println!(
        "Area error: {:.2}%, Vol error: {:.2}%",
        area_err * 100.0,
        vol_err * 100.0
    );

    assert!(
        area_err <= 0.05,
        "Area error {:.2}% must be <= 5%",
        area_err * 100.0
    );
    assert!(
        vol_err <= 0.05,
        "Volume error {:.2}% must be <= 5%",
        vol_err * 100.0
    );

    // 法線が外向きであること
    for (p, n) in mesh.positions.iter().zip(mesh.normals.iter()) {
        let dot = p[0] * n[0] + p[1] * n[1] + p[2] * n[2];
        assert!(dot > 0.0, "Normal must point outward: dot={dot}");
    }
}

#[test]
fn test_single_atom_sphere_default_spacing() {
    let r_vdw = 1.5;
    let r_probe = 1.4;
    let expected_r = r_vdw + r_probe; // 2.9 Å

    let atom = SurfaceAtom {
        id: 1,
        position: [0.0, 0.0, 0.0],
        radius: r_vdw,
    };
    let params = SurfaceParams {
        grid_spacing: 0.5, // 既定値
        probe_radius: r_probe,
        isovalue: 0.5,
    };

    let mesh = build_molecular_surface_from_atoms(&[atom], &params);

    assert!(!mesh.positions.is_empty());
    assert!(!mesh.indices.is_empty());
    assert!(mesh.is_closed(), "Single atom mesh must be closed");

    let pi = std::f32::consts::PI;
    let theory_area = 4.0 * pi * expected_r * expected_r;
    let theory_vol = (4.0 / 3.0) * pi * expected_r * expected_r * expected_r;

    let area = mesh.surface_area();
    let vol = mesh.volume();

    println!(
        "Spacing 0.5: Theory Area={}, Measured Area={}",
        theory_area, area
    );
    println!("Theory Vol={}, Measured Vol={}", theory_vol, vol);

    let area_err = (area - theory_area).abs() / theory_area;
    let vol_err = (vol - theory_vol).abs() / theory_vol;

    println!(
        "Spacing 0.5: Area error: {:.2}%, Vol error: {:.2}%",
        area_err * 100.0,
        vol_err * 100.0
    );

    assert!(
        area_err <= 0.05,
        "Area error {:.2}% must be <= 5%",
        area_err * 100.0
    );
    assert!(
        vol_err <= 0.05,
        "Volume error {:.2}% must be <= 5%",
        vol_err * 100.0
    );
}

#[test]
fn test_two_overlapping_atoms() {
    // 距離 2.0 Å（R_eff = 2.9 Å なので十分に重なる）
    let atoms = vec![
        SurfaceAtom {
            id: 101,
            position: [0.0, 0.0, 0.0],
            radius: 1.5,
        },
        SurfaceAtom {
            id: 102,
            position: [2.0, 0.0, 0.0],
            radius: 1.5,
        },
    ];
    let params = SurfaceParams {
        probe_radius: 1.4,
        ..Default::default()
    };
    let mesh = build_molecular_surface_from_atoms(&atoms, &params);

    assert!(!mesh.positions.is_empty());
    assert!(!mesh.indices.is_empty());
    assert!(mesh.is_closed(), "Overlapping atoms mesh must be closed");

    let comp_count = mesh.connected_components_count();
    println!("Overlapping atoms component count: {}", comp_count);
    assert_eq!(comp_count, 1, "Should form 1 connected component");

    // 頂点原子IDが入力のいずれかであること
    for &atom_id in &mesh.vertex_atom_ids {
        assert!(atom_id == 101 || atom_id == 102);
    }
}

#[test]
fn test_two_separated_atoms() {
    // 距離 15.0 Å（カットオフ約 8.0 Å なので完全に分離）
    let atoms = vec![
        SurfaceAtom {
            id: 201,
            position: [0.0, 0.0, 0.0],
            radius: 1.5,
        },
        SurfaceAtom {
            id: 202,
            position: [15.0, 0.0, 0.0],
            radius: 1.5,
        },
    ];
    let params = SurfaceParams {
        probe_radius: 1.4,
        ..Default::default()
    };
    let mesh = build_molecular_surface_from_atoms(&atoms, &params);

    assert!(!mesh.positions.is_empty());
    assert!(!mesh.indices.is_empty());
    assert!(mesh.is_closed(), "Separated atoms mesh must be closed");

    let comp_count = mesh.connected_components_count();
    println!("Separated atoms component count: {}", comp_count);
    assert_eq!(comp_count, 2, "Should form 2 connected components");

    let mut id_201_count = 0;
    let mut id_202_count = 0;
    for &atom_id in &mesh.vertex_atom_ids {
        if atom_id == 201 {
            id_201_count += 1;
        } else if atom_id == 202 {
            id_202_count += 1;
        }
    }
    assert!(id_201_count > 0);
    assert!(id_202_count > 0);
}

#[test]
fn test_marching_cubes_analytic_sphere() {
    let radius = 4.0f32;
    let spacing = 0.4f32;
    let origin = [-6.0, -6.0, -6.0];
    let dims = [31, 31, 31];

    // 内部で大きく、外部で小さくなる符号付き距離関数（外向き勾配）
    // f(x, y, z) = R - sqrt(x^2 + y^2 + z^2)
    // 等値レベル 0.0 が半径 R の球面
    let grid = ScalarGrid::from_fn(origin, [spacing; 3], dims, |p| {
        let dist = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
        radius - dist
    });

    let mesh = extract_isosurface(&grid, 0.0);

    assert!(!mesh.positions.is_empty());
    assert!(!mesh.indices.is_empty());
    assert!(mesh.is_closed(), "Analytic sphere mesh must be closed");

    let pi = std::f32::consts::PI;
    let theory_area = 4.0 * pi * radius * radius;
    let area = mesh.surface_area();

    let area_err = (area - theory_area).abs() / theory_area;
    println!(
        "Analytic sphere (R={}): Theory Area={}, Measured Area={}",
        radius, theory_area, area
    );
    println!("Analytic sphere area error: {:.2}%", area_err * 100.0);

    assert!(
        area_err <= 0.05,
        "Area error {:.2}% must be <= 5%",
        area_err * 100.0
    );

    // 外向き法線チェック
    for (p, n) in mesh.positions.iter().zip(mesh.normals.iter()) {
        let dot = p[0] * n[0] + p[1] * n[1] + p[2] * n[2];
        assert!(dot > 0.0, "Normal must point outward: dot={dot}");
    }
}

#[test]
fn test_1hls_molecular_surface() {
    let pdb_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/1hls.pdb");
    let bytes = std::fs::read(pdb_path).expect("failed to read 1hls.pdb");
    let mut state = AppState::default();
    load_structure_into_state(&mut state, &bytes, "1hls.pdb").expect("load 1hls");

    let atoms = &state.molecule.atoms;
    assert_eq!(atoms.len(), 782, "1HLS must have 782 atoms");

    let valid_ids: std::collections::HashSet<u32> = atoms.iter().map(|a| a.id).collect();

    let params = SurfaceParams {
        probe_radius: 1.4,
        ..Default::default()
    }; // spacing=0.5, probe=1.4, isovalue=0.5

    let start = std::time::Instant::now();
    let mesh = build_molecular_surface(atoms, &params);
    let elapsed = start.elapsed();

    println!(
        "1HLS surface generation time (debug/current profile): {:?}",
        elapsed
    );
    println!(
        "1HLS mesh: {} vertices, {} triangles (indices: {})",
        mesh.positions.len(),
        mesh.indices.len() / 3,
        mesh.indices.len()
    );

    assert!(!mesh.positions.is_empty(), "Mesh must not be empty");
    assert!(!mesh.indices.is_empty(), "Mesh indices must not be empty");

    // 全ての頂点の原子IDが実在する原子IDであること
    assert_eq!(mesh.vertex_atom_ids.len(), mesh.positions.len());
    for &atom_id in &mesh.vertex_atom_ids {
        assert!(
            valid_ids.contains(&atom_id),
            "Vertex atom ID {atom_id} must exist in atoms"
        );
    }

    let is_closed = mesh.is_closed();
    println!("1HLS mesh is_closed: {}", is_closed);
    assert!(mesh.is_closed(), "1HLS mesh must be closed");
}

#[test]
fn test_1hls_molecular_surface_default_params() {
    let pdb_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/1hls.pdb");
    let bytes = std::fs::read(pdb_path).expect("failed to read 1hls.pdb");
    let mut state = AppState::default();
    load_structure_into_state(&mut state, &bytes, "1hls.pdb").expect("load 1hls");

    let atoms = &state.molecule.atoms;
    assert_eq!(atoms.len(), 782, "1HLS must have 782 atoms");

    let valid_ids: std::collections::HashSet<u32> = atoms.iter().map(|a| a.id).collect();

    let params = SurfaceParams::default(); // 既定値: probe_radius = 0.0

    let mesh = build_molecular_surface(atoms, &params);

    assert!(!mesh.positions.is_empty(), "Mesh must not be empty");
    assert!(!mesh.indices.is_empty(), "Mesh indices must not be empty");

    // 全ての頂点の原子IDが実在する原子IDであること
    assert_eq!(mesh.vertex_atom_ids.len(), mesh.positions.len());
    for &atom_id in &mesh.vertex_atom_ids {
        assert!(
            valid_ids.contains(&atom_id),
            "Vertex atom ID {atom_id} must exist in atoms"
        );
    }

    assert!(mesh.is_closed(), "Default params 1HLS mesh must be closed");
}

/// 擬似乱数の原子の雲（タンパク質ほどの密度: 1 Å^3 あたり約 0.1 個）
fn atom_cloud(n: usize) -> Vec<SurfaceAtom> {
    let side = ((n as f32) / 0.1).cbrt();
    let mut s = 12345u64;
    let mut r = || {
        s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((s >> 33) as f32) / (1u64 << 31) as f32
    };
    (0..n)
        .map(|i| SurfaceAtom {
            id: i as u32,
            position: [r() * side, r() * side, r() * side],
            radius: 1.2 + r() * 0.8,
        })
        .collect()
}

/// 密度の格子が、定義どおりの式（原子ごとのカットオフ内のガウス関数の和）と合うこと。
/// 軸ごとの積に分けた計算（と、並列化）が、結果を変えていないことの確認。
#[test]
fn test_density_grid_matches_the_direct_formula() {
    let atoms = atom_cloud(60);
    let params = SurfaceParams::default();
    let grid = yui_core::surface::generate_gaussian_density_grid(&atoms, &params);

    let denom = (2.0 * (1.0 / params.isovalue).ln()).sqrt();
    let cutoff_factor = (2.0 * (1.0f32 / 0.005).ln()).sqrt();
    let mut worst = 0.0f32;
    for iz in (0..grid.dims[2]).step_by(3) {
        for iy in (0..grid.dims[1]).step_by(3) {
            for ix in (0..grid.dims[0]).step_by(3) {
                let p = grid.grid_to_world(ix, iy, iz);
                let mut want = 0.0f32;
                for a in &atoms {
                    let alpha = (a.radius + params.probe_radius).max(0.1) / denom;
                    let d2: f32 = (0..3).map(|k| (p[k] - a.position[k]).powi(2)).sum();
                    let cutoff = alpha * cutoff_factor;
                    if d2 <= cutoff * cutoff {
                        want += (-d2 / (2.0 * alpha * alpha)).exp();
                    }
                }
                worst = worst.max((grid.get(ix, iy, iz) - want).abs());
            }
        }
    }
    assert!(worst < 1e-4, "largest difference {worst}");
}

/// 計算時間の確認（`cargo test --release -p yui-core --test surface_test -- --ignored --nocapture`）
#[test]
#[ignore = "timing report; run in release mode"]
fn surface_timing_report() {
    use std::time::Instant;
    for n in [1_000usize, 10_000, 50_000] {
        let atoms = atom_cloud(n);
        let params = SurfaceParams::default();
        let t = Instant::now();
        let grid = yui_core::surface::generate_gaussian_density_grid(&atoms, &params);
        let density = t.elapsed();
        let t = Instant::now();
        let mesh = extract_isosurface(&grid, params.isovalue);
        let mc = t.elapsed();
        let t = Instant::now();
        let full = build_molecular_surface_from_atoms(&atoms, &params);
        let all = t.elapsed();
        eprintln!(
            "n={n}: density {density:?} {:?} | marching cubes {mc:?} ({} triangles) | whole build {all:?} ({} vertices)",
            grid.dims,
            mesh.indices.len() / 3,
            full.positions.len()
        );
    }
}
