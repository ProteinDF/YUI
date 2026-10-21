// SPDX-FileCopyrightText: The Yui development team
// SPDX-License-Identifier: GPL-3.0-or-later

//! 分子表面メッシュ生成および等値面抽出（CPU参照実装）モジュール。
//!
//! # 密度の定式化と出典
//! 分子表面のガウス密度場（ガウス関数の和としての関数形）は、VMD の QuickSurf アルゴリズムの定式化に基づきます。
//! - 出典:
//!   1. Michael Krone, John E. Stone, Thomas Ertl, Klaus Schulten,
//!      "Fast Visualization of Gaussian Density Surfaces for Molecular Dynamics and Particle System Trajectories",
//!      EuroVis 2012 - Short Papers, pp. 67–71, 2012.
//!      DOI: 10.2312/PE/EuroVisShort/EuroVisShort2012/067-071
//!   2. VMD User's Guide, QuickSurf
//!      (https://www.ks.uiuc.edu/Research/vmd/vmd-1.9.3/ug/node73.html)
//!
//! 式（Krone et al. 2012 に基づく密度場）:
//! \rho(\mathbf{r}) = \sum_{i=1}^N \exp\left( -\frac{\|\mathbf{r} - \mathbf{r}_i\|^2}{2 \alpha_i^2} \right)
//!
//! パラメータ幅 $\alpha_i$ の決定（YUI 独自の選択）:
//! 密度の関数形は Krone et al. 2012 に従いますが、ガウス関数の幅 $\alpha_i$ を決める方法は QuickSurf 論文のものではなく、
//! 孤立原子の等値面が想定表面半径 $R_i = r_i + r_{\text{probe}}$ と一致するようにした YUI 独自の選択（パラメータ決定法）です。
//! 各原子 $i$（中心 $\mathbf{r}_i$, 半径 $r_i$）と溶媒プローブ半径 $r_{\text{probe}}$ に対し、
//! 等値面レベル（isovalue）を $C$ とするとき、孤立した単一原子において
//! $\|\mathbf{r} - \mathbf{r}_i\| = R_i$ の位置で $\rho = C$ となる条件より解析的に導出されます:
//! \exp\left( -\frac{R_i^2}{2 \alpha_i^2} \right) = C \implies \alpha_i = \frac{R_i}{\sqrt{2 \ln(1/C)}}
//!
//! 既定値:
//! - 格子間隔 (grid spacing): 0.5 Å
//! - プローブ半径 (probe radius): 0.0 Å
//! - 等値面レベル (isovalue): 0.5 (このとき $\alpha_i = R_i / \sqrt{2 \ln 2} \approx R_i / 1.17741$)
//! - カットオフ距離: 寄与が $\epsilon = 0.005$ 以下となる距離 $d_{\text{cutoff}, i} = \alpha_i \sqrt{2 \ln(1/\epsilon)} \approx 3.255 \alpha_i$
//!
//! # 等値面抽出（Marching Cubes）の出典
//! 等値面の多角形化（Polygonisation）には、Marching Cubes アルゴリズムの標準的なルックアップテーブルを採用しています。
//! - 出典:
//!   1. Paul Bourke (1994), "Polygonising a scalar field"
//!      (http://paulbourke.net/geometry/polygonise/)
//!   2. William E. Lorensen and Harvey E. Cline,
//!      "Marching Cubes: A High Resolution 3D Surface Construction Algorithm",
//!      ACM Computer Graphics (SIGGRAPH '87 Proceedings), Vol. 21, No. 4, pp. 163–169, 1987.
//!      DOI: 10.1145/37401.37422

use crate::spatial::AtomBvh;
use crate::state::Atom;
use std::collections::HashMap;

/// 3次元スカラー場（グリッド）
///
/// 分子表面生成のほか、将来の Gaussian CUBE ファイル等による電子密度・分子軌道の
/// 等値面生成にも共通して利用可能な汎用格子データ構造。
#[derive(Debug, Clone, PartialEq)]
pub struct ScalarGrid {
    /// グリッドの原点座標 [x, y, z] (Å)
    pub origin: [f32; 3],
    /// 各軸方向の格子間隔 [dx, dy, dz] (Å)
    pub spacing: [f32; 3],
    /// 各軸方向の格子点数 [nx, ny, nz]
    pub dims: [usize; 3],
    /// 格子点上のスカラー値（インデックス: ix + iy * nx + iz * nx * ny）
    pub values: Vec<f32>,
}

impl ScalarGrid {
    /// 全格子点を指定された初期値で埋めたスカラー格子を生成する。
    pub fn new(origin: [f32; 3], spacing: [f32; 3], dims: [usize; 3], initial_value: f32) -> Self {
        let size = dims[0] * dims[1] * dims[2];
        Self {
            origin,
            spacing,
            dims,
            values: vec![initial_value; size],
        }
    }

    /// 各格子点のワールド座標から関数 `f` でスカラー値を計算してスカラー格子を生成する。
    pub fn from_fn<F>(origin: [f32; 3], spacing: [f32; 3], dims: [usize; 3], f: F) -> Self
    where
        F: Fn([f32; 3]) -> f32,
    {
        let mut grid = Self::new(origin, spacing, dims, 0.0);
        for iz in 0..dims[2] {
            for iy in 0..dims[1] {
                for ix in 0..dims[0] {
                    let pos = grid.grid_to_world(ix, iy, iz);
                    grid.set(ix, iy, iz, f(pos));
                }
            }
        }
        grid
    }

    /// 格子インデックス (ix, iy, iz) をフラットインデックスに変換する。
    #[inline]
    pub fn index_of(&self, ix: usize, iy: usize, iz: usize) -> usize {
        ix + iy * self.dims[0] + iz * self.dims[0] * self.dims[1]
    }

    /// 格子点 (ix, iy, iz) のスカラー値を取得する。
    #[inline]
    pub fn get(&self, ix: usize, iy: usize, iz: usize) -> f32 {
        self.values[self.index_of(ix, iy, iz)]
    }

    /// 格子点 (ix, iy, iz) のスカラー値を設定する。
    #[inline]
    pub fn set(&mut self, ix: usize, iy: usize, iz: usize, val: f32) {
        let idx = self.index_of(ix, iy, iz);
        self.values[idx] = val;
    }

    /// 格子点 (ix, iy, iz) のスカラー値に `delta` を加算する。
    #[inline]
    pub fn add(&mut self, ix: usize, iy: usize, iz: usize, delta: f32) {
        let idx = self.index_of(ix, iy, iz);
        self.values[idx] += delta;
    }

    /// 格子インデックス (ix, iy, iz) をワールド座標 [x, y, z] (Å) に変換する。
    #[inline]
    pub fn grid_to_world(&self, ix: usize, iy: usize, iz: usize) -> [f32; 3] {
        [
            self.origin[0] + ix as f32 * self.spacing[0],
            self.origin[1] + iy as f32 * self.spacing[1],
            self.origin[2] + iz as f32 * self.spacing[2],
        ]
    }

