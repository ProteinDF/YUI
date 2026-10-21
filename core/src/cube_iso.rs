//! Gaussian CUBE ファイル（電子密度、分子軌道、静電ポテンシャル）の等値面。
//!
//! 読み込みは `proteindf_bridge::format::Cube` に任せ、ここは、その値を `ScalarGrid`（Å）にして、
//! 分子表面と同じ `extract_isosurface`（Marching Cubes）で正負の等値面にする。
//! 軸に平行な格子（普通の CUBE）はそのまま、斜めの格子は軸に平行な格子に補間して扱う。

use crate::surface::{ScalarGrid, SurfaceMesh, extract_isosurface};
use proteindf_bridge::format::{BOHR_TO_ANGSTROM, Cube, CubeUnit};

/// 斜めの格子を軸に平行な格子にするときの、格子点の数の上限（これを超えるなら、間隔を粗くする）
const MAX_RESAMPLED_POINTS: usize = 8_000_000;

/// CUBE の `dataset` 番目の値を、Å 単位の `ScalarGrid` にする。
///
/// - 単位が Bohr のときは、原点と間隔を Å にする（値はそのまま）。
/// - 間隔のベクトルが x, y, z 軸に平行で正の向きなら、値をそのまま使う。
/// - それ以外（斜めの格子、軸が負の向き）は、軸に平行な格子に三線形補間で再サンプリングする
///   （`resample_oblique`）。
pub fn cube_to_grid(cube: &Cube, dataset: usize) -> Result<ScalarGrid, String> {
    let n = cube.n_grid();
    if n.iter().any(|&v| v < 2) {
        return Err(format!(
            "The CUBE grid is too small: {} x {} x {}",
            n[0], n[1], n[2]
        ));
    }
    let scale = match cube.unit() {
        CubeUnit::Bohr => BOHR_TO_ANGSTROM,
        CubeUnit::Angstrom => 1.0,
    };
    let sp = cube.spacing();
    let o = cube.origin();
    let origin = [o[0] * scale, o[1] * scale, o[2] * scale];
    let lattice = [
        [sp[0][0] * scale, sp[0][1] * scale, sp[0][2] * scale],
        [sp[1][0] * scale, sp[1][1] * scale, sp[1][2] * scale],
        [sp[2][0] * scale, sp[2][1] * scale, sp[2][2] * scale],
    ];
    let raw = cube.data(dataset).map_err(|e| e.to_string())?;
    // CUBE は z が最も速く変わる並び（ix, iy, iz）。ScalarGrid は x が最も速い。
    let value = |ix: usize, iy: usize, iz: usize| raw[(ix * n[1] + iy) * n[2] + iz] as f32;

    let axis_parallel = (0..3).all(|axis| {
        let diag = lattice[axis][axis];
        let off = (0..3)
            .filter(|&k| k != axis)
            .map(|k| lattice[axis][k].abs())
            .fold(0.0f64, f64::max);
        diag > 0.0 && off <= diag * 1e-6
    });
    if axis_parallel {
        let spacing = [
            lattice[0][0] as f32,
            lattice[1][1] as f32,
            lattice[2][2] as f32,
        ];
        let mut grid = ScalarGrid::new(
            [origin[0] as f32, origin[1] as f32, origin[2] as f32],
            spacing,
            n,
            0.0,
        );
        for ix in 0..n[0] {
            for iy in 0..n[1] {
                for iz in 0..n[2] {
                    grid.set(ix, iy, iz, value(ix, iy, iz));
                }
            }
        }
        return Ok(grid);
    }
    resample_oblique(origin, lattice, n, value)
}

/// 3x3 行列（行ベクトルが格子ベクトル）の逆行列。特異ならエラー。
fn invert3(m: [[f64; 3]; 3]) -> Result<[[f64; 3]; 3], String> {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let scale = m.iter().flatten().fold(0.0f64, |a, &v| a.max(v.abs()));
    if det.abs() <= 1e-12 * scale.powi(3) {
        return Err("The CUBE grid vectors are linearly dependent".to_string());
    }
    let c =
        |r1: usize, c1: usize, r2: usize, c2: usize| m[r1][c1] * m[r2][c2] - m[r1][c2] * m[r2][c1];
    Ok([
        [
            c(1, 1, 2, 2) / det,
            -c(0, 1, 2, 2) / det,
            c(0, 1, 1, 2) / det,
        ],
        [
            -c(1, 0, 2, 2) / det,
            c(0, 0, 2, 2) / det,
            -c(0, 0, 1, 2) / det,
        ],
        [
            c(1, 0, 2, 1) / det,
            -c(0, 0, 2, 1) / det,
            c(0, 0, 1, 1) / det,
        ],
    ])
}

