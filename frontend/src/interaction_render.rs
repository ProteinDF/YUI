// SPDX-FileCopyrightText: The Yui development team
// SPDX-License-Identifier: GPL-3.0-or-later

//! 非共有結合相互作用（水素結合、塩橋、CH-π）およびジスルフィド結合の描画用データ生成モジュール。
//!
//! `architecture.md` 10.2節に準拠。
//! `AppState`（`interactions`、`atom_group`、`representations`）から、
//! 3Dビューポートに描画する破線（円柱インスタンス）の一覧を純粋関数として生成します。

use std::collections::HashSet;
use yui_core::atom_group::AtomGroup;
use yui_core::interaction::{
    InteractionKind, InteractionSet, ResolvedInteraction, resolve_all_interactions,
};
use yui_core::representation::{ColorScheme, Representation, RepresentationKind};
use yui_core::selector::Selector;

/// 水素結合の既定色（黄）
pub const COLOR_HYDROGEN_BOND: [f32; 4] = [1.0, 0.85, 0.1, 1.0];
/// 塩橋の既定色（オレンジ）
pub const COLOR_SALT_BRIDGE: [f32; 4] = [1.0, 0.5, 0.0, 1.0];
/// CH-π相互作用の既定色（シアン系）
pub const COLOR_CH_PI: [f32; 4] = [0.0, 0.8, 0.9, 1.0];
/// ジスルフィド結合の既定色（マゼンタ）: 水素結合の黄色やシアン、オレンジと明確に弁別可能
pub const COLOR_DISULFIDE: [f32; 4] = [0.85, 0.2, 0.85, 1.0];

/// 相互作用の種類に応じた既定色を返す
pub fn default_interaction_color(kind: InteractionKind) -> [f32; 4] {
    match kind {
        InteractionKind::HydrogenBond => COLOR_HYDROGEN_BOND,
        InteractionKind::SaltBridge => COLOR_SALT_BRIDGE,
        InteractionKind::ChPi => COLOR_CH_PI,
        InteractionKind::Disulfide => COLOR_DISULFIDE,
    }
}

/// 描画用破線の要素（両端の3D座標・RGBA色・種類）
#[derive(Debug, Clone, PartialEq)]
pub struct InteractionLine {
    pub start: [f32; 3],
    pub end: [f32; 3],
    pub color: [f32; 4],
    pub kind: InteractionKind,
}

/// 事前解決済みの相互作用リスト（`[ResolvedInteraction]`）から、
/// 指定された表現設定（`[Representation]`）に基づいて描画対象の破線一覧を生成する高速純粋関数。
///
/// 毎フレームの描画処理において、(相互作用数)×(全原子数)の走査を行わず、
/// 保持済みの端点座標・端点原子IDに対して選択範囲・種類・可視性での高速な絞り込み（O(相互作用数)）を行います。
pub fn build_interaction_lines_from_resolved(
    atom_group: Option<&AtomGroup>,
    resolved_interactions: &[ResolvedInteraction],
    representations: &[Representation],
    active_model: Option<&str>,
    model_atom_ranges: &[yui_core::atom_group::ModelAtomRange],
) -> Vec<InteractionLine> {
    if resolved_interactions.is_empty() {
        return Vec::new();
    }

    let target_range =
        active_model.and_then(|model| model_atom_ranges.iter().find(|r| r.model_key == model));

    if active_model.is_some() && target_range.is_none() {
        return Vec::new();
    }

    let mut lines = Vec::new();

    for rep in representations {
        if !rep.visible || rep.kind != RepresentationKind::Interaction {
            continue;
        }

        // 選択された原子ID集合（Selector::All の場合は全選択として None 表現）
        let selected_atom_ids: Option<HashSet<u32>> = match &rep.selection {
            Selector::All => None,
            sel => atom_group.map(|ag| sel.select_atom_ids(ag).into_iter().collect()),
        };

        for inter in resolved_interactions {
            // 0. モデルの絞り込み（両端がそのモデルに属するもの）
            if let Some(range) = target_range {
                let (ref ids0, ref ids1) = inter.endpoint_atom_ids;
                let in_model0 = !ids0.is_empty() && ids0.iter().all(|&id| range.contains(id));
                let in_model1 = !ids1.is_empty() && ids1.iter().all(|&id| range.contains(id));
                if !in_model0 || !in_model1 {
                    continue;
                }
            }

            // 1. 種類の絞り込み
            if !rep.interaction_kinds.is_empty() && !rep.interaction_kinds.contains(&inter.kind) {
                continue;
            }

            // 2. 選択の判定（両端のうち少なくとも一方の原子IDが selection に重なるか）
            if let Some(ref sel_ids) = selected_atom_ids {
                let (ref ids0, ref ids1) = inter.endpoint_atom_ids;
                let overlaps = ids0.iter().any(|id| sel_ids.contains(id))
                    || ids1.iter().any(|id| sel_ids.contains(id));
                if !overlaps {
                    continue;
                }
            }

            // 3. 端点座標（事前解決済み）
            let (start, end) = inter.endpoints;

            // 4. 配色の導出
            let base_color = match rep.color {
                ColorScheme::Uniform(c) => c,
                _ => default_interaction_color(inter.kind),
            };

            let mut color = base_color;
            color[3] = (color[3] * rep.opacity).clamp(0.0, 1.0);

            lines.push(InteractionLine {
                start,
                end,
                color,
                kind: inter.kind,
            });
        }
    }

    lines
}