    /// 格子点における中心差分勾配 $\nabla \rho$ を計算する（境界点では片側差分）。
    pub fn gradient_at(&self, ix: usize, iy: usize, iz: usize) -> [f32; 3] {
        let gx = if self.dims[0] <= 1 {
            0.0
        } else if ix == 0 {
            (self.get(1, iy, iz) - self.get(0, iy, iz)) / self.spacing[0]
        } else if ix + 1 >= self.dims[0] {
            (self.get(ix, iy, iz) - self.get(ix - 1, iy, iz)) / self.spacing[0]
        } else {
            (self.get(ix + 1, iy, iz) - self.get(ix - 1, iy, iz)) / (2.0 * self.spacing[0])
        };

        let gy = if self.dims[1] <= 1 {
            0.0
        } else if iy == 0 {
            (self.get(ix, 1, iz) - self.get(ix, 0, iz)) / self.spacing[1]
        } else if iy + 1 >= self.dims[1] {
            (self.get(ix, iy, iz) - self.get(ix, iy - 1, iz)) / self.spacing[1]
        } else {
            (self.get(ix, iy + 1, iz) - self.get(ix, iy - 1, iz)) / (2.0 * self.spacing[1])
        };

        let gz = if self.dims[2] <= 1 {
            0.0
        } else if iz == 0 {
            (self.get(ix, iy, 1) - self.get(ix, iy, 0)) / self.spacing[2]
        } else if iz + 1 >= self.dims[2] {
            (self.get(ix, iy, iz) - self.get(ix, iy, iz - 1)) / self.spacing[2]
        } else {
            (self.get(ix, iy, iz + 1) - self.get(ix, iy, iz - 1)) / (2.0 * self.spacing[2])
        };

        [gx, gy, gz]
    }
}

/// 分子表面生成のパラメータ
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceParams {
    /// 格子間隔 (Å)。既定値: 0.5
    pub grid_spacing: f32,
    /// 溶媒プローブ半径 (Å)。既定値: 0.0
    pub probe_radius: f32,
    /// 等値面レベル (isovalue)。既定値: 0.5
    pub isovalue: f32,
}

impl Default for SurfaceParams {
    fn default() -> Self {
        Self {
            grid_spacing: 0.5,
            probe_radius: 0.0,
            isovalue: 0.5,
        }
    }
}

/// 分子表面生成用の入力原子データ
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceAtom {
    pub id: u32,
    pub position: [f32; 3],
    pub radius: f32,
}

impl From<&Atom> for SurfaceAtom {
    fn from(a: &Atom) -> Self {
        Self {
            id: a.id,
            position: a.position,
            radius: a.radius,
        }
    }
}

/// 生成された分子表面の三角形メッシュ
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SurfaceMesh {
    /// 頂点座標 [x, y, z] (Å)
    pub positions: Vec<[f32; 3]>,
    /// 頂点法線 [nx, ny, nz]（表面外向きの単位ベクトル）
    pub normals: Vec<[f32; 3]>,
    /// 三角形インデックス列（3要素ごとに1枚の三角形）
    pub indices: Vec<u32>,
    /// 各頂点に最も近い原子の ID
    pub vertex_atom_ids: Vec<u32>,
}

impl SurfaceMesh {
    /// メッシュ全体の表面積 (Å²) を計算する。
    pub fn surface_area(&self) -> f32 {
        let mut area = 0.0;
        for &[i0, i1, i2] in self.indices.as_chunks::<3>().0 {
            let p0 = self.positions[i0 as usize];
            let p1 = self.positions[i1 as usize];
            let p2 = self.positions[i2 as usize];
            let v1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
            let v2 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
            let cross = [
                v1[1] * v2[2] - v1[2] * v2[1],
                v1[2] * v2[0] - v1[0] * v2[2],
                v1[0] * v2[1] - v1[1] * v2[0],
            ];
            let cross_len =
                (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
            area += 0.5 * cross_len;
        }
        area
    }

    /// 発散定理に基づくメッシュの体積 (Å³) を計算する。
    ///
    /// 閉じたメッシュにおいて、四面体符号付き体積の総和 $\frac{1}{6} \sum (\mathbf{a} \times \mathbf{b}) \cdot \mathbf{c}$ により求めます。
    pub fn volume(&self) -> f32 {
        let mut vol = 0.0;
        for &[i0, i1, i2] in self.indices.as_chunks::<3>().0 {
            let a = self.positions[i0 as usize];
            let b = self.positions[i1 as usize];
            let c = self.positions[i2 as usize];
            let bxc = [
                b[1] * c[2] - b[2] * c[1],
                b[2] * c[0] - b[0] * c[2],
                b[0] * c[1] - b[1] * c[0],
            ];
            let det = a[0] * bxc[0] + a[1] * bxc[1] + a[2] * bxc[2];
            vol += det / 6.0;
        }
        vol.abs()
    }

    /// メッシュが閉じているか（すべての辺がちょうど2つの三角形に共有されているか）を判定する。
    pub fn is_closed(&self) -> bool {
        if self.indices.is_empty() {
            return false;
        }
        let mut edge_counts = HashMap::new();
        for &[i0, i1, i2] in self.indices.as_chunks::<3>().0 {
            let edges = [
                (i0.min(i1), i0.max(i1)),
                (i1.min(i2), i1.max(i2)),
                (i2.min(i0), i2.max(i0)),
            ];
            for e in edges {
                *edge_counts.entry(e).or_insert(0usize) += 1;
            }
        }
        edge_counts.values().all(|&c| c == 2)
    }

    /// メッシュの連結成分の個数を計算する。
    pub fn connected_components_count(&self) -> usize {
        if self.positions.is_empty() || self.indices.is_empty() {
            return 0;
        }
        let num_vertices = self.positions.len();
        let mut parent: Vec<usize> = (0..num_vertices).collect();

        fn find(parent: &mut [usize], mut i: usize) -> usize {
            while parent[i] != i {
                parent[i] = parent[parent[i]];
                i = parent[i];
            }
            i
        }

        fn union(parent: &mut [usize], i: usize, j: usize) {
            let root_i = find(parent, i);
            let root_j = find(parent, j);
            if root_i != root_j {
                parent[root_i] = root_j;
            }
        }

        for &[i0, i1, i2] in self.indices.as_chunks::<3>().0 {
            union(&mut parent, i0 as usize, i1 as usize);
            union(&mut parent, i1 as usize, i2 as usize);
        }

        let mut referenced_roots = std::collections::HashSet::new();
        for &idx in &self.indices {
            referenced_roots.insert(find(&mut parent, idx as usize));
        }
        referenced_roots.len()
    }
}

/// 密度の足し込みに使う、原子ごとの値
struct AtomCalc {
    pos: [f32; 3],
    two_alpha_sq: f32,
    cutoff_sq: f32,
    cutoff: f32,
}

/// 密度の格子の形（原点・間隔・格子点数）
struct DensitySetup {
    origin: [f32; 3],
    spacing: f32,
    dims: [usize; 3],
}

/// 軸 `axis` の方向に、原子の影響範囲にある格子の添字の範囲（両端を含む）
fn axis_index_range(ac: &AtomCalc, setup: &DensitySetup, axis: usize) -> (usize, usize) {
    let lo = (((ac.pos[axis] - ac.cutoff - setup.origin[axis]) / setup.spacing).floor() as isize)
        .max(0) as usize;
    let hi = (((ac.pos[axis] + ac.cutoff - setup.origin[axis]) / setup.spacing).ceil() as usize)
        .min(setup.dims[axis] - 1);
    (lo, hi)
}

/// z の層 `z0..z1` のぶんの密度を、`slab`（その層の値の並び）に足し込む。
///
/// ガウス関数は軸ごとの積に分けられる: $\exp(-(dx^2+dy^2+dz^2)/2\alpha^2) =
/// e_x e_y e_z$。原子ごとに軸ごとの表を作っておけば、内側のループは掛け算だけで済み、
/// `exp` を格子点ごとに呼ばなくてよい（これが全体の時間のほとんどだった）。
fn accumulate_slab(
    atoms: &[AtomCalc],
    setup: &DensitySetup,
    z0: usize,
    z1: usize,
    slab: &mut [f32],
) {
    let [nx, ny, _] = setup.dims;
    let (origin, spacing) = (setup.origin, setup.spacing);
    let (mut dx2, mut ex, mut dy2, mut ey) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());

