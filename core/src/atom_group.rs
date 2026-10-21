//! `AtomGroup` implementation backed by `proteindf-bridge`.
//!
//! # Note on Phase 6e Architecture
//! This module re-exports `proteindf_bridge::AtomGroup` and provides extension methods
//! (`AtomGroupExt`) and protein-schema depth traversal helpers (`iter_models`, `iter_chains`,
//! `iter_residues`, `iter_atoms`).
//!
//! In accordance with the protein schema convention:
//! - Root group (`path: "/"`) contains models (e.g. key: `"model_1"`).
//! - Model group contains chains (e.g. key: `"A"`).
//! - Chain group contains residues (e.g. key: `"5"`, `"6"` with `name` set to residue type like `"ALA"`).
//! - Residue group contains atoms (e.g. key: `"CA"`, `"N"`).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};

use crate::state::{Atom as FlatAtom, Molecule};
use proteindf_bridge::{Atom, Position};

/// YUI における分子階層ツリー構造（`proteindf_bridge::AtomGroup` の newtype ラッパー）。
///
/// `Deref`/`DerefMut` により透過的に bridge 側の全 API（`groups()`, `atoms()`, `path()`,
/// `set_group()` 等）を直接利用可能です。また、`rmpv::Value`（`get_raw_data` / `from_dict_data`）を
/// 介して MessagePack および YAML へのロスレスな `Serialize` / `Deserialize` を提供します。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AtomGroup(pub proteindf_bridge::AtomGroup);

impl Deref for AtomGroup {
    type Target = proteindf_bridge::AtomGroup;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for AtomGroup {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<proteindf_bridge::AtomGroup> for AtomGroup {
    fn from(inner: proteindf_bridge::AtomGroup) -> Self {
        Self(inner)
    }
}

impl From<AtomGroup> for proteindf_bridge::AtomGroup {
    fn from(group: AtomGroup) -> Self {
        group.0
    }
}

impl AtomGroup {
    /// 新規の空 `AtomGroup` を作成する。
    pub fn new() -> Self {
        Self(proteindf_bridge::AtomGroup::new())
    }

    /// 指定された名前を持つ `AtomGroup` を作成する。
    pub fn with_name(name: &str) -> Self {
        Self(proteindf_bridge::AtomGroup::with_name(name))
    }

    /// 内部の `proteindf_bridge::AtomGroup` への参照を取得する。
    pub fn as_inner(&self) -> &proteindf_bridge::AtomGroup {
        &self.0
    }

    /// 内部の `proteindf_bridge::AtomGroup` の可変参照を取得する。
    pub fn as_inner_mut(&mut self) -> &mut proteindf_bridge::AtomGroup {
        &mut self.0
    }

    /// 内部の `proteindf_bridge::AtomGroup` を取り出す。
    pub fn into_inner(self) -> proteindf_bridge::AtomGroup {
        self.0
    }

    /// サブグループを設定する（`AtomGroup` または `proteindf_bridge::AtomGroup` を受け付け可能）。
    pub fn set_group(&mut self, key: &str, group: impl Into<proteindf_bridge::AtomGroup>) {
        self.0.set_group(key, group.into());
    }
}

impl Serialize for AtomGroup {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.get_raw_data().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for AtomGroup {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = rmpv::Value::deserialize(deserializer)?;
        let bridge_group = proteindf_bridge::AtomGroup::from_dict_data(&value)
            .map_err(serde::de::Error::custom)?;
        Ok(AtomGroup(bridge_group))
    }
}

/// 二次構造の種類（DSSP相当）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecondaryStructureKind {
    Helix,
    Sheet,
    Loop,
}

/// 一時的な代替実装: proteindf-bridgeのAtomGroupにsecondary_structureの
/// 専用フィールドが実装されるまでの間、residueのpath文字列をキーとして
/// 二次構造ラベルを保持するサイドマップ。
/// TODO: RUST_PORT_SPEC.md §9の要求（bondsと同格の専用フィールド）が
/// bridge側で実装され次第、このマップを廃止してツリーから直接読むようにする。
pub type SecondaryStructureMap = HashMap<String, SecondaryStructureKind>;

/// ルートAtomGroupの直下の子（＝モデル）を列挙する。
pub fn iter_models(
    root: &proteindf_bridge::AtomGroup,
) -> impl Iterator<Item = (&str, &proteindf_bridge::AtomGroup)> {
    root.groups().map(|(k, v)| (k.as_str(), v))
}

/// モデルレベルのAtomGroupの直下の子（＝鎖）を列挙する。
pub fn iter_chains(
    model: &proteindf_bridge::AtomGroup,
) -> impl Iterator<Item = (&str, &proteindf_bridge::AtomGroup)> {
    model.groups().map(|(k, v)| (k.as_str(), v))
}

/// 鎖レベルのAtomGroupの直下の子（＝残基）を列挙する。
pub fn iter_residues(
    chain: &proteindf_bridge::AtomGroup,
) -> impl Iterator<Item = (&str, &proteindf_bridge::AtomGroup)> {
    chain.groups().map(|(k, v)| (k.as_str(), v))
}

/// 残基レベルのAtomGroupが直接持つ原子を列挙する（`.atoms()`のシンプルな別名）。
pub fn iter_atoms(residue: &proteindf_bridge::AtomGroup) -> impl Iterator<Item = (&str, &Atom)> {
    residue.atoms().map(|(k, v)| (k.as_str(), v))
}

/// 残基キー文字列（例: "6", "52A"）を残基番号と挿入コードに分解する。
/// 末尾が英字1文字かつ残りが数値の場合は挿入コードとして分離する。
/// パース失敗時や非数値文字列の場合は (0, None) にフォールバックする。
pub fn parse_residue_key(key: &str) -> (i32, Option<char>) {
    if let Ok(idx) = key.parse::<i32>() {
        return (idx, None);
    }
    if let Some(last) = key.chars().last()
        && last.is_ascii_alphabetic()
    {
        let rest = &key[..key.len() - last.len_utf8()];
        if let Ok(idx) = rest.parse::<i32>() {
            return (idx, Some(last));
        }
    }
    (0, None)
}

/// 平坦化走査において各原子とその階層メタデータを参照する構造体。
#[derive(Debug, Clone)]
pub struct FlatAtomRef<'a> {
    pub id: u32,
    pub model_key: &'a str,
    pub chain_key: &'a str,
    pub residue_key: &'a str,
    pub residue_name: &'a str,
    pub residue_index: i32,
    pub insertion_code: Option<char>,
    pub atom_key: &'a str,
    pub atom: &'a Atom,
}

