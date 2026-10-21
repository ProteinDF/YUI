// SPDX-FileCopyrightText: The Yui development team
// SPDX-License-Identifier: GPL-3.0-or-later

//! 非共有結合相互作用（水素結合、塩橋、CH-π）およびジスルフィド結合の端点解決モジュール。
//!
//! `proteindf-bridge` の `InteractionSet::detect_all` により検出された `Interaction` から、
//! 3Dビューポートでの破線描画に必要な両端の座標（`[f32; 3]`）およびフィルタリング用の
//! 原子ID集合（`Vec<u32>`）を導出します。

pub use proteindf_bridge::ch_pi::AROMATIC_RINGS;
pub use proteindf_bridge::{Interaction, InteractionKind, InteractionSet};

use crate::atom_group::{AtomGroup, iter_flat_atoms};

/// 1つの相互作用（`Interaction`）から、線を引く両端の3次元座標を求める。
///
/// 端点の導出規則:
/// 1. パスが原子を指す場合: その原子の 3D 座標。
/// 2. CH-π 相互作用の芳香環（残基パス）: `proteindf_bridge::ch_pi::calc_ring_geometry` による環の重心。
/// 3. 塩橋でパスが残基や官能基を指す場合: 【暫定仕様】`proteindf-bridge` に官能基中心を求める
///    公開関数が提供されるまでの暫定措置として、該当グループに含まれる全原子の重心（算術平均）を使用。
///
/// 端点の解決に失敗した場合はパニックせず `None` を返します。
pub fn resolve_interaction_endpoints(
    atom_group: &AtomGroup,
    interaction: &Interaction,
) -> Option<([f32; 3], [f32; 3])> {
    if interaction.atoms.len() < 2 {
        return None;
    }
    let p0 = resolve_endpoint_position(&atom_group.0, &interaction.atoms[0], interaction.kind)?;
    let p1 = resolve_endpoint_position(&atom_group.0, &interaction.atoms[1], interaction.kind)?;
    Some((p0, p1))
}

/// 1つの相互作用（`Interaction`）の各端点に対応する原子 ID の集合を求める。
///
/// 6i-ii において「`Representation.selection` に含まれる原子に関わる相互作用だけを表示する」
/// フィルタリングに使用されます。
///
/// 原子 ID の採番は `iter_flat_atoms` と完全に一致します。
/// 端点が残基を指す場合は、その残基に属するすべての原子の ID 集合となります。
/// 端点の解決に失敗した場合はパニックせず `None` を返します。
pub fn resolve_interaction_atom_ids(
    atom_group: &AtomGroup,
    interaction: &Interaction,
) -> Option<(Vec<u32>, Vec<u32>)> {
    if interaction.atoms.len() < 2 {
        return None;
    }
    let ids0 = resolve_endpoint_atom_ids(&atom_group.0, &interaction.atoms[0])?;
    let ids1 = resolve_endpoint_atom_ids(&atom_group.0, &interaction.atoms[1])?;
    Some((ids0, ids1))
}

/// 解決済みの単一相互作用データ。
///
/// 構造読み込み時に事前計算され、毎フレームの3D描画において
/// (相互作用数)×(全原子数)の重複走査を防ぐために使用されます。
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedInteraction {
    /// 相互作用の種類
    pub kind: InteractionKind,
    /// 始点と終点の3次元座標（Å）
    pub endpoints: ([f32; 3], [f32; 3]),
    /// 各端点に対応する原子ID群（端点0, 端点1）
    pub endpoint_atom_ids: (Vec<u32>, Vec<u32>),
}

/// `AtomGroup` と `InteractionSet` から、描画に必要な端点座標および端点原子IDを
/// 事前にすべて解決した `Vec<ResolvedInteraction>` を生成する。
///
/// 端点の解決に失敗した相互作用はスキップされます。
pub fn resolve_all_interactions(
    atom_group: &AtomGroup,
    interactions: &InteractionSet,
) -> Vec<ResolvedInteraction> {
    if interactions.is_empty() {
        return Vec::new();
    }

    let mut resolved = Vec::with_capacity(interactions.len());
    for inter in &interactions.interactions {
        let Some(endpoints) = resolve_interaction_endpoints(atom_group, inter) else {
            continue;
        };
        let Some(endpoint_atom_ids) = resolve_interaction_atom_ids(atom_group, inter) else {
            continue;
        };
        resolved.push(ResolvedInteraction {
            kind: inter.kind,
            endpoints,
            endpoint_atom_ids,
        });
    }
    resolved
}