    for ac in atoms {
        let (min_z, max_z) = axis_index_range(ac, setup, 2);
        let (iz_lo, iz_hi) = (min_z.max(z0), max_z.min(z1 - 1));
        if iz_lo > iz_hi {
            continue;
        }
        let (min_x, max_x) = axis_index_range(ac, setup, 0);
        let (min_y, max_y) = axis_index_range(ac, setup, 1);

        // x と y の表（添字 min_x.. と min_y.. から数える）
        dx2.clear();
        ex.clear();
        for ix in min_x..=max_x {
            let d = origin[0] + ix as f32 * spacing - ac.pos[0];
            dx2.push(d * d);
            ex.push((-d * d / ac.two_alpha_sq).exp());
        }
        dy2.clear();
        ey.clear();
        for iy in min_y..=max_y {
            let d = origin[1] + iy as f32 * spacing - ac.pos[1];
            dy2.push(d * d);
            ey.push((-d * d / ac.two_alpha_sq).exp());
        }

        for iz in iz_lo..=iz_hi {
            let dz = origin[2] + iz as f32 * spacing - ac.pos[2];
            let dz_sq = dz * dz;
            let ez = (-dz_sq / ac.two_alpha_sq).exp();
            let base = (iz - z0) * nx * ny;

            for iy in min_y..=max_y {
                let dyz_sq = dy2[iy - min_y] + dz_sq;
                if dyz_sq > ac.cutoff_sq {
                    continue;
                }
                let eyz = ey[iy - min_y] * ez;
                let row = base + iy * nx;
                for ix in min_x..=max_x {
                    if dx2[ix - min_x] + dyz_sq <= ac.cutoff_sq {
                        slab[row + ix] += ex[ix - min_x] * eyz;
                    }
                }
            }
        }
    }
}

/// 原子の位置と半径からガウス密度場を計算し、スカラー格子を生成する。
///
/// ガウス密度の関数形は VMD QuickSurf (Krone et al. 2012) に基づき、各原子のガウス幅 $\alpha_i$ は
/// 孤立原子で想定表面半径 $R_i = r_i + r_{\text{probe}}$ となる YUI 独自のパラメータ決定法に従います。
/// 各原子の影響範囲（カットオフ）内の格子点のみに値を足し込むことで、全原子×全格子の総当たりを回避します。
pub fn generate_gaussian_density_grid(atoms: &[SurfaceAtom], params: &SurfaceParams) -> ScalarGrid {
    if atoms.is_empty() {
        return ScalarGrid::new([0.0, 0.0, 0.0], [params.grid_spacing; 3], [1, 1, 1], 0.0);
    }

    let isovalue = params.isovalue;
    let inv_c = 1.0 / isovalue;
    let denom_factor = (2.0 * inv_c.ln()).sqrt(); // sqrt(2 ln(1/C))

    // カットオフ判定閾値 epsilon = 0.005 (0.5%)
    let epsilon = 0.005f32;
    let cutoff_factor = (2.0 * (1.0 / epsilon).ln()).sqrt();

    // 全原子のAABBと最大カットオフ半径を計算
    let mut min_pos = atoms[0].position;
    let mut max_pos = atoms[0].position;
    let mut max_cutoff = 0.0f32;

    let mut atom_calcs = Vec::with_capacity(atoms.len());

    for a in atoms {
        for k in 0..3 {
            min_pos[k] = min_pos[k].min(a.position[k]);
            max_pos[k] = max_pos[k].max(a.position[k]);
        }
        let eff_r = (a.radius + params.probe_radius).max(0.1);
        let alpha = eff_r / denom_factor;
        let two_alpha_sq = 2.0 * alpha * alpha;
        let cutoff = alpha * cutoff_factor;
        if cutoff > max_cutoff {
            max_cutoff = cutoff;
        }
        atom_calcs.push(AtomCalc {
            pos: a.position,
            two_alpha_sq,
            cutoff_sq: cutoff * cutoff,
            cutoff,
        });
    }

    let spacing = params.grid_spacing;
    // 余白（padding）: カットオフ分 + 格子間隔
    let padding = max_cutoff + spacing * 2.0;
    let origin = [
        min_pos[0] - padding,
        min_pos[1] - padding,
        min_pos[2] - padding,
    ];
    let max_bound = [
        max_pos[0] + padding,
        max_pos[1] + padding,
        max_pos[2] + padding,
    ];

    let dims = [
        ((max_bound[0] - origin[0]) / spacing).ceil() as usize + 1,
        ((max_bound[1] - origin[1]) / spacing).ceil() as usize + 1,
        ((max_bound[2] - origin[2]) / spacing).ceil() as usize + 1,
    ];

    let mut grid = ScalarGrid::new(origin, [spacing, spacing, spacing], dims, 0.0);

    let setup = DensitySetup {
        origin,
        spacing,
        dims,
    };
    // z 方向の層（スラブ）ごとに足し込む。スラブは互いに重ならないので、並列にしても競合せず、
    // 各格子点への足し込みの順序（原子の順）は直列のときと同じなので、結果も同じになる。
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        let layer = dims[0] * dims[1];
        let slab_layers = dims[2].div_ceil(rayon::current_num_threads() * 4).max(1);
        grid.values
            .par_chunks_mut(layer * slab_layers)
            .enumerate()
            .for_each(|(i, slab)| {
                let z0 = i * slab_layers;
                accumulate_slab(&atom_calcs, &setup, z0, z0 + slab.len() / layer, slab);
            });
    }
    #[cfg(target_arch = "wasm32")]
    accumulate_slab(&atom_calcs, &setup, 0, dims[2], &mut grid.values);

    grid
}

/// 辺のキー用の、速い（暗号用でない）ハッシュ。キーは小さな整数の組なので、乗算と回転で足りる。
#[derive(Default, Clone, Copy)]
struct FastHash;

struct FastHasher(u64);

impl std::hash::BuildHasher for FastHash {
    type Hasher = FastHasher;
    fn build_hasher(&self) -> FastHasher {
        FastHasher(0)
    }
}