/// `iter_models → iter_chains → iter_residues → iter_atoms` の順に木を走査し、
/// 出現順に1始まりの連番IDを割り当てながら列挙する。
///
/// `to_flat_molecule()` と `Selector::select_atom_ids()` はこの関数を共有し、
/// 走査順・ID採番方式の不整合を構造的に防ぐ。
pub fn iter_flat_atoms<'a>(
    root: &'a proteindf_bridge::AtomGroup,
) -> impl Iterator<Item = FlatAtomRef<'a>> {
    let mut flat_atoms = Vec::new();
    let mut atom_id = 1u32;
    for (model_key, model) in iter_models(root) {
        for (chain_key, chain) in iter_chains(model) {
            for (res_key, res) in iter_residues(chain) {
                let (residue_index, insertion_code) = parse_residue_key(res_key);
                let residue_name = if res.name.is_empty() {
                    res_key
                } else {
                    res.name.as_str()
                };
                for (atom_key, atom) in iter_atoms(res) {
                    flat_atoms.push(FlatAtomRef {
                        id: atom_id,
                        model_key,
                        chain_key,
                        residue_key: res_key,
                        residue_name,
                        residue_index,
                        insertion_code,
                        atom_key,
                        atom,
                    });
                    atom_id += 1;
                }
            }
        }
    }
    flat_atoms.into_iter()
}

/// `iter_flat_atoms` と同じ順（モデル → 鎖 → 残基 → 原子）に、原子の座標を差し替える。
///
/// 軌跡のフレームの座標を、構造の木（`AtomGroup`）に反映するために使う。`positions` の長さは、
/// 原子の総数と同じでなければならない（違うときは、何も変えずにエラーを返す）。
pub fn set_flat_positions(
    root: &mut proteindf_bridge::AtomGroup,
    positions: &[[f32; 3]],
) -> Result<(), String> {
    let total = iter_flat_atoms(root).count();
    if total != positions.len() {
        return Err(format!(
            "The frame has {} atoms, but the structure has {total}",
            positions.len()
        ));
    }
    let mut next = 0usize;
    for (_, model) in root.groups_mut() {
        for (_, chain) in model.groups_mut() {
            for (_, residue) in chain.groups_mut() {
                for (_, atom) in residue.atoms_mut() {
                    let p = positions[next];
                    atom.xyz.x = f64::from(p[0]);
                    atom.xyz.y = f64::from(p[1]);
                    atom.xyz.z = f64::from(p[2]);
                    next += 1;
                }
            }
        }
    }
    Ok(())
}

/// 各モデルに属する原子 ID の範囲（開始ID..=終了ID、1始まり、連続）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelAtomRange {
    pub model_key: String,
    pub start_id: u32,
    pub end_id: u32,
}