/// パス文字列から端点の3次元座標を解決する。
fn resolve_endpoint_position(
    root: &proteindf_bridge::AtomGroup,
    path: &str,
    kind: InteractionKind,
) -> Option<[f32; 3]> {
    // 1. 原子パスとしての解決を試みる（/model/chain/residue/atom 等）
    if let Some(atom) = root.get_atom_by_path(path) {
        return Some([atom.xyz.x as f32, atom.xyz.y as f32, atom.xyz.z as f32]);
    }

    // 2. 残基・官能基グループとしての解決を試みる
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segs.is_empty() {
        return None;
    }
    let group = find_group_by_path(root, &segs)?;

    // 2a. CH-π 相互作用の芳香環の場合、環の重心を算出
    if kind == InteractionKind::ChPi {
        let res_name = group.name.trim();
        if let Some((center, _normal)) = AROMATIC_RINGS
            .iter()
            .find(|r| r.residue_name == res_name)
            .and_then(|ring_def| {
                proteindf_bridge::ch_pi::calc_ring_geometry(group, ring_def.atom_names)
            })
        {
            return Some([center.x as f32, center.y as f32, center.z as f32]);
        }
    }

    // 2b. 塩橋等の残基・官能基グループの場合:
    // 【暫定】proteindf-bridge 側に官能基中心を返す公開APIがないため、
    // 暫定的にグループ配下の全原子の重心を用いる。
    calc_group_centroid(group)
}

/// パス文字列から対応する原子 ID 集合を解決する。
fn resolve_endpoint_atom_ids(root: &proteindf_bridge::AtomGroup, path: &str) -> Option<Vec<u32>> {
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segs.is_empty() {
        return None;
    }

    let mut ids = Vec::new();
    for flat in iter_flat_atoms(root) {
        let matches = match segs.len() {
            // 原子パス: /model/chain/residue/atom
            4 => {
                flat.model_key == segs[0]
                    && flat.chain_key == segs[1]
                    && flat.residue_key == segs[2]
                    && (flat.atom_key == segs[3] || flat.atom.name == segs[3])
            }
            // 残基パス: /model/chain/residue
            3 => {
                flat.model_key == segs[0]
                    && flat.chain_key == segs[1]
                    && flat.residue_key == segs[2]
            }
            // 万一のモデル・鎖パス
            2 => flat.model_key == segs[0] && flat.chain_key == segs[1],
            1 => flat.model_key == segs[0],
            _ => false,
        };
        if matches {
            ids.push(flat.id);
        }
    }

    if ids.is_empty() { None } else { Some(ids) }
}

/// 階層セグメントを辿って指定パスのグループを取得する。
fn find_group_by_path<'a>(
    root: &'a proteindf_bridge::AtomGroup,
    segs: &[&str],
) -> Option<&'a proteindf_bridge::AtomGroup> {
    let mut current = root;
    for seg in segs {
        current = current.get_group(seg)?;
    }
    Some(current)
}

/// グループ配下の全原子の重心（算術平均）を算出する。
fn calc_group_centroid(group: &proteindf_bridge::AtomGroup) -> Option<[f32; 3]> {
    let mut count = 0usize;
    let mut sum = [0.0f64; 3];
    collect_atoms_for_centroid(group, &mut sum, &mut count);
    if count == 0 {
        None
    } else {
        Some([
            (sum[0] / count as f64) as f32,
            (sum[1] / count as f64) as f32,
            (sum[2] / count as f64) as f32,
        ])
    }
}