impl std::hash::Hasher for FastHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(b as u64);
        }
    }
    fn write_u8(&mut self, i: u8) {
        self.write_u64(i as u64);
    }
    fn write_usize(&mut self, i: usize) {
        self.write_u64(i as u64);
    }
    fn write_u64(&mut self, i: u64) {
        self.0 = (self.0.rotate_left(5) ^ i).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

/// Marching Cubes アルゴリズムにより、スカラー格子から指定等値レベルの三角形メッシュを生成する。
///
/// 隣接セル間で共有される辺の頂点は一意な格子辺キーにより共有インデックス化されます。
/// 法線は格子点の中心差分勾配を三線形・線形補間して外向きに正規化します。
pub fn extract_isosurface(grid: &ScalarGrid, isolevel: f32) -> SurfaceMesh {
    let dims = grid.dims;
    if dims[0] < 2 || dims[1] < 2 || dims[2] < 2 {
        return SurfaceMesh {
            positions: Vec::new(),
            normals: Vec::new(),
            indices: Vec::new(),
            vertex_atom_ids: Vec::new(),
        };
    }

    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut indices = Vec::new();

    // 辺の一意キー (axis, gx, gy, gz) -> 頂点インデックス
    // （格子点が百万を超えることもあるので、暗号用でない速いハッシュを使う）
    let mut edge_vertex_map: HashMap<(u8, usize, usize, usize), u32, FastHash> = HashMap::default();

    // 立方体の8頂点の相対オフセット
    const VERTEX_OFFSETS: [[usize; 3]; 8] = [
        [0, 0, 0], // 0
        [1, 0, 0], // 1
        [1, 1, 0], // 2
        [0, 1, 0], // 3
        [0, 0, 1], // 4
        [1, 0, 1], // 5
        [1, 1, 1], // 6
        [0, 1, 1], // 7
    ];

    // 立方体の12辺の端点頂点ペア (0..7)
    const EDGE_VERTICES: [[usize; 2]; 12] = [
        [0, 1],
        [1, 2],
        [2, 3],
        [3, 0],
        [4, 5],
        [5, 6],
        [6, 7],
        [7, 4],
        [0, 4],
        [1, 5],
        [2, 6],
        [3, 7],
    ];

    for iz in 0..dims[2] - 1 {
        for iy in 0..dims[1] - 1 {
            for ix in 0..dims[0] - 1 {
                // 8頂点の値と座標
                let mut cell_values = [0.0f32; 8];
                let mut cubeindex = 0u8;

                for k in 0..8 {
                    let off = VERTEX_OFFSETS[k];
                    let val = grid.get(ix + off[0], iy + off[1], iz + off[2]);
                    cell_values[k] = val;
                    if val < isolevel {
                        cubeindex |= 1 << k;
                    }
                }

                let edge_flags = EDGE_TABLE[cubeindex as usize];
                if edge_flags == 0 {
                    continue;
                }

                // 辺ごとの頂点インデックスキャッシュ (12本)
                let mut cell_edge_vertices = [0u32; 12];

                for e in 0..12 {
                    if (edge_flags & (1 << e)) == 0 {
                        continue;
                    }

                    let v0_local = EDGE_VERTICES[e][0];
                    let v1_local = EDGE_VERTICES[e][1];

                    let off0 = VERTEX_OFFSETS[v0_local];
                    let off1 = VERTEX_OFFSETS[v1_local];

                    let p0_grid = (ix + off0[0], iy + off0[1], iz + off0[2]);
                    let p1_grid = (ix + off1[0], iy + off1[1], iz + off1[2]);

                    // 辞書順で小さい方をキーの起点とし、変化している軸を特定
                    let (axis, key_origin) = if p0_grid.0 != p1_grid.0 {
                        (0u8, (p0_grid.0.min(p1_grid.0), p0_grid.1, p0_grid.2))
                    } else if p0_grid.1 != p1_grid.1 {
                        (1u8, (p0_grid.0, p0_grid.1.min(p1_grid.1), p0_grid.2))
                    } else {
                        (2u8, (p0_grid.0, p0_grid.1, p0_grid.2.min(p1_grid.2)))
                    };

                    let edge_key = (axis, key_origin.0, key_origin.1, key_origin.2);

                    if let Some(&vert_idx) = edge_vertex_map.get(&edge_key) {
                        cell_edge_vertices[e] = vert_idx;
                    } else {
                        // 線形補間
                        let val0 = cell_values[v0_local];
                        let val1 = cell_values[v1_local];
                        let t = if (val1 - val0).abs() < 1e-6 {
                            0.5
                        } else {
                            ((isolevel - val0) / (val1 - val0)).clamp(0.0, 1.0)
                        };

                        let pos0 = grid.grid_to_world(p0_grid.0, p0_grid.1, p0_grid.2);
                        let pos1 = grid.grid_to_world(p1_grid.0, p1_grid.1, p1_grid.2);

                        let vert_pos = [
                            pos0[0] + t * (pos1[0] - pos0[0]),
                            pos0[1] + t * (pos1[1] - pos0[1]),
                            pos0[2] + t * (pos1[2] - pos0[2]),
                        ];

                        // 勾配は、表面に頂点ができる辺の両端でだけ計算する（格子点の大半は表面から遠い）
                        let g0 = grid.gradient_at(p0_grid.0, p0_grid.1, p0_grid.2);
                        let g1 = grid.gradient_at(p1_grid.0, p1_grid.1, p1_grid.2);

                        let interp_grad = [
                            g0[0] + t * (g1[0] - g0[0]),
                            g0[1] + t * (g1[1] - g0[1]),
                            g0[2] + t * (g1[2] - g0[2]),
                        ];

                        // 外向き法線は密度の減少方向 (-grad)
                        let mut nx = -interp_grad[0];
                        let mut ny = -interp_grad[1];
                        let mut nz = -interp_grad[2];
                        let len = (nx * nx + ny * ny + nz * nz).sqrt();
                        if len > 1e-6 {
                            nx /= len;
                            ny /= len;
                            nz /= len;
                        } else {
                            nx = 0.0;
                            ny = 1.0;
                            nz = 0.0;
                        }

                        let new_vert_idx = positions.len() as u32;
                        positions.push(vert_pos);
                        normals.push([nx, ny, nz]);

                        edge_vertex_map.insert(edge_key, new_vert_idx);
                        cell_edge_vertices[e] = new_vert_idx;
                    }
                }

                // 三角形のインデックス列生成
                // Paul Bourke の triTable は val < isolevel の頂点を基準にしており、
                // 外向き法線と整合させるためにワインディングを反転 (0, 2, 1) で追加します。
                let tri_row = &TRI_TABLE[cubeindex as usize];
                for i in (0..16).step_by(3) {
                    if tri_row[i] == -1 {
                        break;
                    }
                    let e0 = tri_row[i] as usize;
                    let e1 = tri_row[i + 1] as usize;
                    let e2 = tri_row[i + 2] as usize;

                    indices.push(cell_edge_vertices[e0]);
                    indices.push(cell_edge_vertices[e2]);
                    indices.push(cell_edge_vertices[e1]);
                }
            }
        }
    }

    SurfaceMesh {
        positions,
        normals,
        indices,
        vertex_atom_ids: Vec::new(),
    }
}

/// 原子一覧とパラメータを受け取り、分子表面メッシュ（最近傍原子ID対応付け済み）を生成する。
pub fn build_molecular_surface_from_atoms(
    atoms: &[SurfaceAtom],
    params: &SurfaceParams,
) -> SurfaceMesh {
    if atoms.is_empty() {
        return SurfaceMesh {
            positions: Vec::new(),
            normals: Vec::new(),
            indices: Vec::new(),
            vertex_atom_ids: Vec::new(),
        };
    }

    let grid = generate_gaussian_density_grid(atoms, params);
    let mut mesh = extract_isosurface(&grid, params.isovalue);

    // 各頂点に最も近い原子の ID を探索して対応付ける
    let positions: Vec<[f32; 3]> = atoms.iter().map(|a| a.position).collect();
    let bvh = AtomBvh::build(&positions);

    let nearest_id = |pos: &[f32; 3]| match bvh.query_nearest(*pos) {
        Some((nearest_idx, _)) => atoms[nearest_idx].id,
        None => atoms[0].id,
    };
    // 頂点ごとに独立なので、ネイティブ版では並列にする（順序は変わらない）
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        mesh.vertex_atom_ids = mesh.positions.par_iter().map(nearest_id).collect();
    }
    #[cfg(target_arch = "wasm32")]
    {
        mesh.vertex_atom_ids = mesh.positions.iter().map(nearest_id).collect();
    }

    mesh
}

/// `core::state::Atom` スライスから分子表面メッシュを生成する。
pub fn build_molecular_surface(atoms: &[Atom], params: &SurfaceParams) -> SurfaceMesh {
    let surface_atoms: Vec<SurfaceAtom> = atoms.iter().map(SurfaceAtom::from).collect();
    build_molecular_surface_from_atoms(&surface_atoms, params)
}