/// `AtomGroup`, `InteractionSet`, `[Representation]` から描画対象の相互作用破線一覧を生成する関数。
///
/// 内部で `resolve_all_interactions` により端点座標・端点原子IDを解決し、
/// `build_interaction_lines_from_resolved` に委譲します（単体テスト等との後方互換性のため維持）。
#[allow(dead_code)]
pub fn build_interaction_lines(
    atom_group: Option<&AtomGroup>,
    interactions: &InteractionSet,
    representations: &[Representation],
) -> Vec<InteractionLine> {
    let Some(atom_group) = atom_group else {
        return Vec::new();
    };

    if interactions.is_empty() {
        return Vec::new();
    }

    let resolved = resolve_all_interactions(atom_group, interactions);
    build_interaction_lines_from_resolved(Some(atom_group), &resolved, representations, None, &[])
}

#[cfg(test)]
mod tests {
    use super::*;
    use yui_core::structure_loader::parse_structure_bytes;

    fn load_1hls_fixture() -> (AtomGroup, InteractionSet) {
        let pdb_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/tests/data/1hls.pdb");
        let bytes = std::fs::read(pdb_path).expect("failed to read 1hls.pdb");
        let (group, _sec_map, interactions) =
            parse_structure_bytes(&bytes, "1hls.pdb").expect("failed to parse 1hls.pdb");
        (group, interactions)
    }

    #[test]
    fn test_build_interaction_lines_1hls() {
        let (group, interactions) = load_1hls_fixture();
        assert!(
            !interactions.is_empty(),
            "1HLS should have detected interactions"
        );

        // 1. Selector::All・全種類なら、件数が interactions の件数と一致する
        let rep_all = Representation::new(
            "rep_all_inter",
            Selector::All,
            RepresentationKind::Interaction,
            ColorScheme::Element, // non-Uniform -> 既定色
        );
        let lines_all = build_interaction_lines(Some(&group), &interactions, &[rep_all]);
        assert_eq!(lines_all.len(), interactions.len());
        // 1HLS PDB では 38 本 (SS=3, HB=28, ChPi=7)
        assert_eq!(lines_all.len(), 38);

        // 2. interaction_kinds = [Disulfide] なら 3本になる
        let rep_disulfide = Representation::new(
            "rep_ss",
            Selector::All,
            RepresentationKind::Interaction,
            ColorScheme::Element,
        )
        .with_interaction_kinds(vec![InteractionKind::Disulfide]);
        let lines_ss = build_interaction_lines(Some(&group), &interactions, &[rep_disulfide]);
        assert_eq!(lines_ss.len(), 3);
        assert!(
            lines_ss
                .iter()
                .all(|l| l.kind == InteractionKind::Disulfide)
        );

        // 3. 鎖 A だけを選ぶ Selector では、A–B 鎖間のジスルフィド結合（A7–B7、A20–B19）も含まれる
        //    （少なくとも一方の端点が含まれる、という規則による）
        //    1HLS には A6-A11 (鎖内), A7-B7 (鎖間), A20-B19 (鎖間) の3本があり、すべて鎖Aの原子を含む
        let rep_chain_a_ss = Representation::new(
            "rep_chain_a_ss",
            Selector::PathWildcard("/model_1/A/*".to_string()),
            RepresentationKind::Interaction,
            ColorScheme::Element,
        )
        .with_interaction_kinds(vec![InteractionKind::Disulfide]);
        let lines_chain_a_ss =
            build_interaction_lines(Some(&group), &interactions, &[rep_chain_a_ss]);
        assert_eq!(
            lines_chain_a_ss.len(),
            3,
            "All 3 disulfides in 1HLS have at least one endpoint on chain A"
        );

        // 鎖 B だけを選ぶ Selector: A6-A11 は含まれず、A7-B7, A20-B19 の2本のみが含まれるはず
        let rep_chain_b_ss = Representation::new(
            "rep_chain_b_ss",
            Selector::PathWildcard("/model_1/B/*".to_string()),
            RepresentationKind::Interaction,
            ColorScheme::Element,
        )
        .with_interaction_kinds(vec![InteractionKind::Disulfide]);
        let lines_chain_b_ss =
            build_interaction_lines(Some(&group), &interactions, &[rep_chain_b_ss]);
        assert_eq!(
            lines_chain_b_ss.len(),
            2,
            "Chain B selection should only match inter-chain disulfides (A7-B7 and A20-B19)"
        );

        // 4. visible: false なら 0件になる
        let mut rep_invisible = Representation::new(
            "rep_invis",
            Selector::All,
            RepresentationKind::Interaction,
            ColorScheme::Element,
        );
        rep_invisible.visible = false;
        let lines_invis = build_interaction_lines(Some(&group), &interactions, &[rep_invisible]);
        assert_eq!(lines_invis.len(), 0);

        // 5. Uniform のときは指定色、それ以外のときは種類ごとの既定色になる
        let custom_color = [0.1, 0.2, 0.3, 1.0];
        let rep_uniform = Representation::new(
            "rep_uniform",
            Selector::All,
            RepresentationKind::Interaction,
            ColorScheme::Uniform(custom_color),
        );
        let lines_uniform = build_interaction_lines(Some(&group), &interactions, &[rep_uniform]);
        assert!(!lines_uniform.is_empty());
        for line in &lines_uniform {
            assert_eq!(line.color, custom_color);
        }

        // 種類ごとの既定色の検証（non-Uniform）
        for line in &lines_all {
            match line.kind {
                InteractionKind::HydrogenBond => assert_eq!(line.color, COLOR_HYDROGEN_BOND),
                InteractionKind::SaltBridge => assert_eq!(line.color, COLOR_SALT_BRIDGE),
                InteractionKind::ChPi => assert_eq!(line.color, COLOR_CH_PI),
                InteractionKind::Disulfide => assert_eq!(line.color, COLOR_DISULFIDE),
            }
        }
    }