/// 再帰的にグループ配下の全原子座標を集計する。
fn collect_atoms_for_centroid(
    group: &proteindf_bridge::AtomGroup,
    sum: &mut [f64; 3],
    count: &mut usize,
) {
    for (_, atom) in group.atoms() {
        sum[0] += atom.xyz.x;
        sum[1] += atom.xyz.y;
        sum[2] += atom.xyz.z;
        *count += 1;
    }
    for (_, child) in group.groups() {
        collect_atoms_for_centroid(child, sum, count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atom_group::AtomGroupExt;

    #[test]
    fn test_resolve_invalid_interactions() {
        let (group, _) = AtomGroup::create_test_peptide();

        // 1. 端点不足 (< 2)
        let empty_inter = Interaction::new(InteractionKind::HydrogenBond, vec![]);
        assert!(resolve_interaction_endpoints(&group, &empty_inter).is_none());
        assert!(resolve_interaction_atom_ids(&group, &empty_inter).is_none());

        let single_inter =
            Interaction::new(InteractionKind::HydrogenBond, vec!["/A/1/N".to_string()]);
        assert!(resolve_interaction_endpoints(&group, &single_inter).is_none());
        assert!(resolve_interaction_atom_ids(&group, &single_inter).is_none());

        // 2. 存在しないパス
        let not_found = Interaction::new(
            InteractionKind::HydrogenBond,
            vec![
                "/nonexistent/1/N".to_string(),
                "/nonexistent/2/O".to_string(),
            ],
        );
        assert!(resolve_interaction_endpoints(&group, &not_found).is_none());
        assert!(resolve_interaction_atom_ids(&group, &not_found).is_none());
    }

    #[test]
    fn test_resolve_peptide_endpoints_and_ids() {
        let (group, _) = AtomGroup::create_test_peptide();

        // 原子同士の相互作用（例: N と CA）
        let inter = Interaction::new(
            InteractionKind::HydrogenBond,
            vec!["/model_1/A/5/N".to_string(), "/model_1/A/6/CA".to_string()],
        );

        let endpoints = resolve_interaction_endpoints(&group, &inter);
        assert!(endpoints.is_some());
        let (p0, p1) = endpoints.unwrap();
        assert!(p0.iter().all(|c| !c.is_nan()));
        assert!(p1.iter().all(|c| !c.is_nan()));

        let atom_ids = resolve_interaction_atom_ids(&group, &inter);
        assert!(atom_ids.is_some());
        let (ids0, ids1) = atom_ids.unwrap();
        assert_eq!(ids0.len(), 1);
        assert_eq!(ids1.len(), 1);
        assert_ne!(ids0[0], ids1[0]);

        // 残基パス（塩橋など）: 残基全体の重心および残基内全原子のID集合
        let res_inter = Interaction::new(
            InteractionKind::SaltBridge,
            vec!["/model_1/A/5/".to_string(), "/model_1/A/6/".to_string()],
        );
        let res_endpoints = resolve_interaction_endpoints(&group, &res_inter);
        assert!(res_endpoints.is_some());

        let res_atom_ids = resolve_interaction_atom_ids(&group, &res_inter);
        assert!(res_atom_ids.is_some());
        let (r_ids0, r_ids1) = res_atom_ids.unwrap();
        assert!(r_ids0.len() > 1, "residue 5 should contain multiple atoms");
        assert!(r_ids1.len() > 1, "residue 6 should contain multiple atoms");
    }

    #[test]
    fn test_resolve_all_interactions() {
        let (group, _) = AtomGroup::create_test_peptide();

        // 1. 空の InteractionSet -> 空の Vec
        let empty_set = InteractionSet::new();
        assert!(resolve_all_interactions(&group, &empty_set).is_empty());

        // 2. 有効な相互作用と無効な相互作用の混在
        let mut set = InteractionSet::new();
        set.interactions.push(Interaction::new(
            InteractionKind::HydrogenBond,
            vec!["/model_1/A/5/N".to_string(), "/model_1/A/6/CA".to_string()],
        ));
        set.interactions.push(Interaction::new(
            InteractionKind::HydrogenBond,
            vec!["/invalid/path".to_string()],
        ));
        set.interactions.push(Interaction::new(
            InteractionKind::SaltBridge,
            vec!["/model_1/A/5/".to_string(), "/model_1/A/6/".to_string()],
        ));

        let resolved = resolve_all_interactions(&group, &set);
        assert_eq!(resolved.len(), 2, "invalid interaction should be skipped");
        assert_eq!(resolved[0].kind, InteractionKind::HydrogenBond);
        assert_eq!(resolved[1].kind, InteractionKind::SaltBridge);
    }
}
