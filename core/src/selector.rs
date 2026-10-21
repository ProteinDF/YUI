//! Selector matching engine for `yui-core`, built on the real `proteindf_bridge::AtomGroup`
//! via `core::atom_group`'s `FlatAtomRef`/`iter_flat_atoms` traversal helpers (Phase 6e-iii).
//!
//! # NOTE: `Selector` Itself Is Not a Stand-in
//! Unlike the former local `AtomGroup` stand-in (replaced in Phase 6e-ii), the `Selector` enum
//! is a permanent YUI-specific design: it ports `proteindf-bridge`'s `Select_*` predicate family
//! and adds `And`/`Or`/`Not` boolean combinators that bridge's trait-based selectors don't have
//! (see Chapter 6.2 and Chapter 10 of `architecture.md`). Only the internal `matches()`/
//! `select_atom_ids()` implementation changed when bridge integration landed.
//!
//! ## Path Wildcard Matching Semantics (Specification from §6.2)
//! - Paths are `/`-delimited sequences of segments.
//! - Leading and trailing slashes are ignored: `/A/6` and `/A/6/` both evaluate to `["A", "6"]`.
//! - A `*` segment matches exactly one arbitrary segment at that position (single-level glob; no recursive `**`).
//! - A pattern matches an `AtomGroup` node when the node's path segments equal the pattern's segments position-by-position.
//! - Matching an intermediate node (e.g. a residue) resolves to every atom in the subtree rooted at that node.

use crate::atom_group::{AtomGroup, FlatAtomRef, iter_flat_atoms};
use serde::{Deserialize, Serialize};

/// 後方互換性のための型エイリアス（`FlatAtomRef` へのエイリアス）
pub type AtomContext<'a> = FlatAtomRef<'a>;

/// 選択述語および論理結合（Selector）
///
/// `proteindf-bridge` の `Select_*` 述語ファミリをRustに移植し、
/// 新規に `And`, `Or`, `Not` のブール合成コンビネータを追加した階層セレクタ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Selector {
    All,
    None,

    #[serde(alias = "Select_Symbol", alias = "select_symbol")]
    Symbol(Vec<String>),

    #[serde(alias = "Select_Name", alias = "select_name")]
    Name(Vec<String>),

    #[serde(alias = "Select_ResidueName", alias = "select_residue_name")]
    ResidueName(Vec<String>),

    #[serde(
        alias = "Select_Range",
        alias = "select_range",
        alias = "residue_range"
    )]
    ResidueRange {
        chain: Option<String>,
        start: i32,
        end: i32,
    },

    #[serde(
        alias = "Select_Path_wildcard",
        alias = "Select_PathWildcard",
        alias = "select_path_wildcard",
        alias = "path_wildcard"
    )]
    PathWildcard(String),

    #[serde(
        alias = "Select_PathRegex",
        alias = "select_path_regex",
        alias = "path_regex"
    )]
    PathRegex(String),

    #[serde(alias = "Select_Atom", alias = "select_atom", alias = "atom_ids")]
    AtomIds(Vec<u32>),

    #[serde(alias = "and")]
    And(Vec<Selector>),

    #[serde(alias = "or")]
    Or(Vec<Selector>),

    #[serde(alias = "not")]
    Not {
        inner: Box<Selector>,
    },
}