    #[test]
    fn test_build_interaction_lines_from_resolved_cache() {
        let (group, interactions) = load_1hls_fixture();
        let resolved = resolve_all_interactions(&group, &interactions);
        assert_eq!(resolved.len(), interactions.len());

        let rep_all = Representation::new(
            "rep_all",
            Selector::All,
            RepresentationKind::Interaction,
            ColorScheme::Element,
        );
        let lines_from_cache = build_interaction_lines_from_resolved(
            Some(&group),
            &resolved,
            std::slice::from_ref(&rep_all),
            None,
            &[],
        );
        let lines_direct =
            build_interaction_lines(Some(&group), &interactions, std::slice::from_ref(&rep_all));
        assert_eq!(lines_from_cache, lines_direct);

        // セレクタ絞り込み時の結果一致検証
        let rep_chain_b = Representation::new(
            "rep_chain_b_ss",
            Selector::PathWildcard("/model_1/B/*".to_string()),
            RepresentationKind::Interaction,
            ColorScheme::Element,
        )
        .with_interaction_kinds(vec![InteractionKind::Disulfide]);

        let lines_sel_cached = build_interaction_lines_from_resolved(
            Some(&group),
            &resolved,
            std::slice::from_ref(&rep_chain_b),
            None,
            &[],
        );
        let lines_sel_direct = build_interaction_lines(
            Some(&group),
            &interactions,
            std::slice::from_ref(&rep_chain_b),
        );
        assert_eq!(lines_sel_cached, lines_sel_direct);
        assert_eq!(lines_sel_cached.len(), 2);
    }

    #[test]
    fn test_build_interaction_lines_multi_model_filtering() {
        use yui_core::atom_group::calculate_model_atom_ranges;

        let cif_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/tests/data/1HLS.cif");
        let bytes = std::fs::read(cif_path).expect("failed to read 1HLS.cif");
        let (group, _sec_map, interactions) =
            parse_structure_bytes(&bytes, "1HLS.cif").expect("parse 1HLS.cif");

        let resolved = resolve_all_interactions(&group, &interactions);
        let ranges = calculate_model_atom_ranges(&group);
        assert_eq!(ranges.len(), 20);

        // ジスルフィド結合のみを抽出する Representation
        let rep_ss = Representation::new(
            "rep_ss",
            Selector::All,
            RepresentationKind::Interaction,
            ColorScheme::Element,
        )
        .with_interaction_kinds(vec![InteractionKind::Disulfide]);

        // 1. active_model が None の場合: 全20モデル分のジスルフィド結合（20 * 3 = 60本）
        let lines_all = build_interaction_lines_from_resolved(
            Some(&group),
            &resolved,
            std::slice::from_ref(&rep_ss),
            None,
            &ranges,
        );
        assert_eq!(
            lines_all.len(),
            60,
            "1HLS.cif without active_model should display all 60 disulfide bonds"
        );

        // 2. active_model を第1モデルに指定した場合: 該当モデルの3本のみに絞り込まれること
        let first_model = &ranges[0].model_key;
        let lines_m1 = build_interaction_lines_from_resolved(
            Some(&group),
            &resolved,
            std::slice::from_ref(&rep_ss),
            Some(first_model),
            &ranges,
        );
        assert_eq!(
            lines_m1.len(),
            3,
            "1HLS.cif with active_model specified should display exactly 3 disulfide bonds"
        );
    }
}