pub const EDGE_TABLE: [u16; 256] = [
    0x0, 0x109, 0x203, 0x30a, 0x406, 0x50f, 0x605, 0x70c, 0x80c, 0x905, 0xa0f, 0xb06, 0xc0a, 0xd03,
    0xe09, 0xf00, 0x190, 0x99, 0x393, 0x29a, 0x596, 0x49f, 0x795, 0x69c, 0x99c, 0x895, 0xb9f,
    0xa96, 0xd9a, 0xc93, 0xf99, 0xe90, 0x230, 0x339, 0x33, 0x13a, 0x636, 0x73f, 0x435, 0x53c,
    0xa3c, 0xb35, 0x83f, 0x936, 0xe3a, 0xf33, 0xc39, 0xd30, 0x3a0, 0x2a9, 0x1a3, 0xaa, 0x7a6,
    0x6af, 0x5a5, 0x4ac, 0xbac, 0xaa5, 0x9af, 0x8a6, 0xfaa, 0xea3, 0xda9, 0xca0, 0x460, 0x569,
    0x663, 0x76a, 0x66, 0x16f, 0x265, 0x36c, 0xc6c, 0xd65, 0xe6f, 0xf66, 0x86a, 0x963, 0xa69,
    0xb60, 0x5f0, 0x4f9, 0x7f3, 0x6fa, 0x1f6, 0xff, 0x3f5, 0x2fc, 0xdfc, 0xcf5, 0xfff, 0xef6,
    0x9fa, 0x8f3, 0xbf9, 0xaf0, 0x650, 0x759, 0x453, 0x55a, 0x256, 0x35f, 0x55, 0x15c, 0xe5c,
    0xf55, 0xc5f, 0xd56, 0xa5a, 0xb53, 0x859, 0x950, 0x7c0, 0x6c9, 0x5c3, 0x4ca, 0x3c6, 0x2cf,
    0x1c5, 0xcc, 0xfcc, 0xec5, 0xdcf, 0xcc6, 0xbca, 0xac3, 0x9c9, 0x8c0, 0x8c0, 0x9c9, 0xac3,
    0xbca, 0xcc6, 0xdcf, 0xec5, 0xfcc, 0xcc, 0x1c5, 0x2cf, 0x3c6, 0x4ca, 0x5c3, 0x6c9, 0x7c0,
    0x950, 0x859, 0xb53, 0xa5a, 0xd56, 0xc5f, 0xf55, 0xe5c, 0x15c, 0x55, 0x35f, 0x256, 0x55a,
    0x453, 0x759, 0x650, 0xaf0, 0xbf9, 0x8f3, 0x9fa, 0xef6, 0xfff, 0xcf5, 0xdfc, 0x2fc, 0x3f5,
    0xff, 0x1f6, 0x6fa, 0x7f3, 0x4f9, 0x5f0, 0xb60, 0xa69, 0x963, 0x86a, 0xf66, 0xe6f, 0xd65,
    0xc6c, 0x36c, 0x265, 0x16f, 0x66, 0x76a, 0x663, 0x569, 0x460, 0xca0, 0xda9, 0xea3, 0xfaa,
    0x8a6, 0x9af, 0xaa5, 0xbac, 0x4ac, 0x5a5, 0x6af, 0x7a6, 0xaa, 0x1a3, 0x2a9, 0x3a0, 0xd30,
    0xc39, 0xf33, 0xe3a, 0x936, 0x83f, 0xb35, 0xa3c, 0x53c, 0x435, 0x73f, 0x636, 0x13a, 0x33,
    0x339, 0x230, 0xe90, 0xf99, 0xc93, 0xd9a, 0xa96, 0xb9f, 0x895, 0x99c, 0x69c, 0x795, 0x49f,
    0x596, 0x29a, 0x393, 0x99, 0x190, 0xf00, 0xe09, 0xd03, 0xc0a, 0xb06, 0xa0f, 0x905, 0x80c,
    0x70c, 0x605, 0x50f, 0x406, 0x30a, 0x203, 0x109, 0x0,
];