impl Selector {
    /// 与えられた原子コンテキスト（`FlatAtomRef`）がこのセレクタに一致するかを判定する
    pub fn matches(&self, flat: &FlatAtomRef) -> bool {
        match self {
            Selector::All => true,
            Selector::None => false,
            Selector::Symbol(elements) => {
                let symbol = match flat.atom.symbol() {
                    Ok(s) => s,
                    Err(e) => {
                        log::warn!(
                            "Failed to resolve element symbol for atom '{}' (atomic_number: {}): {}. Falling back to 'X'",
                            flat.atom.name,
                            flat.atom.atomic_number(),
                            e
                        );
                        "X"
                    }
                };
                elements
                    .iter()
                    .any(|elem| elem.eq_ignore_ascii_case(symbol))
            }
            Selector::Name(names) => names.iter().any(|name| name == &flat.atom.name),
            Selector::ResidueName(names) => names
                .iter()
                .any(|name| name.eq_ignore_ascii_case(flat.residue_name)),
            Selector::ResidueRange { chain, start, end } => {
                if let Some(ch) = chain
                    && ch != flat.chain_key
                {
                    return false;
                }
                flat.residue_index >= *start && flat.residue_index <= *end
            }
            Selector::PathWildcard(pattern) => match_path_wildcard(pattern, &flat.atom.path),
            Selector::PathRegex(regex_str) => match_path_regex(regex_str, &flat.atom.path),
            Selector::AtomIds(ids) => ids.contains(&flat.id),
            Selector::And(sub_selectors) => sub_selectors.iter().all(|s| s.matches(flat)),
            Selector::Or(sub_selectors) => sub_selectors.iter().any(|s| s.matches(flat)),
            Selector::Not { inner } => !inner.matches(flat),
        }
    }

    /// `AtomGroup` 全体を走査し、このセレクタに合致する全原子のIDリストを返す。
    ///
    /// `iter_flat_atoms` を `to_flat_molecule()` と共有することにより、
    /// 走査順序および原子ID採番の完全な一貫性を保証する。
    pub fn select_atom_ids(&self, atom_group: &AtomGroup) -> Vec<u32> {
        iter_flat_atoms(atom_group)
            .filter(|flat| self.matches(flat))
            .map(|flat| flat.id)
            .collect()
    }
}

/// スラッシュ区切りパス文字列をセグメント列に分解する。
/// 先頭および末尾のスラッシュは無視される。
/// 例: `"/A/6/"` -> `["A", "6"]`
pub fn split_path_segments(path: &str) -> Vec<&str> {
    path.split('/').filter(|s| !s.is_empty()).collect()
}

/// 2つのセグメント列が位置ごとに一致するか判定する。
/// パターン側の `*` は任意の一致（単一セグメント）として扱う。
fn segments_match(pattern: &[&str], node: &[&str]) -> bool {
    if pattern.len() != node.len() {
        return false;
    }
    for (p, n) in pattern.iter().zip(node.iter()) {
        if *p != "*" && *p != *n {
            return false;
        }
    }
    true
}

/// `Select_PathWildcard` のマッチング判定（architecture.md 6.2節仕様準拠）
///
/// `atom_path`（例: `"/model_1/A/5/CA"`）からセグメント列を抽出し、
/// その先頭プレフィックス（フルパス系およびモデル省略系）のいずれかが
/// パターンセグメント列と位置一致するか判定する。
/// 中間ノード（Model, Chain, Residue）に一致した場合は配下の全原子に解決される。
pub fn match_path_wildcard(pattern: &str, atom_path: &str) -> bool {
    let pattern_segments = split_path_segments(pattern);
    if pattern_segments.is_empty() {
        return false;
    }

    let full_segments = split_path_segments(atom_path);
    if full_segments.is_empty() {
        return false;
    }

    // 1. フルパスプレフィックス（Model, Chain, Residue, Atom）
    for len in 1..=full_segments.len() {
        if segments_match(&pattern_segments, &full_segments[..len]) {
            return true;
        }
    }

    // 2. モデル省略プレフィックス（Chain, Residue, Atom）
    // full_segments の深さが2以上（例: model_1, A, ...）の場合、model セグメントを除いたもの
    if full_segments.len() >= 2 {
        let shorthand = &full_segments[1..];
        for len in 1..=shorthand.len() {
            if segments_match(&pattern_segments, &shorthand[..len]) {
                return true;
            }
        }
    }

    false
}