impl ModelAtomRange {
    /// 指定された原子IDがこのモデルの範囲に含まれるかを判定する。
    #[inline]
    pub fn contains(&self, atom_id: u32) -> bool {
        atom_id >= self.start_id && atom_id <= self.end_id
    }

    /// このモデルに含まれる原子数。
    #[inline]
    pub fn atom_count(&self) -> usize {
        if self.end_id >= self.start_id {
            (self.end_id - self.start_id + 1) as usize
        } else {
            0
        }
    }
}

/// ルート `AtomGroup` から各モデルの原子 ID 範囲を算出する。
///
/// `iter_flat_atoms` と完全に一致する走査順序（モデル順・連番ID）で各モデルの開始ID・終了IDを
/// 1度だけ計算して返します。構造読み込み時に一度だけ呼び出して結果をキャッシュすることで、
/// 毎フレームの描画処理で全原子を走査することなく O(モデル数) で所属モデルを判定できます。
pub fn calculate_model_atom_ranges(root: &proteindf_bridge::AtomGroup) -> Vec<ModelAtomRange> {
    let mut ranges = Vec::new();
    let mut current_id = 1u32;

    for (model_key, model) in iter_models(root) {
        let start_id = current_id;
        for (_chain_key, chain) in iter_chains(model) {
            for (_res_key, res) in iter_residues(chain) {
                current_id += iter_atoms(res).count() as u32;
            }
        }
        if current_id > start_id {
            ranges.push(ModelAtomRange {
                model_key: model_key.to_string(),
                start_id,
                end_id: current_id - 1,
            });
        }
    }

    ranges
}

/// `proteindf_bridge::AtomGroup` に対するYUI向け拡張トレイト。
///
/// 外部クレートの型に対する orphan rule を回避し、テストフィクスチャ生成および
/// レンダリング用平坦化キャッシュ（`Molecule`）派生ロジックを提供します。
pub trait AtomGroupExt: Sized {
    /// テスト用の簡易ペプチド構造フィクスチャ（Ala-Gly ジペプチド相当）と二次構造マップを生成する。
    fn create_test_peptide() -> (Self, SecondaryStructureMap);

    /// カートゥーン/リボン描画検証用の合成タンパク質断片フィクスチャ（16残基）と二次構造マップを生成する。
    fn create_test_protein_fragment() -> (Self, SecondaryStructureMap);

    /// 大規模スケール検証用の合成タンパク質格子フィクスチャと二次構造マップを生成する。
    fn create_test_protein_lattice(
        nx: usize,
        ny: usize,
        nz: usize,
    ) -> (Self, SecondaryStructureMap);

    /// 既定の大規模合成タンパク質格子フィクスチャ（6x6x4 = 144ユニット、9,216原子）を生成する。
    fn create_test_large_protein_lattice() -> (Self, SecondaryStructureMap) {
        Self::create_test_protein_lattice(6, 6, 4)
    }

    /// `Molecule`（レンダリング用の平坦化キャッシュ構造体）への単方向派生変換。
    fn to_flat_molecule(&self) -> Molecule;

    /// 全原子の総数を取得する。
    fn atom_count(&self) -> usize;
}

impl AtomGroupExt for AtomGroup {
    fn create_test_peptide() -> (Self, SecondaryStructureMap) {
        let mut group = AtomGroup::with_name("TestPeptide");
        let mut model = AtomGroup::with_name("model_1");
        let mut chain_a = AtomGroup::with_name("A");

        // 残基 5: ALA
        let mut res5 = AtomGroup::with_name("ALA");
        let atoms5: [(&str, &str, [f64; 3]); 5] = [
            ("N", "N", [0.0, 0.0, 0.0]),
            ("CA", "C", [1.45, 0.0, 0.0]),
            ("C", "C", [2.0, 1.4, 0.0]),
            ("O", "O", [1.3, 2.4, 0.0]),
            ("CB", "C", [2.0, -0.7, 1.2]),
        ];
        for (name, symbol, pos) in atoms5 {
            let mut atom = Atom::new_with_pos(symbol, Position::new(pos[0], pos[1], pos[2]))
                .expect("valid atom symbol");
            atom.name = name.to_string();
            res5.set_atom(name, atom);
        }
        chain_a.set_group("5", res5);

        // 残基 6: GLY
        let mut res6 = AtomGroup::with_name("GLY");
        let atoms6: [(&str, &str, [f64; 3]); 4] = [
            ("N", "N", [3.3, 1.5, 0.0]),
            ("CA", "C", [4.0, 2.8, 0.0]),
            ("C", "C", [5.5, 2.7, 0.0]),
            ("O", "O", [6.1, 1.6, 0.0]),
        ];
        for (name, symbol, pos) in atoms6 {
            let mut atom = Atom::new_with_pos(symbol, Position::new(pos[0], pos[1], pos[2]))
                .expect("valid atom symbol");
            atom.name = name.to_string();
            res6.set_atom(name, atom);
        }
        chain_a.set_group("6", res6);

        model.set_group("A", chain_a);
        group.set_group("model_1", model);
        group.set_path("/".to_string());

        let mut sec_map = SecondaryStructureMap::new();
        sec_map.insert("/model_1/A/5/".to_string(), SecondaryStructureKind::Helix);
        sec_map.insert("/model_1/A/6/".to_string(), SecondaryStructureKind::Loop);

        (group, sec_map)
    }