/// 斜めの格子（格子点 `origin + i*a0 + j*a1 + k*a2`、`a` は `lattice`）を、軸に平行な格子に
/// 三線形補間で写す。新しい格子は、元の格子点を含む直方体で、間隔は元の格子ベクトルのいちばん短い長さ
/// （点の数が `MAX_RESAMPLED_POINTS` を超えるときは粗くする）。元の格子の外は 0。
pub fn resample_oblique<F: Fn(usize, usize, usize) -> f32>(
    origin: [f64; 3],
    lattice: [[f64; 3]; 3],
    n: [usize; 3],
    value: F,
) -> Result<ScalarGrid, String> {
    // 元の格子の 8 つの角から、直方体を決める
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for corner in 0..8 {
        let idx = [
            (corner & 1) as f64 * (n[0] - 1) as f64,
            ((corner >> 1) & 1) as f64 * (n[1] - 1) as f64,
            ((corner >> 2) & 1) as f64 * (n[2] - 1) as f64,
        ];
        for d in 0..3 {
            let p = origin[d]
                + idx[0] * lattice[0][d]
                + idx[1] * lattice[1][d]
                + idx[2] * lattice[2][d];
            lo[d] = lo[d].min(p);
            hi[d] = hi[d].max(p);
        }
    }
    let inv = invert3(lattice)?; // 実座標 → 格子の添字（行ベクトルの規約なので、q * inv）
    let mut h = lattice
        .iter()
        .map(|a| (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt())
        .fold(f64::INFINITY, f64::min);
    let dims_for = |h: f64| -> [usize; 3] {
        [
            ((hi[0] - lo[0]) / h).ceil() as usize + 1,
            ((hi[1] - lo[1]) / h).ceil() as usize + 1,
            ((hi[2] - lo[2]) / h).ceil() as usize + 1,
        ]
    };
    let mut dims = dims_for(h);
    while dims[0] * dims[1] * dims[2] > MAX_RESAMPLED_POINTS {
        h *= 1.25;
        dims = dims_for(h);
    }
    let mut grid = ScalarGrid::new(
        [lo[0] as f32, lo[1] as f32, lo[2] as f32],
        [h as f32; 3],
        dims,
        0.0,
    );
    let eps = 1e-9;
    for iz in 0..dims[2] {
        for iy in 0..dims[1] {
            for ix in 0..dims[0] {
                let q = [
                    lo[0] + ix as f64 * h - origin[0],
                    lo[1] + iy as f64 * h - origin[1],
                    lo[2] + iz as f64 * h - origin[2],
                ];
                // 添字 f = q * inv（q は行ベクトル）
                let f = [
                    q[0] * inv[0][0] + q[1] * inv[1][0] + q[2] * inv[2][0],
                    q[0] * inv[0][1] + q[1] * inv[1][1] + q[2] * inv[2][1],
                    q[0] * inv[0][2] + q[1] * inv[1][2] + q[2] * inv[2][2],
                ];
                let inside = (0..3).all(|d| f[d] >= -eps && f[d] <= (n[d] - 1) as f64 + eps);
                if !inside {
                    continue;
                }
                let base = [
                    (f[0].floor().max(0.0) as usize).min(n[0] - 2),
                    (f[1].floor().max(0.0) as usize).min(n[1] - 2),
                    (f[2].floor().max(0.0) as usize).min(n[2] - 2),
                ];
                let t = [
                    (f[0] - base[0] as f64).clamp(0.0, 1.0) as f32,
                    (f[1] - base[1] as f64).clamp(0.0, 1.0) as f32,
                    (f[2] - base[2] as f64).clamp(0.0, 1.0) as f32,
                ];
                let mut acc = 0.0f32;
                for dz in 0..2 {
                    for dy in 0..2 {
                        for dx in 0..2 {
                            let w = (if dx == 1 { t[0] } else { 1.0 - t[0] })
                                * (if dy == 1 { t[1] } else { 1.0 - t[1] })
                                * (if dz == 1 { t[2] } else { 1.0 - t[2] });
                            if w > 0.0 {
                                acc += w * value(base[0] + dx, base[1] + dy, base[2] + dz);
                            }
                        }
                    }
                }
                grid.set(ix, iy, iz, acc);
            }
        }
    }
    Ok(grid)
}

/// 読み込んだ CUBE。データセット（分子軌道の CUBE では、軌道ごとに 1 つ）を選んで格子にする。
#[derive(Debug, Clone)]
pub struct CubeSet {
    cube: Cube,
}

impl CubeSet {
    /// CUBE の文字列を読む
    pub fn parse(text: &str) -> Result<Self, String> {
        let cube =
            Cube::parse_str(text).map_err(|e| format!("The CUBE file could not be read: {e}"))?;
        if cube.num_datasets() == 0 {
            return Err("The CUBE file has no data".to_string());
        }
        Ok(Self { cube })
    }

    /// データセットの数（分子軌道の CUBE では軌道の数、そうでなければ 1）
    pub fn num_datasets(&self) -> usize {
        self.cube.num_datasets()
    }

    /// `index` 番目のデータセットの軌道番号（分子軌道の CUBE のときだけ）
    pub fn orbital_number(&self, index: usize) -> Option<i32> {
        self.cube.orbitals().and_then(|o| o.get(index).copied())
    }

    /// CUBE の原子の数
    pub fn num_atoms(&self) -> usize {
        self.cube.num_atoms()
    }

    /// CUBE の原子から作った、原子が並んだだけの `AtomGroup`（Å。原子がなければ `None`）
    pub fn atom_group(&self) -> Result<Option<proteindf_bridge::AtomGroup>, String> {
        if self.cube.num_atoms() == 0 {
            return Ok(None);
        }
        self.cube
            .get_atomgroup()
            .map(Some)
            .map_err(|e| format!("The atoms of the CUBE file could not be read: {e}"))
    }

    /// `index` 番目のデータセットの格子（Å）
    pub fn grid(&self, index: usize) -> Result<ScalarGrid, String> {
        cube_to_grid(&self.cube, index)
    }
}

/// CUBE の文字列から、0 番目のデータセットの `ScalarGrid`（Å）を作る。
pub fn grid_from_cube_text(text: &str) -> Result<ScalarGrid, String> {
    CubeSet::parse(text)?.grid(0)
}

/// 格子の最大の絶対値（等値レベルの上限の目安）
pub fn grid_peak(grid: &ScalarGrid) -> f32 {
    grid.values.iter().fold(0.0f32, |m, &v| m.max(v.abs()))
}

/// 既定の等値レベル。値がすべて 0 以上なら密度（0.05）、符号があれば軌道（0.02）とみなす。
/// どちらも、格子の最大の絶対値より大きいときは、最大値の 1/2 に下げる。
pub fn default_isolevel(grid: &ScalarGrid) -> f32 {
    let (mut min, mut max) = (f32::INFINITY, f32::NEG_INFINITY);
    for &v in &grid.values {
        min = min.min(v);
        max = max.max(v);
    }
    let base = if min >= 0.0 { 0.05 } else { 0.02 };
    let peak = max.max(-min);
    if peak.is_finite() && peak > 0.0 && base > peak {
        peak * 0.5
    } else {
        base
    }
}

/// 正の等値面（`+isolevel`）と負の等値面（`-isolevel`）。負は、値の符号を反転した格子から取る。
/// 値がすべて 0 以上の密度では、負は空になる。
pub fn extract_signed_isosurfaces(grid: &ScalarGrid, isolevel: f32) -> (SurfaceMesh, SurfaceMesh) {
    let level = isolevel.abs();
    let positive = extract_isosurface(grid, level);
    let mut flipped = grid.clone();
    for v in &mut flipped.values {
        *v = -*v;
    }
    let negative = extract_isosurface(&flipped, level);
    (positive, negative)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 原点に中心を持つ球対称の関数 exp(-r^2) を、`n`^3 の格子（間隔 `h` Bohr）で書いた CUBE の文字列。
    fn gaussian_cube(n: usize, h: f64, sign: f64) -> String {
        let mut s = String::from("test\nsphere\n");
        let o = -(n as f64 - 1.0) * h / 2.0;
        s.push_str(&format!("    1 {o:.6} {o:.6} {o:.6}\n"));
        s.push_str(&format!("{n:5} {h:.6} 0.000000 0.000000\n"));
        s.push_str(&format!("{n:5} 0.000000 {h:.6} 0.000000\n"));
        s.push_str(&format!("{n:5} 0.000000 0.000000 {h:.6}\n"));
        s.push_str("    1 1.000000 0.000000 0.000000 0.000000\n");
        let mut count = 0;
        for ix in 0..n {
            for iy in 0..n {
                for iz in 0..n {
                    let x = o + ix as f64 * h;
                    let y = o + iy as f64 * h;
                    let z = o + iz as f64 * h;
                    let v = sign * (-(x * x + y * y + z * z)).exp();
                    s.push_str(&format!(" {v:.8e}"));
                    count += 1;
                    if count % 6 == 0 {
                        s.push('\n');
                    }
                }
            }
        }
        s.push('\n');
        s
    }

    #[test]
    fn test_cube_becomes_a_grid_in_angstrom_with_x_fastest_order() {
        let cube = Cube::parse_str(&gaussian_cube(9, 0.5, 1.0)).unwrap();
        let grid = cube_to_grid(&cube, 0).unwrap();
        assert_eq!(grid.dims, [9, 9, 9]);
        let h = (0.5 * BOHR_TO_ANGSTROM) as f32;
        assert!((grid.spacing[0] - h).abs() < 1e-6);
        assert!((grid.origin[0] + 4.0 * h).abs() < 1e-5);
        // 中心（原点）の値は 1、端は小さい
        assert!((grid.get(4, 4, 4) - 1.0).abs() < 1e-6);
        assert!(grid.get(0, 0, 0) < 1e-4);
        // 並びの入れ替えが正しい: (ix, iy, iz) の値は、CUBE の同じ点と一致する
        for &(ix, iy, iz) in &[(1, 2, 3), (7, 0, 5), (2, 8, 1)] {
            let expected = cube.value(ix, iy, iz).unwrap() as f32;
            assert!((grid.get(ix, iy, iz) - expected).abs() < 1e-7);
        }
    }

    #[test]
    fn test_density_gives_only_the_positive_lobe_and_orbital_sign_gives_negative() {
        let pos = cube_to_grid(&Cube::parse_str(&gaussian_cube(15, 0.4, 1.0)).unwrap(), 0).unwrap();
        let (p, n) = extract_signed_isosurfaces(&pos, 0.3);
        assert!(!p.indices.is_empty() && p.is_closed());
        assert!(n.indices.is_empty());

        let neg =
            cube_to_grid(&Cube::parse_str(&gaussian_cube(15, 0.4, -1.0)).unwrap(), 0).unwrap();
        let (p, n) = extract_signed_isosurfaces(&neg, 0.3);
        assert!(p.indices.is_empty());
        assert!(!n.indices.is_empty() && n.is_closed());
    }

    #[test]
    fn test_isosurface_radius_matches_the_analytic_value() {
        // exp(-r^2) = 0.3 → r = sqrt(-ln 0.3) Bohr
        let grid =
            cube_to_grid(&Cube::parse_str(&gaussian_cube(21, 0.3, 1.0)).unwrap(), 0).unwrap();
        let (mesh, _) = extract_signed_isosurfaces(&grid, 0.3);
        let expected = (-(0.3f64).ln()).sqrt() * BOHR_TO_ANGSTROM;
        let mean: f64 = mesh
            .positions
            .iter()
            .map(|p| ((p[0] * p[0] + p[1] * p[1] + p[2] * p[2]) as f64).sqrt())
            .sum::<f64>()
            / mesh.positions.len() as f64;
        assert!(
            (mean - expected).abs() / expected < 0.03,
            "mean {mean} vs {expected}"
        );
    }

    #[test]
    fn test_default_isolevel_depends_on_the_sign_and_the_peak() {
        let mut g = ScalarGrid::new([0.0; 3], [1.0; 3], [2, 2, 2], 0.0);
        g.values[0] = 3.0;
        assert_eq!(default_isolevel(&g), 0.05);
        g.values[1] = -3.0;
        assert_eq!(default_isolevel(&g), 0.02);
        // 値が小さすぎるときは、最大の絶対値の半分
        let mut small = ScalarGrid::new([0.0; 3], [1.0; 3], [2, 2, 2], 0.0);
        small.values[0] = 0.01;
        assert!((default_isolevel(&small) - 0.005).abs() < 1e-7);
    }

    #[test]
    fn test_oblique_grid_is_resampled_onto_an_axis_parallel_grid() {
        // 球対称の関数を、x 軸の間隔ベクトルを斜めにした格子で書く（せん断された格子）
        let (n, h) = (21usize, 0.3f64);
        let shear = 0.4 * h;
        let origin = [-3.0f64, -3.0, -3.0];
        let mut s = format!(
            "t\nt\n    1 {:.6} {:.6} {:.6}\n",
            origin[0], origin[1], origin[2]
        );
        s.push_str(&format!(
            "{n:5} {h} {shear} 0.0\n{n:5} 0.0 {h} 0.0\n{n:5} 0.0 0.0 {h}\n"
        ));
        s.push_str("    1 1.0 0.0 0.0 0.0\n");
        for ix in 0..n {
            for iy in 0..n {
                for iz in 0..n {
                    let x = origin[0] + ix as f64 * h;
                    let y = origin[1] + ix as f64 * shear + iy as f64 * h;
                    let z = origin[2] + iz as f64 * h;
                    s.push_str(&format!(" {:.8e}\n", (-(x * x + y * y + z * z)).exp()));
                }
            }
        }
        let cube = Cube::parse_str(&s).unwrap();
        let grid = cube_to_grid(&cube, 0).unwrap();
        // 軸に平行な格子になり、間隔は 1 つの値（Bohr → Å）
        assert_eq!(grid.spacing[0], grid.spacing[1]);
        // 補間した値が、元の関数 exp(-r^2) と近い（元の格子の内側の点で）
        let to_bohr = 1.0 / BOHR_TO_ANGSTROM;
        let mut checked = 0;
        for &(ix, iy, iz) in &[(10usize, 10usize, 10usize), (12, 9, 11), (8, 12, 10)] {
            let p = grid.grid_to_world(ix, iy, iz);
            let (x, y, z) = (
                p[0] as f64 * to_bohr,
                p[1] as f64 * to_bohr,
                p[2] as f64 * to_bohr,
            );
            let expected = (-(x * x + y * y + z * z)).exp() as f32;
            if (x * x + y * y + z * z) < 4.0 {
                assert!(
                    (grid.get(ix, iy, iz) - expected).abs() < 0.03,
                    "{:?}: {} vs {}",
                    (ix, iy, iz),
                    grid.get(ix, iy, iz),
                    expected
                );
                checked += 1;
            }
        }
        assert!(checked >= 2);
        // 等値面の半径は、軸に平行な場合と同じ解析解に近い
        let (mesh, _) = extract_signed_isosurfaces(&grid, 0.3);
        assert!(!mesh.indices.is_empty());
        let expected = (-(0.3f64).ln()).sqrt() * BOHR_TO_ANGSTROM;
        let mean: f64 = mesh
            .positions
            .iter()
            .map(|p| ((p[0] * p[0] + p[1] * p[1] + p[2] * p[2]) as f64).sqrt())
            .sum::<f64>()
            / mesh.positions.len() as f64;
        assert!(
            (mean - expected).abs() / expected < 0.05,
            "mean {mean} vs {expected}"
        );
    }

    #[test]
    fn test_negative_axis_direction_and_degenerate_grids() {
        // x 軸が負の向き: 反転した格子として扱える（同じ関数なら、同じ値になる）
        let (n, h) = (9usize, 0.5f64);
        let make = |sign: f64| {
            let start = if sign > 0.0 { -2.0 } else { 2.0 };
            let mut s = format!("t\nt\n    1 {start:.6} -2.0 -2.0\n");
            s.push_str(&format!(
                "{n:5} {:.6} 0.0 0.0\n{n:5} 0.0 {h} 0.0\n{n:5} 0.0 0.0 {h}\n",
                sign * h
            ));
            s.push_str("    1 1.0 0.0 0.0 0.0\n");
            for ix in 0..n {
                for iy in 0..n {
                    for iz in 0..n {
                        let x = start + ix as f64 * sign * h;
                        let y = -2.0 + iy as f64 * h;
                        let z = -2.0 + iz as f64 * h;
                        s.push_str(&format!(" {:.8e}\n", (-(x * x + y * y + z * z)).exp()));
                    }
                }
            }
            s
        };
        let a = cube_to_grid(&Cube::parse_str(&make(1.0)).unwrap(), 0).unwrap();
        let b = cube_to_grid(&Cube::parse_str(&make(-1.0)).unwrap(), 0).unwrap();
        // 中心の値が一致する
        let c = |g: &ScalarGrid| {
            let p = [0.0f32, 0.0, 0.0];
            let f = |d: usize| ((p[d] - g.origin[d]) / g.spacing[d]).round() as usize;
            g.get(f(0), f(1), f(2))
        };
        assert!(
            (c(&a) - 1.0).abs() < 0.05 && (c(&b) - 1.0).abs() < 0.05,
            "{} {}",
            c(&a),
            c(&b)
        );

        // 1 点の格子や、一次従属な格子ベクトルはエラー
        let tiny = "t\nt\n    1 0 0 0\n    1 0.5 0 0\n    2 0 0.5 0\n    2 0 0 0.5\n    1 1.0 0 0 0\n 1.0 1.0 1.0 1.0\n";
        assert!(
            Cube::parse_str(tiny).is_err()
                || cube_to_grid(&Cube::parse_str(tiny).unwrap(), 0).is_err()
        );
        assert!(invert3([[1.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 0.0, 1.0]]).is_err());
    }

    /// 軌道 2 つ（NAtoms < 0）の CUBE。1 つ目は +exp(-r^2)、2 つ目は -exp(-r^2)
    fn two_orbital_cube() -> String {
        let (n, h) = (5usize, 0.5f64);
        let o = -(n as f64 - 1.0) * h / 2.0;
        let mut s = format!("t\nt\n   -1 {o:.6} {o:.6} {o:.6}\n");
        s.push_str(&format!(
            "{n:5} {h} 0.0 0.0\n{n:5} 0.0 {h} 0.0\n{n:5} 0.0 0.0 {h}\n"
        ));
        s.push_str("    1 1.0 0.0 0.0 0.0\n    2    2    7\n");
        for ix in 0..n {
            for iy in 0..n {
                for iz in 0..n {
                    let (x, y, z) = (o + ix as f64 * h, o + iy as f64 * h, o + iz as f64 * h);
                    let v = (-(x * x + y * y + z * z)).exp();
                    s.push_str(&format!(" {:.8e} {:.8e}\n", v, -v));
                }
            }
        }
        s
    }

    #[test]
    fn test_orbital_cube_has_one_dataset_per_orbital() {
        let set = CubeSet::parse(&two_orbital_cube()).unwrap();
        assert_eq!(set.num_datasets(), 2);
        assert_eq!(set.orbital_number(0), Some(2));
        assert_eq!(set.orbital_number(1), Some(7));
        let (a, b) = (set.grid(0).unwrap(), set.grid(1).unwrap());
        assert!((a.get(2, 2, 2) - 1.0).abs() < 1e-6);
        assert!((b.get(2, 2, 2) + 1.0).abs() < 1e-6);
        assert!((grid_peak(&b) - 1.0).abs() < 1e-6);
        assert!(set.grid(2).is_err());
        assert_eq!(set.orbital_number(5), None);
    }

    #[test]
    fn test_plain_cube_has_one_dataset_without_an_orbital_number() {
        let set = CubeSet::parse(&gaussian_cube(3, 0.5, 1.0)).unwrap();
        assert_eq!(set.num_datasets(), 1);
        assert_eq!(set.orbital_number(0), None);
        assert!(CubeSet::parse("garbage").is_err());
    }

    #[test]
    fn test_sample_files_in_the_repository_load() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/");
        let sphere =
            CubeSet::parse(&std::fs::read_to_string(format!("{dir}sphere.cube")).unwrap()).unwrap();
        assert_eq!(sphere.num_datasets(), 1);
        let orbitals =
            CubeSet::parse(&std::fs::read_to_string(format!("{dir}orbitals.cube")).unwrap())
                .unwrap();
        assert_eq!(orbitals.num_datasets(), 2);
        assert_eq!(orbitals.orbital_number(1), Some(2));
        // 2 つ目は p 型: 正負の両方の面ができる
        let grid = orbitals.grid(1).unwrap();
        let (p, n) = extract_signed_isosurfaces(&grid, default_isolevel(&grid));
        assert!(!p.indices.is_empty() && !n.indices.is_empty());
    }
}