/// `Select_PathRegex` のマッチング判定
pub fn match_path_regex(regex_str: &str, atom_path: &str) -> bool {
    let Ok(re) = regex::Regex::new(regex_str) else {
        return false;
    };

    let full_segments = split_path_segments(atom_path);
    if full_segments.is_empty() {
        return false;
    }

    // 1. フルパスプレフィックスのパス文字列（例: "/model_1", "/model_1/A", "/model_1/A/6", "/model_1/A/6/CA"）
    let mut current_full = String::new();
    for seg in &full_segments {
        current_full.push('/');
        current_full.push_str(seg);
        if re.is_match(&current_full) {
            return true;
        }
    }

    // 2. モデル省略プレフィックスのパス文字列（例: "/A", "/A/6", "/A/6/CA"）
    if full_segments.len() >= 2 {
        let mut current_short = String::new();
        for seg in &full_segments[1..] {
            current_short.push('/');
            current_short.push_str(seg);
            if re.is_match(&current_short) {
                return true;
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atom_group::{AtomGroup, AtomGroupExt};
    use proteindf_bridge::{Atom, Position};

    #[test]
    fn test_path_wildcard_matching_semantics() {
        let (group, _sec_map) = AtomGroup::create_test_peptide();

        // 1. 先頭・末尾スラッシュの無視: /A/6 と /A/6/ は同一結果
        let sel_no_trailing = Selector::PathWildcard("/A/6".to_string());
        let sel_trailing = Selector::PathWildcard("/A/6/".to_string());
        let ids_no_trailing = sel_no_trailing.select_atom_ids(&group);
        let ids_trailing = sel_trailing.select_atom_ids(&group);

        assert_eq!(ids_no_trailing, ids_trailing);
        // 残基6 (GLY) の全4原子 (id 6, 7, 8, 9) が選択されること（中間ノード解決）
        assert_eq!(ids_no_trailing, vec![6, 7, 8, 9]);

        // 2. /*/A/6 による任意モデルのマッチング
        let sel_wildcard_model = Selector::PathWildcard("/*/A/6".to_string());
        let ids_wildcard = sel_wildcard_model.select_atom_ids(&group);
        assert_eq!(ids_wildcard, vec![6, 7, 8, 9]);

        // 3. 完全パス /model_1/A/5/CA による単一原子マッチング
        let sel_exact_atom = Selector::PathWildcard("/model_1/A/5/CA".to_string());
        let ids_exact = sel_exact_atom.select_atom_ids(&group);
        assert_eq!(ids_exact, vec![2]); // 残基5のCA (id 2)

        // 4. モデル省略の単一原子マッチング /A/5/CA
        let sel_atom_shorthand = Selector::PathWildcard("/A/5/CA".to_string());
        let ids_shorthand = sel_atom_shorthand.select_atom_ids(&group);
        assert_eq!(ids_shorthand, vec![2]);
    }

    #[test]
    fn test_predicates_and_boolean_combinators() {
        let (group, _sec_map) = AtomGroup::create_test_peptide();

        // Symbol
        let sel_carbon = Selector::Symbol(vec!["C".to_string()]);
        let carbon_ids = sel_carbon.select_atom_ids(&group);
        // C原子: 2(CA), 3(C), 5(CB), 7(CA), 8(C)
        assert_eq!(carbon_ids, vec![2, 3, 5, 7, 8]);

        // Name
        let sel_ca = Selector::Name(vec!["CA".to_string()]);
        assert_eq!(sel_ca.select_atom_ids(&group), vec![2, 7]);

        // ResidueName
        let sel_ala = Selector::ResidueName(vec!["ALA".to_string()]);
        assert_eq!(sel_ala.select_atom_ids(&group), vec![1, 2, 3, 4, 5]);

        // ResidueRange
        let sel_range = Selector::ResidueRange {
            chain: Some("A".to_string()),
            start: 6,
            end: 6,
        };
        assert_eq!(sel_range.select_atom_ids(&group), vec![6, 7, 8, 9]);

        // AtomIds
        let sel_ids = Selector::AtomIds(vec![1, 4, 9]);
        assert_eq!(sel_ids.select_atom_ids(&group), vec![1, 4, 9]);

        // Boolean: And(ALA, CA) -> id 2
        let sel_and = Selector::And(vec![sel_ala.clone(), sel_ca.clone()]);
        assert_eq!(sel_and.select_atom_ids(&group), vec![2]);

        // Boolean: Or(CA, O)
        let sel_o = Selector::Name(vec!["O".to_string()]);
        let sel_or = Selector::Or(vec![sel_ca, sel_o]);
        // CA (2, 7), O (4, 9)
        assert_eq!(sel_or.select_atom_ids(&group), vec![2, 4, 7, 9]);

        // Boolean: Not(Carbon) within ALA
        let sel_not_carbon = Selector::And(vec![
            sel_ala,
            Selector::Not {
                inner: Box::new(sel_carbon),
            },
        ]);
        // ALAの全5原子のうちC以外: N(1), O(4)
        assert_eq!(sel_not_carbon.select_atom_ids(&group), vec![1, 4]);
    }

    #[test]
    fn test_path_regex() {
        let (group, _sec_map) = AtomGroup::create_test_peptide();
        let sel_regex = Selector::PathRegex(r".*/A/6/C.*".to_string());
        // A鎖残基6のCから始まる原子: C(8), CA(7)
        let matched = sel_regex.select_atom_ids(&group);
        assert_eq!(matched, vec![7, 8]);
    }

    #[test]
    fn test_residue_range_with_insertion_code() {
        // 挿入コード付き残基 "6A" を構築して ResidueRange のマッチングを検証
        let mut group = AtomGroup::with_name("InsCodeTest");
        let mut model = proteindf_bridge::AtomGroup::with_name("model_1");
        let mut chain = proteindf_bridge::AtomGroup::with_name("A");
        let mut res6a = proteindf_bridge::AtomGroup::with_name("HIS");
        let atom = Atom::new_with_pos("C", Position::new(0.0, 0.0, 0.0)).expect("valid atom");
        res6a.set_atom("CA", atom);
        chain.set_group("6A", res6a);
        model.set_group("A", chain);
        group.set_group("model_1", model);
        group.set_path("/".to_string());

        // start=6, end=6 の範囲に "6A"（residue_index=6）が含まれることを確認
        let sel_match = Selector::ResidueRange {
            chain: Some("A".to_string()),
            start: 6,
            end: 6,
        };
        assert_eq!(sel_match.select_atom_ids(&group), vec![1]);

        // 範囲外 (start=7, end=10) ではマッチしないことを確認
        let sel_no_match = Selector::ResidueRange {
            chain: Some("A".to_string()),
            start: 7,
            end: 10,
        };
        assert!(sel_no_match.select_atom_ids(&group).is_empty());
    }

    #[test]
    fn test_select_atom_ids_consistency_with_to_flat_molecule() {
        // 16残基合成タンパク質断片で、select_atom_ids と to_flat_molecule の ID 整合性を検証
        let (group, _) = AtomGroup::create_test_protein_fragment();
        let flat = group.to_flat_molecule();

        // 1. Selector::All の選択結果と平坦化分子の全IDが完全に一致すること
        let all_ids = Selector::All.select_atom_ids(&group);
        let flat_ids: Vec<u32> = flat.atoms.iter().map(|a| a.id).collect();
        assert_eq!(all_ids, flat_ids);

        // 2. 特定セレクタ（Name: CA）で選ばれたIDが、平坦化分子側でもCAであることを確認
        let ca_ids = Selector::Name(vec!["CA".to_string()]).select_atom_ids(&group);
        assert_eq!(ca_ids.len(), 16); // 16残基各1個
        for id in &ca_ids {
            let atom = flat
                .atoms
                .iter()
                .find(|a| a.id == *id)
                .expect("atom exists in flat");
            assert_eq!(atom.element, "C");
        }
    }
}