    fn create_test_protein_fragment() -> (Self, SecondaryStructureMap) {
        let mut group = AtomGroup::with_name("TestProteinFragment");
        let mut model = AtomGroup::with_name("model_1");
        let mut chain_a = AtomGroup::with_name("A");

        let residue_specs: [(i32, &'static str, SecondaryStructureKind, [f64; 3]); 16] = [
            (1, "ALA", SecondaryStructureKind::Helix, [2.30, 0.00, 0.0]),
            (2, "LEU", SecondaryStructureKind::Helix, [-0.40, 2.26, 1.5]),
            (3, "GLU", SecondaryStructureKind::Helix, [-2.16, -0.79, 3.0]),
            (4, "MET", SecondaryStructureKind::Helix, [1.15, -1.99, 4.5]),
            (5, "GLN", SecondaryStructureKind::Helix, [1.76, 1.48, 6.0]),
            (6, "LYS", SecondaryStructureKind::Helix, [-1.48, 1.76, 7.5]),
            (7, "GLY", SecondaryStructureKind::Loop, [-0.5, 3.5, 8.8]),
            (8, "PRO", SecondaryStructureKind::Loop, [1.2, 4.8, 9.8]),
            (9, "SER", SecondaryStructureKind::Loop, [3.2, 5.0, 10.5]),
            (10, "ASN", SecondaryStructureKind::Loop, [5.1, 4.2, 11.0]),
            (11, "VAL", SecondaryStructureKind::Sheet, [7.2, 3.4, 11.5]),
            (12, "ILE", SecondaryStructureKind::Sheet, [9.5, 4.2, 12.2]),
            (13, "THR", SecondaryStructureKind::Sheet, [11.8, 3.5, 12.8]),
            (14, "TYR", SecondaryStructureKind::Sheet, [14.1, 4.3, 13.5]),
            (15, "PHE", SecondaryStructureKind::Sheet, [16.4, 3.6, 14.1]),
            (16, "TRP", SecondaryStructureKind::Sheet, [18.7, 4.4, 14.8]),
        ];

        let mut sec_map = SecondaryStructureMap::new();

        for (res_id, name, sec_struct, ca_pos) in residue_specs {
            let mut res = AtomGroup::with_name(name);

            // N
            let mut atom_n = Atom::new_with_pos(
                "N",
                Position::new(ca_pos[0] - 0.6, ca_pos[1] - 0.7, ca_pos[2] - 0.3),
            )
            .expect("valid atom symbol");
            atom_n.name = "N".to_string();
            res.set_atom("N", atom_n);

            // CA
            let mut atom_ca =
                Atom::new_with_pos("C", Position::new(ca_pos[0], ca_pos[1], ca_pos[2]))
                    .expect("valid atom symbol");
            atom_ca.name = "CA".to_string();
            res.set_atom("CA", atom_ca);

            // C
            let mut atom_c = Atom::new_with_pos(
                "C",
                Position::new(ca_pos[0] + 0.7, ca_pos[1] + 0.5, ca_pos[2] + 0.2),
            )
            .expect("valid atom symbol");
            atom_c.name = "C".to_string();
            res.set_atom("C", atom_c);

            // O
            let mut atom_o = Atom::new_with_pos(
                "O",
                Position::new(ca_pos[0] + 1.2, ca_pos[1] + 0.3, ca_pos[2] + 0.9),
            )
            .expect("valid atom symbol");
            atom_o.name = "O".to_string();
            res.set_atom("O", atom_o);

            let res_key = res_id.to_string();
            chain_a.set_group(&res_key, res);
            sec_map.insert(format!("/model_1/A/{res_key}/"), sec_struct);
        }

        model.set_group("A", chain_a);
        group.set_group("model_1", model);
        group.set_path("/".to_string());

        (group, sec_map)
    }

    fn create_test_protein_lattice(
        nx: usize,
        ny: usize,
        nz: usize,
    ) -> (Self, SecondaryStructureMap) {
        let mut group = AtomGroup::with_name("TestProteinLattice");
        let mut model = AtomGroup::with_name("model_1");

        let residue_specs: [(i32, &'static str, SecondaryStructureKind, [f64; 3]); 16] = [
            (1, "ALA", SecondaryStructureKind::Helix, [2.30, 0.00, 0.0]),
            (2, "LEU", SecondaryStructureKind::Helix, [-0.40, 2.26, 1.5]),
            (3, "GLU", SecondaryStructureKind::Helix, [-2.16, -0.79, 3.0]),
            (4, "MET", SecondaryStructureKind::Helix, [1.15, -1.99, 4.5]),
            (5, "GLN", SecondaryStructureKind::Helix, [1.76, 1.48, 6.0]),
            (6, "LYS", SecondaryStructureKind::Helix, [-1.48, 1.76, 7.5]),
            (7, "GLY", SecondaryStructureKind::Loop, [-0.5, 3.5, 8.8]),
            (8, "PRO", SecondaryStructureKind::Loop, [1.2, 4.8, 9.8]),
            (9, "SER", SecondaryStructureKind::Loop, [3.2, 5.0, 10.5]),
            (10, "ASN", SecondaryStructureKind::Loop, [5.1, 4.2, 11.0]),
            (11, "VAL", SecondaryStructureKind::Sheet, [7.2, 3.4, 11.5]),
            (12, "ILE", SecondaryStructureKind::Sheet, [9.5, 4.2, 12.2]),
            (13, "THR", SecondaryStructureKind::Sheet, [11.8, 3.5, 12.8]),
            (14, "TYR", SecondaryStructureKind::Sheet, [14.1, 4.3, 13.5]),
            (15, "PHE", SecondaryStructureKind::Sheet, [16.4, 3.6, 14.1]),
            (16, "TRP", SecondaryStructureKind::Sheet, [18.7, 4.4, 14.8]),
        ];

        let mut sec_map = SecondaryStructureMap::new();
        let mut chain_index = 0;

        for ix in 0..nx {
            for iy in 0..ny {
                for iz in 0..nz {
                    let offset = [ix as f64 * 30.0, iy as f64 * 20.0, iz as f64 * 25.0];
                    let chain_id = format!("C_{chain_index}");
                    chain_index += 1;
                    let mut chain = AtomGroup::with_name(&chain_id);

                    for (res_id, name, sec_struct, ca_pos) in residue_specs {
                        let mut res = AtomGroup::with_name(name);

                        let shifted_ca = [
                            ca_pos[0] + offset[0],
                            ca_pos[1] + offset[1],
                            ca_pos[2] + offset[2],
                        ];

                        // N
                        let mut atom_n = Atom::new_with_pos(
                            "N",
                            Position::new(
                                shifted_ca[0] - 0.6,
                                shifted_ca[1] - 0.7,
                                shifted_ca[2] - 0.3,
                            ),
                        )
                        .expect("valid atom symbol");
                        atom_n.name = "N".to_string();
                        res.set_atom("N", atom_n);

                        // CA
                        let mut atom_ca = Atom::new_with_pos(
                            "C",
                            Position::new(shifted_ca[0], shifted_ca[1], shifted_ca[2]),
                        )
                        .expect("valid atom symbol");
                        atom_ca.name = "CA".to_string();
                        res.set_atom("CA", atom_ca);

                        // C
                        let mut atom_c = Atom::new_with_pos(
                            "C",
                            Position::new(
                                shifted_ca[0] + 0.7,
                                shifted_ca[1] + 0.5,
                                shifted_ca[2] + 0.2,
                            ),
                        )
                        .expect("valid atom symbol");
                        atom_c.name = "C".to_string();
                        res.set_atom("C", atom_c);

                        // O
                        let mut atom_o = Atom::new_with_pos(
                            "O",
                            Position::new(
                                shifted_ca[0] + 1.2,
                                shifted_ca[1] + 0.3,
                                shifted_ca[2] + 0.9,
                            ),
                        )
                        .expect("valid atom symbol");
                        atom_o.name = "O".to_string();
                        res.set_atom("O", atom_o);

                        let res_key = res_id.to_string();
                        chain.set_group(&res_key, res);
                        sec_map.insert(format!("/model_1/{chain_id}/{res_key}/"), sec_struct);
                    }

                    model.set_group(&chain_id, chain);
                }
            }
        }

        group.set_group("model_1", model);
        group.set_path("/".to_string());

        (group, sec_map)
    }

    fn to_flat_molecule(&self) -> Molecule {
        let mut flat_atoms = Vec::new();
        let mut atom_path_to_id = HashMap::new();

        for flat in iter_flat_atoms(self) {
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
            let color = default_element_color(symbol);
            let radius = default_element_radius(symbol);

            flat_atoms.push(FlatAtom {
                id: flat.id,
                element: symbol.to_string(),
                position: [
                    flat.atom.xyz.x as f32,
                    flat.atom.xyz.y as f32,
                    flat.atom.xyz.z as f32,
                ],
                radius,
                color,
                residue_name: flat.residue_name.to_string(),
                residue_index: flat.residue_index,
                chain_id: flat.chain_key.to_string(),
            });
            atom_path_to_id.insert(flat.atom.path.clone(), flat.id);
        }

        let mut bonded_clone = self.0.clone();
        let bonds = match bonded_clone.setup() {
            Ok(()) => {
                let mut records = Vec::new();
                collect_bond_records(&bonded_clone, &mut records);
                records
                    .into_iter()
                    .filter_map(|(a1, a2, order)| {
                        let id1 = atom_path_to_id.get(&a1.path)?;
                        let id2 = atom_path_to_id.get(&a2.path)?;
                        Some(crate::state::Bond {
                            atom_a: *id1,
                            atom_b: *id2,
                            order: order as f32,
                        })
                    })
                    .collect()
            }
            Err(e) => {
                log::warn!("Bond detection failed (falling back to no bonds): {e}");
                Vec::new()
            }
        };

        Molecule {
            atoms: flat_atoms,
            bonds,
        }
    }

    fn atom_count(&self) -> usize {
        self.get_number_of_all_atoms()
    }
}

/// 木全体（ルート、モデル、鎖、残基など全階層）から定義された結合レコードを再帰的に走査し、
/// 両端の原子参照を解決して収集するヘルパー。
///
/// `add_bond()` は2原子の最も近い共通祖先グループに格納されるため、
/// 残基内の結合（残基レベル）だけでなく、隣接残基間の主鎖ペプチド結合（鎖レベル）などを
/// 漏れなく収集するために全階層の走査が必須となる。
fn collect_bond_records<'a>(
    group: &'a proteindf_bridge::AtomGroup,
    out: &mut Vec<(
        &'a proteindf_bridge::Atom,
        &'a proteindf_bridge::Atom,
        usize,
    )>,
) {
    for record in group.bonds() {
        if let Some((a1, a2)) = group.resolve_bond(record) {
            out.push((a1, a2, record.order));
        }
    }
    for (_key, child) in group.groups() {
        collect_bond_records(child, out);
    }
}

/// 元素別の既定色（RGBA）
fn default_element_color(element: &str) -> [f32; 4] {
    match element.to_uppercase().as_str() {
        "C" => [0.5, 0.5, 0.5, 1.0], // グレー
        "N" => [0.2, 0.2, 0.9, 1.0], // 青
        "O" => [0.9, 0.2, 0.2, 1.0], // 赤
        "S" => [0.9, 0.9, 0.2, 1.0], // 黄
        "H" => [0.9, 0.9, 0.9, 1.0], // 白
        _ => [0.7, 0.5, 0.7, 1.0],   // その他
    }
}

/// 元素別の既定ファンデルワールス半径（Å）
fn default_element_radius(element: &str) -> f32 {
    match element.to_uppercase().as_str() {
        "H" => 1.20,
        "C" => 1.70,
        "N" => 1.55,
        "O" => 1.52,
        "S" => 1.80,
        "P" => 1.80,
        _ => 1.50,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atom_group_fixture_and_flatten() {
        let (group, sec_map) = AtomGroup::create_test_peptide();
        assert_eq!(group.atom_count(), 9);
        assert_eq!(sec_map.len(), 2);
        assert_eq!(
            sec_map.get("/model_1/A/5/"),
            Some(&SecondaryStructureKind::Helix)
        );
        assert_eq!(
            sec_map.get("/model_1/A/6/"),
            Some(&SecondaryStructureKind::Loop)
        );

        let molecule = group.to_flat_molecule();
        assert_eq!(molecule.atoms.len(), 9);
        assert_eq!(molecule.atoms[0].element, "N");
        assert_eq!(molecule.atoms[0].residue_name, "ALA");
        assert_eq!(molecule.atoms[0].chain_id, "A");
        assert_eq!(molecule.atoms[5].residue_name, "GLY");
        assert_eq!(molecule.atoms[5].residue_index, 6);
    }

    #[test]
    fn test_atom_group_protein_fragment_fixture() {
        let (group, sec_map) = AtomGroup::create_test_protein_fragment();
        assert_eq!(group.get_number_of_groups(), 1); // model_1
        assert_eq!(group.atom_count(), 16 * 4);
        assert_eq!(sec_map.len(), 16);

        // 二次構造ラベルの検証
        assert_eq!(
            sec_map.get("/model_1/A/1/"),
            Some(&SecondaryStructureKind::Helix)
        );
        assert_eq!(
            sec_map.get("/model_1/A/6/"),
            Some(&SecondaryStructureKind::Helix)
        );
        assert_eq!(
            sec_map.get("/model_1/A/7/"),
            Some(&SecondaryStructureKind::Loop)
        );
        assert_eq!(
            sec_map.get("/model_1/A/10/"),
            Some(&SecondaryStructureKind::Loop)
        );
        assert_eq!(
            sec_map.get("/model_1/A/11/"),
            Some(&SecondaryStructureKind::Sheet)
        );
        assert_eq!(
            sec_map.get("/model_1/A/16/"),
            Some(&SecondaryStructureKind::Sheet)
        );

        let flat = group.to_flat_molecule();
        assert_eq!(flat.atoms.len(), 64);
    }

    #[test]
    fn test_atom_group_protein_lattice_fixture() {
        // 小規模格子: 2x2x2 = 8ユニット = 512原子
        let (small_lattice, sec_map_small) = AtomGroup::create_test_protein_lattice(2, 2, 2);
        assert_eq!(small_lattice.get_number_of_groups(), 1);
        let model = small_lattice.get_group("model_1").expect("model_1 exists");
        assert_eq!(model.get_number_of_groups(), 8);
        assert_eq!(small_lattice.atom_count(), 512);
        assert_eq!(sec_map_small.len(), 8 * 16);

        let flat_small = small_lattice.to_flat_molecule();
        assert_eq!(flat_small.atoms.len(), 512);
        // 原子のIDが一意で連番であることを確認
        assert_eq!(flat_small.atoms[0].id, 1);
        assert_eq!(flat_small.atoms[511].id, 512);

        // 既定の大規模格子: 6x6x4 = 144ユニット = 9,216原子
        let (large_lattice, sec_map_large) = AtomGroup::create_test_large_protein_lattice();
        let model_large = large_lattice.get_group("model_1").expect("model_1 exists");
        assert_eq!(model_large.get_number_of_groups(), 144);
        assert_eq!(large_lattice.atom_count(), 9216);
        assert_eq!(sec_map_large.len(), 144 * 16);
    }

    #[test]
    fn test_atom_group_yaml_and_msgpack_round_trip() {
        let (group, _) = AtomGroup::create_test_peptide();

        // 順序に依存せず原子集合の内容（鎖, 残基番号, 残基名, 元素, 座標）を比較するためのヘルパー
        fn atom_signature(flat: &Molecule) -> Vec<(String, i32, String, String, [i64; 3])> {
            let mut list: Vec<_> = flat
                .atoms
                .iter()
                .map(|a| {
                    (
                        a.chain_id.clone(),
                        a.residue_index,
                        a.residue_name.clone(),
                        a.element.clone(),
                        // 浮動小数点数を安全に比較・ソートするために丸める
                        [
                            (a.position[0] * 1000.0).round() as i64,
                            (a.position[1] * 1000.0).round() as i64,
                            (a.position[2] * 1000.0).round() as i64,
                        ],
                    )
                })
                .collect();
            list.sort();
            list
        }

        let orig_sig = atom_signature(&group.to_flat_molecule());

        // 1. YAML ラウンドトリップ検証
        let yaml = serde_yaml::to_string(&group).expect("serialize AtomGroup to YAML");
        let from_yaml: AtomGroup =
            serde_yaml::from_str(&yaml).expect("deserialize AtomGroup from YAML");
        assert_eq!(group.atom_count(), from_yaml.atom_count());
        let yaml_sig = atom_signature(&from_yaml.to_flat_molecule());
        assert_eq!(orig_sig, yaml_sig);

        // 2. MessagePack ラウンドトリップ検証
        let packed = rmp_serde::to_vec(&group).expect("serialize AtomGroup to MessagePack");
        let from_msgpack: AtomGroup =
            rmp_serde::from_slice(&packed).expect("deserialize AtomGroup from MessagePack");
        assert_eq!(group.atom_count(), from_msgpack.atom_count());
        let msgpack_sig = atom_signature(&from_msgpack.to_flat_molecule());
        assert_eq!(orig_sig, msgpack_sig);
    }

    #[test]
    fn test_parse_residue_key() {
        assert_eq!(parse_residue_key("6"), (6, None));
        assert_eq!(parse_residue_key("52A"), (52, Some('A')));
        assert_eq!(parse_residue_key("100b"), (100, Some('b')));
        assert_eq!(parse_residue_key(""), (0, None));
        assert_eq!(parse_residue_key("XYZ"), (0, None));
        assert_eq!(parse_residue_key("-5"), (-5, None));
    }

    #[test]
    fn test_to_flat_molecule_real_bonds_peptide() {
        let (group, _) = AtomGroup::create_test_peptide();
        let flat = group.to_flat_molecule();

        println!(
            "=== Generated Bonds for Peptide (total: {}) ===",
            flat.bonds.len()
        );
        for (i, b) in flat.bonds.iter().enumerate() {
            let a1 = flat.atoms.iter().find(|a| a.id == b.atom_a).unwrap();
            let a2 = flat.atoms.iter().find(|a| a.id == b.atom_b).unwrap();
            println!(
                "  [{}] id {}({}:{}) - id {}({}:{}) : order {}",
                i + 1,
                b.atom_a,
                a1.residue_name,
                a1.element,
                b.atom_b,
                a2.residue_name,
                a2.element,
                b.order
            );
        }

        // 1. 結合リストが非空であること
        assert!(!flat.bonds.is_empty(), "bonds must not be empty");

        // 2. 緩い健全性チェック: 総結合数は原子数(9)より少ないこと（旧バグの完全グラフ化=24結合の再発防止）
        assert!(
            flat.bonds.len() < flat.atoms.len(),
            "total bonds ({}) must be less than total atoms ({})",
            flat.bonds.len(),
            flat.atoms.len()
        );

        // 3. 残基内の共有結合（例: ALAの N(id=1) と CA(id=2)）が含まれること
        let has_intra_residue_bond = flat
            .bonds
            .iter()
            .any(|b| (b.atom_a == 1 && b.atom_b == 2) || (b.atom_a == 2 && b.atom_b == 1));
        assert!(
            has_intra_residue_bond,
            "bonds must contain intra-residue N-CA bond in ALA"
        );

        // 4. 残基間の主鎖ペプチド結合（ALAの C(id=3) と GLYの N(id=6)）が含まれること
        let has_peptide_backbone_bond = flat
            .bonds
            .iter()
            .any(|b| (b.atom_a == 3 && b.atom_b == 6) || (b.atom_a == 6 && b.atom_b == 3));
        assert!(
            has_peptide_backbone_bond,
            "bonds must contain inter-residue peptide backbone bond between ALA(C) and GLY(N)"
        );

        // 5. CCDテンプレート適用確認: C=O 二重結合（order == 2.0）の存在確認
        let ala_co_bond = flat
            .bonds
            .iter()
            .find(|b| (b.atom_a == 3 && b.atom_b == 4) || (b.atom_a == 4 && b.atom_b == 3));
        assert!(
            ala_co_bond.is_some(),
            "bonds must contain ALA C=O bond (id 3 - id 4)"
        );
        assert_eq!(
            ala_co_bond.unwrap().order,
            2.0,
            "ALA C=O bond order must be 2.0 (double bond from CCD template)"
        );

        let gly_co_bond = flat
            .bonds
            .iter()
            .find(|b| (b.atom_a == 8 && b.atom_b == 9) || (b.atom_a == 9 && b.atom_b == 8));
        assert!(
            gly_co_bond.is_some(),
            "bonds must contain GLY C=O bond (id 8 - id 9)"
        );
        assert_eq!(
            gly_co_bond.unwrap().order,
            2.0,
            "GLY C=O bond order must be 2.0 (double bond from CCD template)"
        );
    }

    #[test]
    fn test_set_flat_positions_updates_the_tree_in_flat_atom_order() {
        let (mut group, _) = AtomGroup::create_test_peptide();
        let n = iter_flat_atoms(&group).count();
        assert_eq!(n, 9);
        let new: Vec<[f32; 3]> = (0..n).map(|i| [i as f32, 10.0 + i as f32, -1.5]).collect();
        set_flat_positions(&mut group, &new).unwrap();

        // 平坦化したキャッシュの座標が、渡した座標と同じ順になる
        let flat = group.to_flat_molecule();
        for (i, a) in flat.atoms.iter().enumerate() {
            assert_eq!(a.position, new[i], "atom {}", a.id);
        }
        // 木の座標も同じ
        for (i, fa) in iter_flat_atoms(&group).enumerate() {
            assert_eq!(fa.atom.xyz.x as f32, new[i][0]);
            assert_eq!(fa.atom.xyz.y as f32, new[i][1]);
        }
    }

    #[test]
    fn test_set_flat_positions_rejects_a_wrong_atom_count_without_changing_anything() {
        let (mut group, _) = AtomGroup::create_test_peptide();
        let before = group.to_flat_molecule();
        let err = set_flat_positions(&mut group, &[[0.0; 3]; 3]).unwrap_err();
        assert!(err.contains("3 atoms") && err.contains("9"), "{err}");
        assert_eq!(group.to_flat_molecule().atoms, before.atoms);
    }
}