pub const TRI_TABLE: [[i8; 16]; 256] = [
    [
        -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    ],
    [0, 8, 3, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 1, 9, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [1, 8, 3, 9, 8, 1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [1, 2, 10, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 8, 3, 1, 2, 10, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [9, 2, 10, 0, 2, 9, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [2, 8, 3, 2, 10, 8, 10, 9, 8, -1, -1, -1, -1, -1, -1, -1],
    [3, 11, 2, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 11, 2, 8, 11, 0, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [1, 9, 0, 2, 3, 11, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [1, 11, 2, 1, 9, 11, 9, 8, 11, -1, -1, -1, -1, -1, -1, -1],
    [3, 10, 1, 11, 10, 3, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 10, 1, 0, 8, 10, 8, 11, 10, -1, -1, -1, -1, -1, -1, -1],
    [3, 9, 0, 3, 11, 9, 11, 10, 9, -1, -1, -1, -1, -1, -1, -1],
    [9, 8, 10, 10, 8, 11, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [4, 7, 8, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [4, 3, 0, 7, 3, 4, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 1, 9, 8, 4, 7, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [4, 1, 9, 4, 7, 1, 7, 3, 1, -1, -1, -1, -1, -1, -1, -1],
    [1, 2, 10, 8, 4, 7, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [3, 4, 7, 3, 0, 4, 1, 2, 10, -1, -1, -1, -1, -1, -1, -1],
    [9, 2, 10, 9, 0, 2, 8, 4, 7, -1, -1, -1, -1, -1, -1, -1],
    [2, 10, 9, 2, 9, 7, 2, 7, 3, 7, 9, 4, -1, -1, -1, -1],
    [8, 4, 7, 3, 11, 2, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [11, 4, 7, 11, 2, 4, 2, 0, 4, -1, -1, -1, -1, -1, -1, -1],
    [9, 0, 1, 8, 4, 7, 2, 3, 11, -1, -1, -1, -1, -1, -1, -1],
    [4, 7, 11, 9, 4, 11, 9, 11, 2, 9, 2, 1, -1, -1, -1, -1],
    [3, 10, 1, 3, 11, 10, 7, 8, 4, -1, -1, -1, -1, -1, -1, -1],
    [1, 11, 10, 1, 4, 11, 1, 0, 4, 7, 11, 4, -1, -1, -1, -1],
    [4, 7, 8, 9, 0, 11, 9, 11, 10, 11, 0, 3, -1, -1, -1, -1],
    [4, 7, 11, 4, 11, 9, 9, 11, 10, -1, -1, -1, -1, -1, -1, -1],
    [9, 5, 4, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [9, 5, 4, 0, 8, 3, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 5, 4, 1, 5, 0, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [8, 5, 4, 8, 3, 5, 3, 1, 5, -1, -1, -1, -1, -1, -1, -1],
    [1, 2, 10, 9, 5, 4, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [3, 0, 8, 1, 2, 10, 4, 9, 5, -1, -1, -1, -1, -1, -1, -1],
    [5, 2, 10, 5, 4, 2, 4, 0, 2, -1, -1, -1, -1, -1, -1, -1],
    [2, 10, 5, 3, 2, 5, 3, 5, 4, 3, 4, 8, -1, -1, -1, -1],
    [9, 5, 4, 2, 3, 11, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 11, 2, 0, 8, 11, 4, 9, 5, -1, -1, -1, -1, -1, -1, -1],
    [0, 5, 4, 0, 1, 5, 2, 3, 11, -1, -1, -1, -1, -1, -1, -1],
    [2, 1, 5, 2, 5, 8, 2, 8, 11, 4, 8, 5, -1, -1, -1, -1],
    [10, 3, 11, 10, 1, 3, 9, 5, 4, -1, -1, -1, -1, -1, -1, -1],
    [4, 9, 5, 0, 8, 1, 8, 10, 1, 8, 11, 10, -1, -1, -1, -1],
    [5, 4, 0, 5, 0, 11, 5, 11, 10, 11, 0, 3, -1, -1, -1, -1],
    [5, 4, 8, 5, 8, 10, 10, 8, 11, -1, -1, -1, -1, -1, -1, -1],
    [9, 7, 8, 5, 7, 9, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [9, 3, 0, 9, 5, 3, 5, 7, 3, -1, -1, -1, -1, -1, -1, -1],
    [0, 7, 8, 0, 1, 7, 1, 5, 7, -1, -1, -1, -1, -1, -1, -1],
    [1, 5, 3, 3, 5, 7, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [9, 7, 8, 9, 5, 7, 10, 1, 2, -1, -1, -1, -1, -1, -1, -1],
    [10, 1, 2, 9, 5, 0, 5, 3, 0, 5, 7, 3, -1, -1, -1, -1],
    [8, 0, 2, 8, 2, 5, 8, 5, 7, 10, 5, 2, -1, -1, -1, -1],
    [2, 10, 5, 2, 5, 3, 3, 5, 7, -1, -1, -1, -1, -1, -1, -1],
    [7, 9, 5, 7, 8, 9, 3, 11, 2, -1, -1, -1, -1, -1, -1, -1],
    [9, 5, 7, 9, 7, 2, 9, 2, 0, 2, 7, 11, -1, -1, -1, -1],
    [2, 3, 11, 0, 1, 8, 1, 7, 8, 1, 5, 7, -1, -1, -1, -1],
    [11, 2, 1, 11, 1, 7, 7, 1, 5, -1, -1, -1, -1, -1, -1, -1],
    [9, 5, 8, 8, 5, 7, 10, 1, 3, 10, 3, 11, -1, -1, -1, -1],
    [5, 7, 0, 5, 0, 9, 7, 11, 0, 1, 0, 10, 11, 10, 0, -1],
    [11, 10, 0, 11, 0, 3, 10, 5, 0, 8, 0, 7, 5, 7, 0, -1],
    [11, 10, 5, 7, 11, 5, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [10, 6, 5, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 8, 3, 5, 10, 6, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [9, 0, 1, 5, 10, 6, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [1, 8, 3, 1, 9, 8, 5, 10, 6, -1, -1, -1, -1, -1, -1, -1],
    [1, 6, 5, 2, 6, 1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [1, 6, 5, 1, 2, 6, 3, 0, 8, -1, -1, -1, -1, -1, -1, -1],
    [9, 6, 5, 9, 0, 6, 0, 2, 6, -1, -1, -1, -1, -1, -1, -1],
    [5, 9, 8, 5, 8, 2, 5, 2, 6, 3, 2, 8, -1, -1, -1, -1],
    [2, 3, 11, 10, 6, 5, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [11, 0, 8, 11, 2, 0, 10, 6, 5, -1, -1, -1, -1, -1, -1, -1],
    [0, 1, 9, 2, 3, 11, 5, 10, 6, -1, -1, -1, -1, -1, -1, -1],
    [5, 10, 6, 1, 9, 2, 9, 11, 2, 9, 8, 11, -1, -1, -1, -1],
    [6, 3, 11, 6, 5, 3, 5, 1, 3, -1, -1, -1, -1, -1, -1, -1],
    [0, 8, 11, 0, 11, 5, 0, 5, 1, 5, 11, 6, -1, -1, -1, -1],
    [3, 11, 6, 0, 3, 6, 0, 6, 5, 0, 5, 9, -1, -1, -1, -1],
    [6, 5, 9, 6, 9, 11, 11, 9, 8, -1, -1, -1, -1, -1, -1, -1],
    [5, 10, 6, 4, 7, 8, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [4, 3, 0, 4, 7, 3, 6, 5, 10, -1, -1, -1, -1, -1, -1, -1],
    [1, 9, 0, 5, 10, 6, 8, 4, 7, -1, -1, -1, -1, -1, -1, -1],
    [10, 6, 5, 1, 9, 7, 1, 7, 3, 7, 9, 4, -1, -1, -1, -1],
    [6, 1, 2, 6, 5, 1, 4, 7, 8, -1, -1, -1, -1, -1, -1, -1],
    [1, 2, 5, 5, 2, 6, 3, 0, 4, 3, 4, 7, -1, -1, -1, -1],
    [8, 4, 7, 9, 0, 5, 0, 6, 5, 0, 2, 6, -1, -1, -1, -1],
    [7, 3, 9, 7, 9, 4, 3, 2, 9, 5, 9, 6, 2, 6, 9, -1],
    [3, 11, 2, 7, 8, 4, 10, 6, 5, -1, -1, -1, -1, -1, -1, -1],
    [5, 10, 6, 4, 7, 2, 4, 2, 0, 2, 7, 11, -1, -1, -1, -1],
    [0, 1, 9, 4, 7, 8, 2, 3, 11, 5, 10, 6, -1, -1, -1, -1],
    [9, 2, 1, 9, 11, 2, 9, 4, 11, 7, 11, 4, 5, 10, 6, -1],
    [8, 4, 7, 3, 11, 5, 3, 5, 1, 5, 11, 6, -1, -1, -1, -1],
    [5, 1, 11, 5, 11, 6, 1, 0, 11, 7, 11, 4, 0, 4, 11, -1],
    [0, 5, 9, 0, 6, 5, 0, 3, 6, 11, 6, 3, 8, 4, 7, -1],
    [6, 5, 9, 6, 9, 11, 4, 7, 9, 7, 11, 9, -1, -1, -1, -1],
    [10, 4, 9, 6, 4, 10, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [4, 10, 6, 4, 9, 10, 0, 8, 3, -1, -1, -1, -1, -1, -1, -1],
    [10, 0, 1, 10, 6, 0, 6, 4, 0, -1, -1, -1, -1, -1, -1, -1],
    [8, 3, 1, 8, 1, 6, 8, 6, 4, 6, 1, 10, -1, -1, -1, -1],
    [1, 4, 9, 1, 2, 4, 2, 6, 4, -1, -1, -1, -1, -1, -1, -1],
    [3, 0, 8, 1, 2, 9, 2, 4, 9, 2, 6, 4, -1, -1, -1, -1],
    [0, 2, 4, 4, 2, 6, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [8, 3, 2, 8, 2, 4, 4, 2, 6, -1, -1, -1, -1, -1, -1, -1],
    [10, 4, 9, 10, 6, 4, 11, 2, 3, -1, -1, -1, -1, -1, -1, -1],
    [0, 8, 2, 2, 8, 11, 4, 9, 10, 4, 10, 6, -1, -1, -1, -1],
    [3, 11, 2, 0, 1, 6, 0, 6, 4, 6, 1, 10, -1, -1, -1, -1],
    [6, 4, 1, 6, 1, 10, 4, 8, 1, 2, 1, 11, 8, 11, 1, -1],
    [9, 6, 4, 9, 3, 6, 9, 1, 3, 11, 6, 3, -1, -1, -1, -1],
    [8, 11, 1, 8, 1, 0, 11, 6, 1, 9, 1, 4, 6, 4, 1, -1],
    [3, 11, 6, 3, 6, 0, 0, 6, 4, -1, -1, -1, -1, -1, -1, -1],
    [6, 4, 8, 11, 6, 8, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [7, 10, 6, 7, 8, 10, 8, 9, 10, -1, -1, -1, -1, -1, -1, -1],
    [0, 7, 3, 0, 10, 7, 0, 9, 10, 6, 7, 10, -1, -1, -1, -1],
    [10, 6, 7, 1, 10, 7, 1, 7, 8, 1, 8, 0, -1, -1, -1, -1],
    [10, 6, 7, 10, 7, 1, 1, 7, 3, -1, -1, -1, -1, -1, -1, -1],
    [1, 2, 6, 1, 6, 8, 1, 8, 9, 8, 6, 7, -1, -1, -1, -1],
    [2, 6, 9, 2, 9, 1, 6, 7, 9, 0, 9, 3, 7, 3, 9, -1],
    [7, 8, 0, 7, 0, 6, 6, 0, 2, -1, -1, -1, -1, -1, -1, -1],
    [7, 3, 2, 6, 7, 2, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [2, 3, 11, 10, 6, 8, 10, 8, 9, 8, 6, 7, -1, -1, -1, -1],
    [2, 0, 7, 2, 7, 11, 0, 9, 7, 6, 7, 10, 9, 10, 7, -1],
    [1, 8, 0, 1, 7, 8, 1, 10, 7, 6, 7, 10, 2, 3, 11, -1],
    [11, 2, 1, 11, 1, 7, 10, 6, 1, 6, 7, 1, -1, -1, -1, -1],
    [8, 9, 6, 8, 6, 7, 9, 1, 6, 11, 6, 3, 1, 3, 6, -1],
    [0, 9, 1, 11, 6, 7, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [7, 8, 0, 7, 0, 6, 3, 11, 0, 11, 6, 0, -1, -1, -1, -1],
    [7, 11, 6, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [7, 6, 11, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [3, 0, 8, 11, 7, 6, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 1, 9, 11, 7, 6, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [8, 1, 9, 8, 3, 1, 11, 7, 6, -1, -1, -1, -1, -1, -1, -1],
    [10, 1, 2, 6, 11, 7, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [1, 2, 10, 3, 0, 8, 6, 11, 7, -1, -1, -1, -1, -1, -1, -1],
    [2, 9, 0, 2, 10, 9, 6, 11, 7, -1, -1, -1, -1, -1, -1, -1],
    [6, 11, 7, 2, 10, 3, 10, 8, 3, 10, 9, 8, -1, -1, -1, -1],
    [7, 2, 3, 6, 2, 7, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [7, 0, 8, 7, 6, 0, 6, 2, 0, -1, -1, -1, -1, -1, -1, -1],
    [2, 7, 6, 2, 3, 7, 0, 1, 9, -1, -1, -1, -1, -1, -1, -1],
    [1, 6, 2, 1, 8, 6, 1, 9, 8, 8, 7, 6, -1, -1, -1, -1],
    [10, 7, 6, 10, 1, 7, 1, 3, 7, -1, -1, -1, -1, -1, -1, -1],
    [10, 7, 6, 1, 7, 10, 1, 8, 7, 1, 0, 8, -1, -1, -1, -1],
    [0, 3, 7, 0, 7, 10, 0, 10, 9, 6, 10, 7, -1, -1, -1, -1],
    [7, 6, 10, 7, 10, 8, 8, 10, 9, -1, -1, -1, -1, -1, -1, -1],
    [6, 8, 4, 11, 8, 6, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [3, 6, 11, 3, 0, 6, 0, 4, 6, -1, -1, -1, -1, -1, -1, -1],
    [8, 6, 11, 8, 4, 6, 9, 0, 1, -1, -1, -1, -1, -1, -1, -1],
    [9, 4, 6, 9, 6, 3, 9, 3, 1, 11, 3, 6, -1, -1, -1, -1],
    [6, 8, 4, 6, 11, 8, 2, 10, 1, -1, -1, -1, -1, -1, -1, -1],
    [1, 2, 10, 3, 0, 11, 0, 6, 11, 0, 4, 6, -1, -1, -1, -1],
    [4, 11, 8, 4, 6, 11, 0, 2, 9, 2, 10, 9, -1, -1, -1, -1],
    [10, 9, 3, 10, 3, 2, 9, 4, 3, 11, 3, 6, 4, 6, 3, -1],
    [8, 2, 3, 8, 4, 2, 4, 6, 2, -1, -1, -1, -1, -1, -1, -1],
    [0, 4, 2, 4, 6, 2, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [1, 9, 0, 2, 3, 4, 2, 4, 6, 4, 3, 8, -1, -1, -1, -1],
    [1, 9, 4, 1, 4, 2, 2, 4, 6, -1, -1, -1, -1, -1, -1, -1],
    [8, 1, 3, 8, 6, 1, 8, 4, 6, 6, 10, 1, -1, -1, -1, -1],
    [10, 1, 0, 10, 0, 6, 6, 0, 4, -1, -1, -1, -1, -1, -1, -1],
    [4, 6, 3, 4, 3, 8, 6, 10, 3, 0, 3, 9, 10, 9, 3, -1],
    [10, 9, 4, 6, 10, 4, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [4, 9, 5, 7, 6, 11, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 8, 3, 4, 9, 5, 11, 7, 6, -1, -1, -1, -1, -1, -1, -1],
    [5, 0, 1, 5, 4, 0, 7, 6, 11, -1, -1, -1, -1, -1, -1, -1],
    [11, 7, 6, 8, 3, 4, 3, 5, 4, 3, 1, 5, -1, -1, -1, -1],
    [9, 5, 4, 10, 1, 2, 7, 6, 11, -1, -1, -1, -1, -1, -1, -1],
    [6, 11, 7, 1, 2, 10, 0, 8, 3, 4, 9, 5, -1, -1, -1, -1],
    [7, 6, 11, 5, 4, 10, 4, 2, 10, 4, 0, 2, -1, -1, -1, -1],
    [3, 4, 8, 3, 5, 4, 3, 2, 5, 10, 5, 2, 11, 7, 6, -1],
    [7, 2, 3, 7, 6, 2, 5, 4, 9, -1, -1, -1, -1, -1, -1, -1],
    [9, 5, 4, 0, 8, 6, 0, 6, 2, 6, 8, 7, -1, -1, -1, -1],
    [3, 6, 2, 3, 7, 6, 1, 5, 0, 5, 4, 0, -1, -1, -1, -1],
    [6, 2, 8, 6, 8, 7, 2, 1, 8, 4, 8, 5, 1, 5, 8, -1],
    [9, 5, 4, 10, 1, 6, 1, 7, 6, 1, 3, 7, -1, -1, -1, -1],
    [1, 6, 10, 1, 7, 6, 1, 0, 7, 8, 7, 0, 9, 5, 4, -1],
    [4, 0, 10, 4, 10, 5, 0, 3, 10, 6, 10, 7, 3, 7, 10, -1],
    [7, 6, 10, 7, 10, 8, 5, 4, 10, 4, 8, 10, -1, -1, -1, -1],
    [6, 9, 5, 6, 11, 9, 11, 8, 9, -1, -1, -1, -1, -1, -1, -1],
    [3, 6, 11, 0, 6, 3, 0, 5, 6, 0, 9, 5, -1, -1, -1, -1],
    [0, 11, 8, 0, 5, 11, 0, 1, 5, 5, 6, 11, -1, -1, -1, -1],
    [6, 11, 3, 6, 3, 5, 5, 3, 1, -1, -1, -1, -1, -1, -1, -1],
    [1, 2, 10, 9, 5, 11, 9, 11, 8, 11, 5, 6, -1, -1, -1, -1],
    [0, 11, 3, 0, 6, 11, 0, 9, 6, 5, 6, 9, 1, 2, 10, -1],
    [11, 8, 5, 11, 5, 6, 8, 0, 5, 10, 5, 2, 0, 2, 5, -1],
    [6, 11, 3, 6, 3, 5, 2, 10, 3, 10, 5, 3, -1, -1, -1, -1],
    [5, 8, 9, 5, 2, 8, 5, 6, 2, 3, 8, 2, -1, -1, -1, -1],
    [9, 5, 6, 9, 6, 0, 0, 6, 2, -1, -1, -1, -1, -1, -1, -1],
    [1, 5, 8, 1, 8, 0, 5, 6, 8, 3, 8, 2, 6, 2, 8, -1],
    [1, 5, 6, 2, 1, 6, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [1, 3, 6, 1, 6, 10, 3, 8, 6, 5, 6, 9, 8, 9, 6, -1],
    [10, 1, 0, 10, 0, 6, 9, 5, 0, 5, 6, 0, -1, -1, -1, -1],
    [0, 3, 8, 5, 6, 10, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [10, 5, 6, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [11, 5, 10, 7, 5, 11, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [11, 5, 10, 11, 7, 5, 8, 3, 0, -1, -1, -1, -1, -1, -1, -1],
    [5, 11, 7, 5, 10, 11, 1, 9, 0, -1, -1, -1, -1, -1, -1, -1],
    [10, 7, 5, 10, 11, 7, 9, 8, 1, 8, 3, 1, -1, -1, -1, -1],
    [11, 1, 2, 11, 7, 1, 7, 5, 1, -1, -1, -1, -1, -1, -1, -1],
    [0, 8, 3, 1, 2, 7, 1, 7, 5, 7, 2, 11, -1, -1, -1, -1],
    [9, 7, 5, 9, 2, 7, 9, 0, 2, 2, 11, 7, -1, -1, -1, -1],
    [7, 5, 2, 7, 2, 11, 5, 9, 2, 3, 2, 8, 9, 8, 2, -1],
    [2, 5, 10, 2, 3, 5, 3, 7, 5, -1, -1, -1, -1, -1, -1, -1],
    [8, 2, 0, 8, 5, 2, 8, 7, 5, 10, 2, 5, -1, -1, -1, -1],
    [9, 0, 1, 5, 10, 3, 5, 3, 7, 3, 10, 2, -1, -1, -1, -1],
    [9, 8, 2, 9, 2, 1, 8, 7, 2, 10, 2, 5, 7, 5, 2, -1],
    [1, 3, 5, 3, 7, 5, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 8, 7, 0, 7, 1, 1, 7, 5, -1, -1, -1, -1, -1, -1, -1],
    [9, 0, 3, 9, 3, 5, 5, 3, 7, -1, -1, -1, -1, -1, -1, -1],
    [9, 8, 7, 5, 9, 7, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [5, 8, 4, 5, 10, 8, 10, 11, 8, -1, -1, -1, -1, -1, -1, -1],
    [5, 0, 4, 5, 11, 0, 5, 10, 11, 11, 3, 0, -1, -1, -1, -1],
    [0, 1, 9, 8, 4, 10, 8, 10, 11, 10, 4, 5, -1, -1, -1, -1],
    [10, 11, 4, 10, 4, 5, 11, 3, 4, 9, 4, 1, 3, 1, 4, -1],
    [2, 5, 1, 2, 8, 5, 2, 11, 8, 4, 5, 8, -1, -1, -1, -1],
    [0, 4, 11, 0, 11, 3, 4, 5, 11, 2, 11, 1, 5, 1, 11, -1],
    [0, 2, 5, 0, 5, 9, 2, 11, 5, 4, 5, 8, 11, 8, 5, -1],
    [9, 4, 5, 2, 11, 3, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [2, 5, 10, 3, 5, 2, 3, 4, 5, 3, 8, 4, -1, -1, -1, -1],
    [5, 10, 2, 5, 2, 4, 4, 2, 0, -1, -1, -1, -1, -1, -1, -1],
    [3, 10, 2, 3, 5, 10, 3, 8, 5, 4, 5, 8, 0, 1, 9, -1],
    [5, 10, 2, 5, 2, 4, 1, 9, 2, 9, 4, 2, -1, -1, -1, -1],
    [8, 4, 5, 8, 5, 3, 3, 5, 1, -1, -1, -1, -1, -1, -1, -1],
    [0, 4, 5, 1, 0, 5, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [8, 4, 5, 8, 5, 3, 9, 0, 5, 0, 3, 5, -1, -1, -1, -1],
    [9, 4, 5, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [4, 11, 7, 4, 9, 11, 9, 10, 11, -1, -1, -1, -1, -1, -1, -1],
    [0, 8, 3, 4, 9, 7, 9, 11, 7, 9, 10, 11, -1, -1, -1, -1],
    [1, 10, 11, 1, 11, 4, 1, 4, 0, 7, 4, 11, -1, -1, -1, -1],
    [3, 1, 4, 3, 4, 8, 1, 10, 4, 7, 4, 11, 10, 11, 4, -1],
    [4, 11, 7, 9, 11, 4, 9, 2, 11, 9, 1, 2, -1, -1, -1, -1],
    [9, 7, 4, 9, 11, 7, 9, 1, 11, 2, 11, 1, 0, 8, 3, -1],
    [11, 7, 4, 11, 4, 2, 2, 4, 0, -1, -1, -1, -1, -1, -1, -1],
    [11, 7, 4, 11, 4, 2, 8, 3, 4, 3, 2, 4, -1, -1, -1, -1],
    [2, 9, 10, 2, 7, 9, 2, 3, 7, 7, 4, 9, -1, -1, -1, -1],
    [9, 10, 7, 9, 7, 4, 10, 2, 7, 8, 7, 0, 2, 0, 7, -1],
    [3, 7, 10, 3, 10, 2, 7, 4, 10, 1, 10, 0, 4, 0, 10, -1],
    [1, 10, 2, 8, 7, 4, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [4, 9, 1, 4, 1, 7, 7, 1, 3, -1, -1, -1, -1, -1, -1, -1],
    [4, 9, 1, 4, 1, 7, 0, 8, 1, 8, 7, 1, -1, -1, -1, -1],
    [4, 0, 3, 7, 4, 3, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [4, 8, 7, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [9, 10, 8, 10, 11, 8, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [3, 0, 9, 3, 9, 11, 11, 9, 10, -1, -1, -1, -1, -1, -1, -1],
    [0, 1, 10, 0, 10, 8, 8, 10, 11, -1, -1, -1, -1, -1, -1, -1],
    [3, 1, 10, 11, 3, 10, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [1, 2, 11, 1, 11, 9, 9, 11, 8, -1, -1, -1, -1, -1, -1, -1],
    [3, 0, 9, 3, 9, 11, 1, 2, 9, 2, 11, 9, -1, -1, -1, -1],
    [0, 2, 11, 8, 0, 11, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [3, 2, 11, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [2, 3, 8, 2, 8, 10, 10, 8, 9, -1, -1, -1, -1, -1, -1, -1],
    [9, 10, 2, 0, 9, 2, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [2, 3, 8, 2, 8, 10, 0, 1, 8, 1, 10, 8, -1, -1, -1, -1],
    [1, 10, 2, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [1, 3, 8, 9, 1, 8, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 9, 1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 3, 8, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [
        -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    ],
];
