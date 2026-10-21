//! 空間分割構造（BVH）による原子の近傍探索とペア検出モジュール。
//!
//! 生体高分子の非一様な幾何形状に適応し、AABB（Axis-Aligned Bounding Box）間の
//! 最短距離計算に基づく二重木走査（dual-tree traversal）により、平均 O(n log n) で
//! 結合検出用の近接原子ペアや半径近傍を探索します。
//! 将来の Compute Shader カリング（フェーズ5b-ii）における視錐台枝刈りとも整合します。

use serde::{Deserialize, Serialize};

/// 3次元軸平行バウンディングボックス（AABB）
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Aabb {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Aabb {
    /// 無効（反転した）初期値を持つ空の AABB を作成する。
    pub fn empty() -> Self {
        Self {
            min: [f32::INFINITY, f32::INFINITY, f32::INFINITY],
            max: [f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY],
        }
    }

    /// 単一点から AABB を作成する。
    pub fn from_point(p: [f32; 3]) -> Self {
        Self { min: p, max: p }
    }

    /// 球（中心と半径）から AABB を作成する。
    pub fn from_sphere(center: [f32; 3], radius: f32) -> Self {
        Self {
            min: [center[0] - radius, center[1] - radius, center[2] - radius],
            max: [center[0] + radius, center[1] + radius, center[2] + radius],
        }
    }

    /// 点を取り込んで AABB を拡張する。
    pub fn union_point(&mut self, p: [f32; 3]) {
        self.min[0] = self.min[0].min(p[0]);
        self.min[1] = self.min[1].min(p[1]);
        self.min[2] = self.min[2].min(p[2]);

        self.max[0] = self.max[0].max(p[0]);
        self.max[1] = self.max[1].max(p[1]);
        self.max[2] = self.max[2].max(p[2]);
    }

    /// 別の AABB を取り込んで拡張する。
    pub fn union_aabb(&mut self, other: &Aabb) {
        self.min[0] = self.min[0].min(other.min[0]);
        self.min[1] = self.min[1].min(other.min[1]);
        self.min[2] = self.min[2].min(other.min[2]);

        self.max[0] = self.max[0].max(other.max[0]);
        self.max[1] = self.max[1].max(other.max[1]);
        self.max[2] = self.max[2].max(other.max[2]);
    }

    /// 中心座標を計算する。
    pub fn center(&self) -> [f32; 3] {
        [
            (self.min[0] + self.max[0]) * 0.5,
            (self.min[1] + self.max[1]) * 0.5,
            (self.min[2] + self.max[2]) * 0.5,
        ]
    }

    /// 各軸の長さ（extent）を計算する。
    pub fn extents(&self) -> [f32; 3] {
        [
            (self.max[0] - self.min[0]).max(0.0),
            (self.max[1] - self.min[1]).max(0.0),
            (self.max[2] - self.min[2]).max(0.0),
        ]
    }

    /// 最も長い軸（0: X, 1: Y, 2: Z）を返す。
    pub fn longest_axis(&self) -> usize {
        let e = self.extents();
        if e[0] >= e[1] && e[0] >= e[2] {
            0
        } else if e[1] >= e[2] {
            1
        } else {
            2
        }
    }

    /// 点 `p` との最短距離の2乗を計算する（点が内部にあれば 0.0）。
    pub fn distance_sq_to_point(&self, p: [f32; 3]) -> f32 {
        let mut dist_sq = 0.0;
        for (i, &v) in p.iter().enumerate() {
            if v < self.min[i] {
                let d = self.min[i] - v;
                dist_sq += d * d;
            } else if v > self.max[i] {
                let d = v - self.max[i];
                dist_sq += d * d;
            }
        }
        dist_sq
    }

    /// 別の AABB との最短距離の2乗を計算する（交差または接触していれば 0.0）。
    pub fn distance_sq_to_aabb(&self, other: &Aabb) -> f32 {
        let mut dist_sq = 0.0;
        for i in 0..3 {
            let d = (self.min[i] - other.max[i]).max(other.min[i] - self.max[i]);
            if d > 0.0 {
                dist_sq += d * d;
            }
        }
        dist_sq
    }

    /// レイ（始点 `origin`, 方向逆数 `dir_inv`）と、各軸 `margin` だけ拡張した AABB との交差判定を行う（Slab法）。
    /// 交差する場合、有効な最近交点距離（`t >= 0.0`）を返す。
    pub fn intersect_ray_with_margin(
        &self,
        origin: [f32; 3],
        dir_inv: [f32; 3],
        margin: f32,
        max_t: f32,
    ) -> Option<f32> {
        let mut t_min = 0.0f32;
        let mut t_max = max_t;

        for i in 0..3 {
            let min_val = self.min[i] - margin;
            let max_val = self.max[i] + margin;

            let t1 = (min_val - origin[i]) * dir_inv[i];
            let t2 = (max_val - origin[i]) * dir_inv[i];

            let (near, far) = if dir_inv[i] >= 0.0 {
                (t1, t2)
            } else {
                (t2, t1)
            };

            t_min = t_min.max(near);
            t_max = t_max.min(far);

            if t_min > t_max {
                return None;
            }
        }

        Some(t_min.max(0.0))
    }
}

/// BVHのノード構造
#[derive(Debug, Clone)]
enum BvhNode {
    Leaf {
        aabb: Aabb,
        indices: Vec<usize>,
    },
    Internal {
        aabb: Aabb,
        left: Box<BvhNode>,
        right: Box<BvhNode>,
    },
}

impl BvhNode {
    fn aabb(&self) -> &Aabb {
        match self {
            BvhNode::Leaf { aabb, .. } => aabb,
            BvhNode::Internal { aabb, .. } => aabb,
        }
    }
}

/// 原子の3次元座標集合に対して構築されるBVH（境界ボリューム階層）
pub struct AtomBvh {
    root: Option<BvhNode>,
    positions: Vec<[f32; 3]>,
}

const LEAF_MAX_ATOMS: usize = 8;

impl AtomBvh {
    /// 原子の3次元座標スライスから BVH を構築する。
    pub fn build(positions: &[[f32; 3]]) -> Self {
        let mut indices: Vec<usize> = (0..positions.len()).collect();
        let root = if positions.is_empty() {
            None
        } else {
            Some(Self::build_recursive(&mut indices, positions))
        };

        Self {
            root,
            positions: positions.to_vec(),
        }
    }

    fn build_recursive(indices: &mut [usize], positions: &[[f32; 3]]) -> BvhNode {
        let mut aabb = Aabb::empty();
        for &idx in indices.iter() {
            aabb.union_point(positions[idx]);
        }

        if indices.len() <= LEAF_MAX_ATOMS {
            return BvhNode::Leaf {
                aabb,
                indices: indices.to_vec(),
            };
        }

        let axis = aabb.longest_axis();

        // 最長軸の座標でソートし、中央値で分割
        indices.sort_unstable_by(|&a, &b| {
            positions[a][axis]
                .partial_cmp(&positions[b][axis])
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mid = indices.len() / 2;
        let (left_indices, right_indices) = indices.split_at_mut(mid);

        let left = Box::new(Self::build_recursive(left_indices, positions));
        let right = Box::new(Self::build_recursive(right_indices, positions));

        BvhNode::Internal { aabb, left, right }
    }

    /// 原子の総数を取得する。
    pub fn len(&self) -> usize {
        self.positions.len()
    }

    /// BVHが空かどうかを取得する。
    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    /// 単一点 `target` を中心とする半径 `radius` 以内の原子インデックスを列挙する。
    pub fn query_radius(&self, target: [f32; 3], radius: f32) -> Vec<usize> {
        let mut results = Vec::new();
        if let Some(root) = &self.root {
            let radius_sq = radius * radius;
            Self::query_radius_node(root, target, radius_sq, &self.positions, &mut results);
        }
        results
    }

    /// 単一点 `target` に最も近い原子のインデックスと最短距離の2乗 `(index, dist_sq)` を探索する。
    pub fn query_nearest(&self, target: [f32; 3]) -> Option<(usize, f32)> {
        let root = self.root.as_ref()?;
        let mut best_idx = None;
        let mut best_dist_sq = f32::INFINITY;
        Self::query_nearest_node(
            root,
            target,
            &self.positions,
            &mut best_idx,
            &mut best_dist_sq,
        );
        best_idx.map(|idx| (idx, best_dist_sq))
    }

    fn query_nearest_node(
        node: &BvhNode,
        target: [f32; 3],
        positions: &[[f32; 3]],
        best_idx: &mut Option<usize>,
        best_dist_sq: &mut f32,
    ) {
        if node.aabb().distance_sq_to_point(target) >= *best_dist_sq {
            return;
        }

        match node {
            BvhNode::Leaf { indices, .. } => {
                for &idx in indices {
                    let p = positions[idx];
                    let dx = p[0] - target[0];
                    let dy = p[1] - target[1];
                    let dz = p[2] - target[2];
                    let dist_sq = dx * dx + dy * dy + dz * dz;
                    if dist_sq < *best_dist_sq {
                        *best_dist_sq = dist_sq;
                        *best_idx = Some(idx);
                    }
                }
            }
            BvhNode::Internal { left, right, .. } => {
                let d_left = left.aabb().distance_sq_to_point(target);
                let d_right = right.aabb().distance_sq_to_point(target);
                if d_left < d_right {
                    if d_left < *best_dist_sq {
                        Self::query_nearest_node(left, target, positions, best_idx, best_dist_sq);
                    }
                    if d_right < *best_dist_sq {
                        Self::query_nearest_node(right, target, positions, best_idx, best_dist_sq);
                    }
                } else {
                    if d_right < *best_dist_sq {
                        Self::query_nearest_node(right, target, positions, best_idx, best_dist_sq);
                    }
                    if d_left < *best_dist_sq {
                        Self::query_nearest_node(left, target, positions, best_idx, best_dist_sq);
                    }
                }
            }
        }
    }

    fn query_radius_node(
        node: &BvhNode,
        target: [f32; 3],
        radius_sq: f32,
        positions: &[[f32; 3]],
        results: &mut Vec<usize>,
    ) {
        if node.aabb().distance_sq_to_point(target) > radius_sq {
            return;
        }

        match node {
            BvhNode::Leaf { indices, .. } => {
                for &idx in indices {
                    let p = positions[idx];
                    let dx = p[0] - target[0];
                    let dy = p[1] - target[1];
                    let dz = p[2] - target[2];
                    let dist_sq = dx * dx + dy * dy + dz * dz;
                    if dist_sq <= radius_sq {
                        results.push(idx);
                    }
                }
            }
            BvhNode::Internal { left, right, .. } => {
                Self::query_radius_node(left, target, radius_sq, positions, results);
                Self::query_radius_node(right, target, radius_sq, positions, results);
            }
        }
    }

    /// 全原子ペアのうち、距離が `min_dist <= dist <= max_dist` の範囲にある
    /// インデックスペア `(i, j)` （常に `i < j`）を二重木走査（dual-tree traversal）により高速に列挙する。
    pub fn query_pairs_within_range(&self, min_dist: f32, max_dist: f32) -> Vec<(usize, usize)> {
        let mut results = Vec::new();
        if let Some(root) = &self.root {
            let min_dist_sq = min_dist * min_dist;
            let max_dist_sq = max_dist * max_dist;

            // 木自身の自己ペア走査を開始
            Self::query_pairs_self(
                root,
                min_dist_sq,
                max_dist_sq,
                &self.positions,
                &mut results,
            );
        }
        results
    }

    fn query_pairs_self(
        node: &BvhNode,
        min_dist_sq: f32,
        max_dist_sq: f32,
        positions: &[[f32; 3]],
        results: &mut Vec<(usize, usize)>,
    ) {
        match node {
            BvhNode::Leaf { indices, .. } => {
                for i in 0..indices.len() {
                    let a_idx = indices[i];
                    let pa = positions[a_idx];
                    for &b_idx in indices.iter().skip(i + 1) {
                        let pb = positions[b_idx];

                        let dx = pa[0] - pb[0];
                        let dy = pa[1] - pb[1];
                        let dz = pa[2] - pb[2];
                        let dist_sq = dx * dx + dy * dy + dz * dz;

                        if dist_sq >= min_dist_sq && dist_sq <= max_dist_sq {
                            let pair = if a_idx < b_idx {
                                (a_idx, b_idx)
                            } else {
                                (b_idx, a_idx)
                            };
                            results.push(pair);
                        }
                    }
                }
            }
            BvhNode::Internal { left, right, .. } => {
                // 左ノード内の自己ペア
                Self::query_pairs_self(left, min_dist_sq, max_dist_sq, positions, results);
                // 右ノード内の自己ペア
                Self::query_pairs_self(right, min_dist_sq, max_dist_sq, positions, results);
                // 左右ノード間の交差ペア
                Self::query_pairs_cross(left, right, min_dist_sq, max_dist_sq, positions, results);
            }
        }
    }

    fn query_pairs_cross(
        node_a: &BvhNode,
        node_b: &BvhNode,
        min_dist_sq: f32,
        max_dist_sq: f32,
        positions: &[[f32; 3]],
        results: &mut Vec<(usize, usize)>,
    ) {
        // AABB間の最短距離が探索最大距離を超えていれば枝刈り
        if node_a.aabb().distance_sq_to_aabb(node_b.aabb()) > max_dist_sq {
            return;
        }

        match (node_a, node_b) {
            (
                BvhNode::Leaf {
                    indices: a_indices, ..
                },
                BvhNode::Leaf {
                    indices: b_indices, ..
                },
            ) => {
                for &a_idx in a_indices {
                    let pa = positions[a_idx];
                    for &b_idx in b_indices {
                        if a_idx == b_idx {
                            continue;
                        }

                        let pb = positions[b_idx];
                        let dx = pa[0] - pb[0];
                        let dy = pa[1] - pb[1];
                        let dz = pa[2] - pb[2];
                        let dist_sq = dx * dx + dy * dy + dz * dz;

                        if dist_sq >= min_dist_sq && dist_sq <= max_dist_sq {
                            let pair = if a_idx < b_idx {
                                (a_idx, b_idx)
                            } else {
                                (b_idx, a_idx)
                            };
                            results.push(pair);
                        }
                    }
                }
            }
            (
                BvhNode::Leaf { .. },
                BvhNode::Internal {
                    left: b_left,
                    right: b_right,
                    ..
                },
            ) => {
                Self::query_pairs_cross(
                    node_a,
                    b_left,
                    min_dist_sq,
                    max_dist_sq,
                    positions,
                    results,
                );
                Self::query_pairs_cross(
                    node_a,
                    b_right,
                    min_dist_sq,
                    max_dist_sq,
                    positions,
                    results,
                );
            }
            (
                BvhNode::Internal {
                    left: a_left,
                    right: a_right,
                    ..
                },
                BvhNode::Leaf { .. },
            ) => {
                Self::query_pairs_cross(
                    a_left,
                    node_b,
                    min_dist_sq,
                    max_dist_sq,
                    positions,
                    results,
                );
                Self::query_pairs_cross(
                    a_right,
                    node_b,
                    min_dist_sq,
                    max_dist_sq,
                    positions,
                    results,
                );
            }
            (
                BvhNode::Internal {
                    left: a_left,
                    right: a_right,
                    aabb: aabb_a,
                },
                BvhNode::Internal {
                    left: b_left,
                    right: b_right,
                    aabb: aabb_b,
                },
            ) => {
                // より大きい AABB を持つ側を優先して分割
                let vol_a = {
                    let e = aabb_a.extents();
                    e[0] * e[1] * e[2]
                };
                let vol_b = {
                    let e = aabb_b.extents();
                    e[0] * e[1] * e[2]
                };

                if vol_a >= vol_b {
                    Self::query_pairs_cross(
                        a_left,
                        node_b,
                        min_dist_sq,
                        max_dist_sq,
                        positions,
                        results,
                    );
                    Self::query_pairs_cross(
                        a_right,
                        node_b,
                        min_dist_sq,
                        max_dist_sq,
                        positions,
                        results,
                    );
                } else {
                    Self::query_pairs_cross(
                        node_a,
                        b_left,
                        min_dist_sq,
                        max_dist_sq,
                        positions,
                        results,
                    );
                    Self::query_pairs_cross(
                        node_a,
                        b_right,
                        min_dist_sq,
                        max_dist_sq,
                        positions,
                        results,
                    );
                }
            }
        }
    }

    /// レイ（始点 `origin`, 正規化方向 `direction`）と最も手前で交差する原子を探索する。
    /// 各原子の判定半径として `radii` が与えられた場合はそれを参照し、
    /// `None` または要素数が不足する場合は既定の半径 `default_radius`（Å）を使用する。
    /// 戻り値は `Option<(atom_index, distance_t)>`。
    pub fn query_ray(
        &self,
        origin: [f32; 3],
        direction: [f32; 3],
        radii: Option<&[f32]>,
        default_radius: f32,
    ) -> Option<(usize, f32)> {
        let root = self.root.as_ref()?;
        let dir_inv = [
            if direction[0].abs() > 1e-9 {
                1.0 / direction[0]
            } else {
                f32::INFINITY
            },
            if direction[1].abs() > 1e-9 {
                1.0 / direction[1]
            } else {
                f32::INFINITY
            },
            if direction[2].abs() > 1e-9 {
                1.0 / direction[2]
            } else {
                f32::INFINITY
            },
        ];

        let max_margin = radii
            .map(|r| r.iter().copied().fold(default_radius, f32::max))
            .unwrap_or(default_radius);

        let mut best_hit = None;
        let mut best_t = f32::INFINITY;

        Self::query_ray_node(
            root,
            origin,
            direction,
            dir_inv,
            &self.positions,
            radii,
            default_radius,
            max_margin,
            &mut best_t,
            &mut best_hit,
        );

        best_hit.map(|idx| (idx, best_t))
    }

    #[allow(clippy::too_many_arguments)]
    fn query_ray_node(
        node: &BvhNode,
        origin: [f32; 3],
        direction: [f32; 3],
        dir_inv: [f32; 3],
        positions: &[[f32; 3]],
        radii: Option<&[f32]>,
        default_radius: f32,
        margin: f32,
        best_t: &mut f32,
        best_hit: &mut Option<usize>,
    ) {
        let Some(t_box) = node
            .aabb()
            .intersect_ray_with_margin(origin, dir_inv, margin, *best_t)
        else {
            return;
        };

        if t_box >= *best_t {
            return;
        }

        match node {
            BvhNode::Leaf { indices, .. } => {
                for &idx in indices {
                    let center = positions[idx];
                    let radius = radii
                        .and_then(|r| r.get(idx))
                        .copied()
                        .unwrap_or(default_radius);

                    if let Some(t) = intersect_ray_sphere(origin, direction, center, radius)
                        && t < *best_t
                    {
                        *best_t = t;
                        *best_hit = Some(idx);
                    }
                }
            }
            BvhNode::Internal { left, right, .. } => {
                let left_t = left
                    .aabb()
                    .intersect_ray_with_margin(origin, dir_inv, margin, *best_t);
                let right_t = right
                    .aabb()
                    .intersect_ray_with_margin(origin, dir_inv, margin, *best_t);

                match (left_t, right_t) {
                    (Some(tl), Some(tr)) => {
                        if tl <= tr {
                            Self::query_ray_node(
                                left,
                                origin,
                                direction,
                                dir_inv,
                                positions,
                                radii,
                                default_radius,
                                margin,
                                best_t,
                                best_hit,
                            );
                            Self::query_ray_node(
                                right,
                                origin,
                                direction,
                                dir_inv,
                                positions,
                                radii,
                                default_radius,
                                margin,
                                best_t,
                                best_hit,
                            );
                        } else {
                            Self::query_ray_node(
                                right,
                                origin,
                                direction,
                                dir_inv,
                                positions,
                                radii,
                                default_radius,
                                margin,
                                best_t,
                                best_hit,
                            );
                            Self::query_ray_node(
                                left,
                                origin,
                                direction,
                                dir_inv,
                                positions,
                                radii,
                                default_radius,
                                margin,
                                best_t,
                                best_hit,
                            );
                        }
                    }
                    (Some(_), None) => {
                        Self::query_ray_node(
                            left,
                            origin,
                            direction,
                            dir_inv,
                            positions,
                            radii,
                            default_radius,
                            margin,
                            best_t,
                            best_hit,
                        );
                    }
                    (None, Some(_)) => {
                        Self::query_ray_node(
                            right,
                            origin,
                            direction,
                            dir_inv,
                            positions,
                            radii,
                            default_radius,
                            margin,
                            best_t,
                            best_hit,
                        );
                    }
                    (None, None) => {}
                }
            }
        }
    }
}

/// レイと球の交差判定。
/// 始点 `origin` から正規化方向 `direction` の半直線と、中心 `center`・半径 `radius` の球との交点を計算する。
/// 交差する場合、最小の正のパラメータ `t >= 0.0` を返す。
pub fn intersect_ray_sphere(
    origin: [f32; 3],
    direction: [f32; 3],
    center: [f32; 3],
    radius: f32,
) -> Option<f32> {
    let vx = center[0] - origin[0];
    let vy = center[1] - origin[1];
    let vz = center[2] - origin[2];

    let t_proj = vx * direction[0] + vy * direction[1] + vz * direction[2];
    let v_sq = vx * vx + vy * vy + vz * vz;
    let r_sq = radius * radius;

    // 始点が球の内部にある場合
    if v_sq <= r_sq {
        return Some(0.0);
    }

    // 球がレイの背後にある場合
    if t_proj < 0.0 {
        return None;
    }

    let d_sq = v_sq - t_proj * t_proj;
    if d_sq > r_sq {
        return None;
    }

    let delta = (r_sq - d_sq).max(0.0).sqrt();
    let t = t_proj - delta;
    if t >= 0.0 { Some(t) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AtomGroup;
    use crate::atom_group::AtomGroupExt;
    use std::collections::HashSet;

    #[test]
    fn test_aabb_distance() {
        let box_a = Aabb {
            min: [0.0, 0.0, 0.0],
            max: [1.0, 1.0, 1.0],
        };

        // 点との距離
        assert_eq!(box_a.distance_sq_to_point([0.5, 0.5, 0.5]), 0.0);
        assert_eq!(box_a.distance_sq_to_point([2.0, 0.5, 0.5]), 1.0);
        assert_eq!(box_a.distance_sq_to_point([2.0, 2.0, 1.0]), 2.0);

        // 他のAABBとの距離
        let box_b = Aabb {
            min: [3.0, 0.0, 0.0],
            max: [4.0, 1.0, 1.0],
        };
        assert_eq!(box_a.distance_sq_to_aabb(&box_b), 4.0); // 3.0 - 1.0 = 2.0 -> 2^2 = 4

        // 交差するAABB
        let box_c = Aabb {
            min: [0.5, 0.5, 0.5],
            max: [1.5, 1.5, 1.5],
        };
        assert_eq!(box_a.distance_sq_to_aabb(&box_c), 0.0);
    }

    #[test]
    fn test_bvh_query_pairs_correctness_against_brute_force() {
        // 小規模格子（512原子）を用いて総当たりとBVHの一致を検証
        let (group, _) = AtomGroup::create_test_protein_lattice(2, 2, 2);
        let mol = group.to_flat_molecule();
        let positions: Vec<[f32; 3]> = mol.atoms.iter().map(|a| a.position).collect();
        let n = positions.len();

        let min_dist = 0.4;
        let max_dist = 1.9;
        let min_sq = min_dist * min_dist;
        let max_sq = max_dist * max_dist;

        // 総当たりによる全ペア判定
        let mut brute_pairs = HashSet::new();
        for i in 0..n {
            for j in (i + 1)..n {
                let pa = positions[i];
                let pb = positions[j];
                let dx = pa[0] - pb[0];
                let dy = pa[1] - pb[1];
                let dz = pa[2] - pb[2];
                let dist_sq = dx * dx + dy * dy + dz * dz;
                if dist_sq >= min_sq && dist_sq <= max_sq {
                    brute_pairs.insert((i, j));
                }
            }
        }

        // BVH による全ペア探索
        let bvh = AtomBvh::build(&positions);
        let bvh_pairs_vec = bvh.query_pairs_within_range(min_dist, max_dist);
        let bvh_pairs: HashSet<(usize, usize)> = bvh_pairs_vec.into_iter().collect();

        // 結合数が正の数であり、総当たりと完全に一致することをアサート
        assert!(!brute_pairs.is_empty(), "Bond count should be positive");
        assert_eq!(
            bvh_pairs, brute_pairs,
            "BVH query pairs must exactly match brute-force pairs"
        );
    }

    #[test]
    fn test_bvh_query_radius_correctness() {
        let (group, _) = AtomGroup::create_test_peptide();
        let mol = group.to_flat_molecule();
        let positions: Vec<[f32; 3]> = mol.atoms.iter().map(|a| a.position).collect();

        let bvh = AtomBvh::build(&positions);
        let target = positions[1]; // CA atom
        let radius = 1.6;

        let bvh_res = bvh.query_radius(target, radius);
        let mut brute_res = Vec::new();
        for (i, p) in positions.iter().enumerate() {
            let dx = p[0] - target[0];
            let dy = p[1] - target[1];
            let dz = p[2] - target[2];
            if dx * dx + dy * dy + dz * dz <= radius * radius {
                brute_res.push(i);
            }
        }

        let mut s1 = bvh_res;
        s1.sort_unstable();
        let mut s2 = brute_res;
        s2.sort_unstable();
        assert_eq!(s1, s2);
    }

    #[test]
    fn test_bvh_scaling_large_structure() {
        // 大規模格子（9,216原子）
        let (group, _) = AtomGroup::create_test_large_protein_lattice();
        let mol = group.to_flat_molecule();
        let positions: Vec<[f32; 3]> = mol.atoms.iter().map(|a| a.position).collect();
        assert_eq!(positions.len(), 9216);

        let start = std::time::Instant::now();
        let bvh = AtomBvh::build(&positions);
        let build_time = start.elapsed();

        let start_query = std::time::Instant::now();
        let pairs = bvh.query_pairs_within_range(0.4, 1.9);
        let query_time = start_query.elapsed();

        // 総当たりなら (9216 * 9215) / 2 = 42,458,880 回の距離計算が必要。
        // BVHなら短時間（ミリ秒オーダー）で完了する。
        assert!(!pairs.is_empty());
        println!(
            "Phase 5b-i BVH scale benchmark (9,216 atoms): build={:?}, query={:?}, found {} bonds",
            build_time,
            query_time,
            pairs.len()
        );
    }

    #[test]
    fn test_ray_sphere_intersection() {
        let center = [0.0, 0.0, 10.0];
        let radius = 1.0;

        // 正面からの直撃レイ
        let origin = [0.0, 0.0, 0.0];
        let dir = [0.0, 0.0, 1.0];
        let t = intersect_ray_sphere(origin, dir, center, radius);
        assert!(t.is_some());
        let dist = t.unwrap();
        assert!((dist - 9.0).abs() < 1e-5);

        // 外れレイ
        let miss_dir = [1.0, 0.0, 0.0];
        assert!(intersect_ray_sphere(origin, miss_dir, center, radius).is_none());

        // かすりレイ（距離 0.99 はヒット、1.01 はミス）
        let graze_dir = [0.099, 0.0, 0.9950879]; // sqrt(1 - 0.099^2) ~= 0.9950879
        assert!(intersect_ray_sphere(origin, graze_dir, center, radius).is_some());

        // 背後にある球
        let opp_dir = [0.0, 0.0, -1.0];
        assert!(intersect_ray_sphere(origin, opp_dir, center, radius).is_none());

        // 球の内部からのレイ
        let inside_origin = [0.0, 0.0, 10.5];
        assert_eq!(
            intersect_ray_sphere(inside_origin, dir, center, radius),
            Some(0.0)
        );
    }

    #[test]
    fn test_bvh_query_ray_hit_and_ordering() {
        // Z軸上に3つの原子を配置: A(z=10), B(z=20), C(z=30)
        let positions = vec![[0.0, 0.0, 10.0], [0.0, 0.0, 20.0], [0.0, 0.0, 30.0]];
        let bvh = AtomBvh::build(&positions);

        // Z正方向のレイ -> 最も手前の A (index 0) がヒット
        let hit_forward = bvh.query_ray([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], None, 1.0);
        assert!(hit_forward.is_some());
        let (idx_f, dist_f) = hit_forward.unwrap();
        assert_eq!(idx_f, 0);
        assert!((dist_f - 9.0).abs() < 1e-5);

        // Z負方向のレイ（z=50から） -> 最も手前の C (index 2) がヒット
        let hit_backward = bvh.query_ray([0.0, 0.0, 50.0], [0.0, 0.0, -1.0], None, 1.0);
        assert!(hit_backward.is_some());
        let (idx_b, dist_b) = hit_backward.unwrap();
        assert_eq!(idx_b, 2);
        assert!((dist_b - 19.0).abs() < 1e-5);

        // 各原子ごとの半径指定
        let radii = vec![0.5, 0.5, 0.5];
        let hit_custom_radii = bvh.query_ray([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], Some(&radii), 0.5);
        assert!(hit_custom_radii.is_some());
        let (idx_c, dist_c) = hit_custom_radii.unwrap();
        assert_eq!(idx_c, 0);
        assert!((dist_c - 9.5).abs() < 1e-5);

        // 全く当たらないレイ
        let hit_miss = bvh.query_ray([0.0, 50.0, 0.0], [1.0, 0.0, 0.0], None, 1.0);
        assert!(hit_miss.is_none());
    }

    #[test]
    fn test_bvh_query_nearest() {
        let positions = vec![
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [0.0, 10.0, 0.0],
            [0.0, 0.0, 10.0],
        ];
        let bvh = AtomBvh::build(&positions);

        let nearest = bvh.query_nearest([1.0, 1.0, 1.0]).unwrap();
        assert_eq!(nearest.0, 0);
        assert!((nearest.1 - 3.0).abs() < 1e-5);

        let nearest_x = bvh.query_nearest([9.0, 0.0, 0.0]).unwrap();
        assert_eq!(nearest_x.0, 1);
        assert!((nearest_x.1 - 1.0).abs() < 1e-5);

        let empty_bvh = AtomBvh::build(&[]);
        assert!(empty_bvh.query_nearest([0.0, 0.0, 0.0]).is_none());
    }
}
