use super::gpu_types::InstanceRaw;
use crate::math::{Mat4, vec3_normalize};
use yui_core::atom_group::{AtomGroup, iter_flat_atoms};
use yui_core::representation::{Representation, RepresentationKind};
use yui_core::spatial::AtomBvh;
use yui_core::state::Molecule;

/// 単位ベクトル u から v への 4x4 回転行列
pub(crate) fn rotation_between(u: [f32; 3], v: [f32; 3]) -> Mat4 {
    let dot = u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
    if dot > 0.99999 {
        #[rustfmt::skip]
        return [
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            0.0, 0.0, 0.0, 1.0,
        ];
    }
    if dot < -0.99999 {
        #[rustfmt::skip]
        return [
            -1.0, 0.0, 0.0, 0.0,
            0.0, -1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            0.0, 0.0, 0.0, 1.0,
        ];
    }

    let w = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];

    // Rodrigues' rotation formula
    let c = dot;
    let s = (1.0 - c * c).max(0.0).sqrt();
    let wn = vec3_normalize(w);

    let (x, y, z) = (wn[0], wn[1], wn[2]);
    let omc = 1.0 - c;

    [
        c + x * x * omc,
        y * x * omc + z * s,
        z * x * omc - y * s,
        0.0,
        x * y * omc - z * s,
        c + y * y * omc,
        z * y * omc + x * s,
        0.0,
        x * z * omc + y * s,
        y * z * omc - x * s,
        c + z * z * omc,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
    ]
}

/// AtomGroup から指定原子IDの原子名を取得するヘルパー
pub(crate) fn find_atom_name(atom_group: Option<&AtomGroup>, atom_id: u32) -> Option<String> {
    let ag = atom_group?;
    iter_flat_atoms(ag)
        .find(|fa| fa.id == atom_id)
        .map(|fa| fa.atom.name.clone())
}

/// 相互作用破線の円柱半径（Å）。共有結合棒（0.12Å）より細いが、既定の大きさでも見える太さ
pub const INTERACTION_STICK_RADIUS: f32 = 0.08;

/// 原子を球・スティックで描く表現の描き方（`Cpk`・`Licorice`・`Line` は `BallAndStick` と描き分ける）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum AtomStyle {
    /// 縮めた球（`radius * BALL_SCALE`）と灰色のスティック
    #[default]
    BallAndStick,
    /// ファンデルワールス半径の球。スティックなし
    Spacefill,
    /// 太いスティックだけ（結合は原子の色で半分ずつ塗る）。関節は同じ太さの球でなめらかにする
    Licorice,
    /// 細い線だけ（結合は原子の色で半分ずつ塗る）
    Line,
}

/// 描画の大きさの既定値。設定ファイルの `render:`（`user_settings::RenderSettings`）で変えられる。
/// ボール・スティックの球の半径は、ファンデルワールス半径のこの倍率
pub const BALL_SCALE: f32 = 0.35;
/// ボール・スティックのスティックの半径（Å）
pub const BALL_STICK_RADIUS: f32 = 0.12;
/// ライコリスのスティックの半径（Å）
pub const LICORICE_RADIUS: f32 = 0.2;
/// 線の太さ（円柱の半径、Å）
pub const LINE_RADIUS: f32 = 0.04;
/// 線表示の原子をクリックで拾うときの半径（Å）。線は細いので、太さより大きくとる
const LINE_PICK_RADIUS: f32 = 0.15;

impl AtomStyle {
    /// 原子を球・スティックで描く表現の種類なら、その描き方
    pub(crate) fn of_kind(kind: RepresentationKind) -> Option<Self> {
        match kind {
            RepresentationKind::BallAndStick => Some(Self::BallAndStick),
            RepresentationKind::Cpk => Some(Self::Spacefill),
            RepresentationKind::Licorice => Some(Self::Licorice),
            RepresentationKind::Line => Some(Self::Line),
            _ => None,
        }
    }

    /// 描く球の半径。球を描かないなら `None`
    pub(crate) fn sphere_radius(self, vdw_radius: f32) -> Option<f32> {
        match self {
            Self::BallAndStick => {
                Some(vdw_radius * crate::user_settings::startup_render().ball_scale)
            }
            Self::Spacefill => Some(vdw_radius),
            Self::Licorice => Some(crate::user_settings::startup_render().licorice_radius),
            Self::Line => None,
        }
    }

    /// クリックと選択の強調に使う半径（球を描かない表現でも、拾える大きさにする）
    pub(crate) fn pick_radius(self, vdw_radius: f32) -> f32 {
        self.sphere_radius(vdw_radius).unwrap_or(LINE_PICK_RADIUS)
    }

    /// 結合の円柱の半径。スティックを描かない表現なら `None`
    fn stick_radius(self) -> Option<f32> {
        match self {
            Self::BallAndStick => Some(crate::user_settings::startup_render().stick_radius),
            Self::Spacefill => None,
            Self::Licorice => Some(crate::user_settings::startup_render().licorice_radius),
            Self::Line => Some(crate::user_settings::startup_render().line_radius),
        }
    }
}

/// 表現ひとつぶんの、球・スティックで描く原子と、その色。
///
/// 表現はそれぞれ独立に描く（VMD と同じ）。同じ原子が複数の表現に入るときは、どれも重ねて描く
/// （例えば、スペースフィルの大きな球の中に、ボール・スティックが隠れる）。
pub(crate) struct StyleLayer<'a> {
    pub style: AtomStyle,
    pub atoms: Vec<&'a yui_core::state::Atom>,
    /// この表現の配色による原子の色（なければ元素色）
    pub colors: std::collections::HashMap<u32, [f32; 4]>,
}

/// 「隠す」カード（`hides_atoms`）が隠す原子ID（すべての表現に効く。ツリーの「Hide」がこれを使う）
pub(crate) fn hidden_atom_ids(
    atom_group: Option<&AtomGroup>,
    representations: &[Representation],
) -> std::collections::HashSet<u32> {
    let mut hidden = std::collections::HashSet::new();
    for r in representations {
        if !r.hides_atoms || !r.visible {
            continue;
        }
        if let Some(ag) = atom_group {
            hidden.extend(r.selection.select_atom_ids(ag));
        } else if let yui_core::selector::Selector::AtomIds(ids) = &r.selection {
            hidden.extend(ids.iter().cloned());
        }
    }
    hidden
}

/// 表示中の表現ごとの、球・スティックで描く原子と色。表現がひとつもないときは、全原子をボール・スティックで描く。
pub(crate) fn style_layers<'a>(
    molecule: &'a Molecule,
    atom_group: Option<&'a AtomGroup>,
    representations: &'a [Representation],
    active_model: Option<&str>,
    model_atom_ranges: &[yui_core::atom_group::ModelAtomRange],
) -> Vec<StyleLayer<'a>> {
    if representations.is_empty() {
        let (atoms, colors) =
            collect_active_atoms(molecule, atom_group, &[], active_model, model_atom_ranges);
        return vec![StyleLayer {
            style: AtomStyle::BallAndStick,
            atoms,
            colors,
        }];
    }
    let hidden = hidden_atom_ids(atom_group, representations);
    representations
        .iter()
        .filter(|r| r.visible && !r.hides_atoms)
        .filter_map(|r| {
            let style = AtomStyle::of_kind(r.kind)?;
            // この表現だけで、選んだ原子と色を求める（隠す原子は、全表現ぶんを別に引く）
            let (mut atoms, colors) = collect_active_atoms(
                molecule,
                atom_group,
                std::slice::from_ref(r),
                active_model,
                model_atom_ranges,
            );
            atoms.retain(|a| !hidden.contains(&a.id));
            Some(StyleLayer {
                style,
                atoms,
                colors,
            })
        })
        .collect()
}

/// クリックと選択の強調の対象になる原子と、その半径（複数の表現に入る原子は、いちばん大きい半径）。
/// 順序は、最初に現れた順。
pub(crate) fn pickable_atoms<'a>(
    layers: &[StyleLayer<'a>],
) -> Vec<(&'a yui_core::state::Atom, f32)> {
    let mut index: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    let mut out: Vec<(&'a yui_core::state::Atom, f32)> = Vec::new();
    for layer in layers {
        for &atom in &layer.atoms {
            let radius = layer.style.pick_radius(atom.radius);
            match index.get(&atom.id) {
                Some(&i) => out[i].1 = out[i].1.max(radius),
                None => {
                    index.insert(atom.id, out.len());
                    out.push((atom, radius));
                }
            }
        }
    }
    out
}

/// 現在の表現設定および選択中のモデルに基づいて描画・ピッキング対象となる原子リストとカスタム色を抽出
pub(crate) fn collect_active_atoms<'a>(
    molecule: &'a Molecule,
    atom_group: Option<&'a AtomGroup>,
    representations: &'a [Representation],
    active_model: Option<&str>,
    model_atom_ranges: &[yui_core::atom_group::ModelAtomRange],
) -> (
    Vec<&'a yui_core::state::Atom>,
    std::collections::HashMap<u32, [f32; 4]>,
) {
    let mut hidden_ids = std::collections::HashSet::new();
    let mut custom_colors = std::collections::HashMap::new();

    // 原子の所属（鎖ID・残基番号）を高速に引くためのマップ
    let atom_map: std::collections::HashMap<u32, &yui_core::state::Atom> =
        molecule.atoms.iter().map(|a| (a.id, a)).collect();

    // 鎖IDの一意な出現順リスト（一貫した鎖インデックス付け）
    let mut unique_chains: Vec<&str> = Vec::new();
    for a in &molecule.atoms {
        if !unique_chains.contains(&a.chain_id.as_str()) {
            unique_chains.push(&a.chain_id);
        }
    }

    // (鎖ID, 残基番号) の一意な出現順リスト（鎖をまたいだ残基番号の衝突を防ぐ）
    let mut unique_residues: Vec<(&str, i32)> = Vec::new();
    for a in &molecule.atoms {
        let key = (a.chain_id.as_str(), a.residue_index);
        if !unique_residues.contains(&key) {
            unique_residues.push(key);
        }
    }

    for r in representations {
        if r.kind == RepresentationKind::Interaction {
            continue;
        }
        if r.hides_atoms {
            if r.visible {
                if let Some(ag) = atom_group {
                    hidden_ids.extend(r.selection.select_atom_ids(ag));
                } else if let yui_core::selector::Selector::AtomIds(ids) = &r.selection {
                    hidden_ids.extend(ids.iter().cloned());
                }
            }
        } else if r.visible {
            let ids = if let Some(ag) = atom_group {
                r.selection.select_atom_ids(ag)
            } else if let yui_core::selector::Selector::AtomIds(ids) = &r.selection {
                ids.clone()
            } else {
                Vec::new()
            };
            for id in ids {
                match r.color {
                    yui_core::ColorScheme::Uniform(c) => {
                        custom_colors.insert(id, c);
                    }
                    yui_core::ColorScheme::Chain => {
                        if let Some(atom) = atom_map.get(&id) {
                            let chain_idx = unique_chains
                                .iter()
                                .position(|&c| c == atom.chain_id)
                                .unwrap_or(0);
                            custom_colors.insert(id, crate::ribbon::default_chain_color(chain_idx));
                        }
                    }
                    yui_core::ColorScheme::Residue => {
                        if let Some(atom) = atom_map.get(&id) {
                            let key = (atom.chain_id.as_str(), atom.residue_index);
                            let res_idx =
                                unique_residues.iter().position(|&k| k == key).unwrap_or(0);
                            custom_colors.insert(id, crate::ribbon::default_residue_color(res_idx));
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    let has_ball_and_stick = representations
        .iter()
        .any(|r| AtomStyle::of_kind(r.kind).is_some() && r.visible);

    let target_range =
        active_model.and_then(|model| model_atom_ranges.iter().find(|r| r.model_key == model));

    let in_active_model = |id: u32| -> bool {
        match (active_model, target_range) {
            (Some(_), Some(range)) => range.contains(id),
            (Some(_), None) => false,
            (None, _) => true,
        }
    };

    let atoms: Vec<&yui_core::state::Atom> = if representations.is_empty() {
        molecule
            .atoms
            .iter()
            .filter(|a| !hidden_ids.contains(&a.id) && in_active_model(a.id))
            .collect()
    } else if has_ball_and_stick {
        let mut matched_ids = std::collections::HashSet::new();
        for r in representations {
            if AtomStyle::of_kind(r.kind).is_some() && r.visible {
                if let Some(ag) = atom_group {
                    matched_ids.extend(r.selection.select_atom_ids(ag));
                } else if let yui_core::selector::Selector::AtomIds(ids) = &r.selection {
                    matched_ids.extend(ids.iter().cloned());
                }
            }
        }
        molecule
            .atoms
            .iter()
            .filter(|a| {
                matched_ids.contains(&a.id) && !hidden_ids.contains(&a.id) && in_active_model(a.id)
            })
            .collect()
    } else {
        molecule
            .atoms
            .iter()
            .filter(|a| !hidden_ids.contains(&a.id) && in_active_model(a.id))
            .collect()
    };

    (atoms, custom_colors)
}

/// 表示対象の原子集合と結合定義から結合スティック（円柱インスタンス）を生成する（全原子がボール・スティック）。
#[cfg(test)]
pub(crate) fn generate_stick_instances(
    bonds: &[yui_core::state::Bond],
    atoms_to_draw: &[&yui_core::state::Atom],
) -> Vec<InstanceRaw> {
    generate_styled_sticks(
        bonds,
        atoms_to_draw,
        AtomStyle::BallAndStick,
        &std::collections::HashMap::new(),
    )
}

/// スティック 1 本ぶん（両端、半径、色）。GPU のインスタンスにも、SVG の線にも使う。
pub(crate) struct StickSegment {
    pub start: [f32; 3],
    pub end: [f32; 3],
    pub radius: f32,
    pub color: [f32; 4],
}

/// ひとつの描き方（`style`）で、結合のスティック（両端・半径・色）を求める（`generate_styled_sticks` の元になる）。
///
/// `bonds`（`Molecule.bonds`）が非空の場合は、事前計算された実結合のうち両端が表示対象に含まれるものを
/// 描画する。`bonds` が空の場合は、空間分割BVHによる幾何ヒューリスティック（0.4〜1.9Å）にフォールバックする。
///
/// - ボール・スティック: 灰色の円柱 1 本。
/// - ライコリスと線: 結合の中点で 2 つに分け、原子ごとにその原子の色（`colors` の上書き、なければ元素色）。
/// - スペースフィルは、スティックを描かない。
pub(crate) fn styled_stick_segments(
    bonds: &[yui_core::state::Bond],
    atoms_to_draw: &[&yui_core::state::Atom],
    style: AtomStyle,
    colors: &std::collections::HashMap<u32, [f32; 4]>,
) -> Vec<StickSegment> {
    let bond_color = {
        let c = crate::user_settings::startup_render().bond_color;
        [c[0], c[1], c[2], 1.0]
    };

    let Some(radius) = style.stick_radius() else {
        return Vec::new();
    };

    // 結合の両端の原子（表示対象の中の組）
    let pairs: Vec<(&yui_core::state::Atom, &yui_core::state::Atom)> = if !bonds.is_empty() {
        // 事前計算された実結合を使用（表示中の原子集合にフィルタ）
        let by_id: std::collections::HashMap<u32, &yui_core::state::Atom> =
            atoms_to_draw.iter().map(|&a| (a.id, a)).collect();
        bonds
            .iter()
            .filter_map(|b| Some((*by_id.get(&b.atom_a)?, *by_id.get(&b.atom_b)?)))
            .collect()
    } else {
        // フォールバック: 空間分割BVHによる幾何ヒューリスティック（O(n log n)）
        let positions: Vec<[f32; 3]> = atoms_to_draw.iter().map(|a| a.position).collect();
        let bvh = AtomBvh::build(&positions);
        bvh.query_pairs_within_range(0.4, 1.9)
            .into_iter()
            .map(|(i, j)| (atoms_to_draw[i], atoms_to_draw[j]))
            .collect()
    };

    let mut sticks = Vec::with_capacity(pairs.len());
    for (a, b) in pairs {
        if style == AtomStyle::BallAndStick {
            sticks.push(StickSegment {
                start: a.position,
                end: b.position,
                radius,
                color: bond_color,
            });
            continue;
        }
        let mid = [
            (a.position[0] + b.position[0]) * 0.5,
            (a.position[1] + b.position[1]) * 0.5,
            (a.position[2] + b.position[2]) * 0.5,
        ];
        for atom in [a, b] {
            sticks.push(StickSegment {
                start: atom.position,
                end: mid,
                radius,
                color: colors.get(&atom.id).copied().unwrap_or(atom.color),
            });
        }
    }
    sticks
}

/// `styled_stick_segments` を GPU のインスタンス（円柱）にしたもの。
pub(crate) fn generate_styled_sticks(
    bonds: &[yui_core::state::Bond],
    atoms_to_draw: &[&yui_core::state::Atom],
    style: AtomStyle,
    colors: &std::collections::HashMap<u32, [f32; 4]>,
) -> Vec<InstanceRaw> {
    styled_stick_segments(bonds, atoms_to_draw, style, colors)
        .into_iter()
        .map(|s| InstanceRaw::from_cylinder(s.start, s.end, s.radius, s.color))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use yui_core::representation::{ColorScheme, Representation, RepresentationKind};
    use yui_core::selector::Selector;
    use yui_core::state::{Atom, Molecule};

    #[test]
    fn test_collect_active_atoms_chain_and_residue_colors() {
        // テスト用の分子: 鎖A(残基1, 残基2) と 鎖B(残基1)
        let atoms = vec![
            Atom {
                id: 1,
                element: "C".to_string(),
                position: [0.0, 0.0, 0.0],
                radius: 1.7,
                color: [0.5, 0.5, 0.5, 1.0],
                residue_name: "ALA".to_string(),
                residue_index: 1,
                chain_id: "A".to_string(),
            },
            Atom {
                id: 2,
                element: "N".to_string(),
                position: [1.0, 0.0, 0.0],
                radius: 1.55,
                color: [0.5, 0.5, 0.5, 1.0],
                residue_name: "ALA".to_string(),
                residue_index: 1,
                chain_id: "A".to_string(),
            },
            Atom {
                id: 3,
                element: "C".to_string(),
                position: [2.0, 0.0, 0.0],
                radius: 1.7,
                color: [0.5, 0.5, 0.5, 1.0],
                residue_name: "GLY".to_string(),
                residue_index: 2,
                chain_id: "A".to_string(),
            },
            Atom {
                id: 4,
                element: "C".to_string(),
                position: [3.0, 0.0, 0.0],
                radius: 1.7,
                color: [0.5, 0.5, 0.5, 1.0],
                residue_name: "VAL".to_string(),
                residue_index: 1,
                chain_id: "B".to_string(),
            },
        ];
        let molecule = Molecule {
            atoms,
            bonds: Vec::new(),
        };

        // 1. ColorScheme::Chain の検証
        let rep_chain = vec![Representation::new(
            "rep_chain",
            Selector::AtomIds(vec![1, 2, 3, 4]),
            RepresentationKind::BallAndStick,
            ColorScheme::Chain,
        )];
        let (_, colors_chain) = collect_active_atoms(&molecule, None, &rep_chain, None, &[]);

        let color_a1 = colors_chain.get(&1).unwrap();
        let color_a2 = colors_chain.get(&2).unwrap();
        let color_a3 = colors_chain.get(&3).unwrap();
        let color_b4 = colors_chain.get(&4).unwrap();

        // 同じ鎖Aの原子は同色
        assert_eq!(color_a1, color_a2);
        assert_eq!(color_a1, color_a3);
        // 異なる鎖Bの原子とは異なる色
        assert_ne!(color_a1, color_b4);
        assert_eq!(*color_a1, crate::ribbon::default_chain_color(0));
        assert_eq!(*color_b4, crate::ribbon::default_chain_color(1));

        // 2. ColorScheme::Residue の検証
        let rep_residue = vec![Representation::new(
            "rep_res",
            Selector::AtomIds(vec![1, 2, 3, 4]),
            RepresentationKind::BallAndStick,
            ColorScheme::Residue,
        )];
        let (_, colors_res) = collect_active_atoms(&molecule, None, &rep_residue, None, &[]);

        let color_r1_c = colors_res.get(&1).unwrap();
        let color_r1_n = colors_res.get(&2).unwrap();
        let color_r2_c = colors_res.get(&3).unwrap();
        let color_chainb_r1 = colors_res.get(&4).unwrap();

        // 同じ残基1内の原子は同色
        assert_eq!(color_r1_c, color_r1_n);
        // 異なる残基2の原子とは異なる色
        assert_ne!(color_r1_c, color_r2_c);
        // 鎖Bの残基1は、鎖Aの残基1と残基番号が同じでも異なる色（鎖をまたいだ衝突がないこと）
        assert_ne!(color_r1_c, color_chainb_r1);
    }

    #[test]
    fn test_generate_stick_instances_precalculated_vs_fallback() {
        use yui_core::state::Bond;

        let a1 = Atom {
            id: 1,
            element: "C".to_string(),
            position: [0.0, 0.0, 0.0],
            radius: 1.7,
            color: [1.0, 0.0, 0.0, 1.0],
            residue_name: "ALA".to_string(),
            residue_index: 1,
            chain_id: "A".to_string(),
        };
        let a2 = Atom {
            id: 2,
            element: "N".to_string(),
            position: [1.3, 0.0, 0.0],
            radius: 1.55,
            color: [0.0, 0.0, 1.0, 1.0],
            residue_name: "ALA".to_string(),
            residue_index: 1,
            chain_id: "A".to_string(),
        };
        let a3 = Atom {
            id: 3,
            element: "C".to_string(),
            position: [2.5, 0.0, 0.0],
            radius: 1.7,
            color: [0.0, 1.0, 0.0, 1.0],
            residue_name: "GLY".to_string(),
            residue_index: 2,
            chain_id: "A".to_string(),
        };

        // 1. 実結合（Bondリスト）が存在する場合のスティック生成
        let bonds = vec![
            Bond {
                atom_a: 1,
                atom_b: 2,
                order: 1.0,
            },
            Bond {
                atom_a: 2,
                atom_b: 3,
                order: 1.0,
            },
        ];

        // 全原子が表示対象の場合: 2本のスティックが生成される
        let all_atoms = vec![&a1, &a2, &a3];
        let sticks_all = generate_stick_instances(&bonds, &all_atoms);
        assert_eq!(sticks_all.len(), 2);

        // a3 を除外した場合: a2-a3 の結合はスキップされ、1本のみ生成される
        let partial_atoms = vec![&a1, &a2];
        let sticks_partial = generate_stick_instances(&bonds, &partial_atoms);
        assert_eq!(sticks_partial.len(), 1);

        // 2. 実結合が存在しない場合（フォールバック: BVH距離探索）
        let positions: Vec<[f32; 3]> = all_atoms.iter().map(|a| a.position).collect();
        let bvh = AtomBvh::build(&positions);
        let pairs = bvh.query_pairs_within_range(0.4, 1.9);
        assert_eq!(pairs.len(), 2); // a1-a2 (1.3) と a2-a3 (1.2) が閾値以内
    }

    #[test]
    fn test_collect_active_atoms_multi_model_filtering() {
        use yui_core::atom_group::calculate_model_atom_ranges;
        use yui_core::structure_loader::parse_structure_bytes;

        let cif_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/tests/data/1HLS.cif");
        let bytes = std::fs::read(cif_path).expect("failed to read 1HLS.cif");
        let (group, _sec_map, _interactions) =
            parse_structure_bytes(&bytes, "1HLS.cif").expect("parse 1HLS.cif");
        let molecule = yui_core::AtomGroupExt::to_flat_molecule(&group);
        let ranges = calculate_model_atom_ranges(&group);

        assert_eq!(molecule.atoms.len(), 15640);
        assert_eq!(ranges.len(), 20);

        // 1. active_model が None のとき: 全 15,640 原子が描画対象
        let (all_atoms, _) = collect_active_atoms(&molecule, Some(&group), &[], None, &ranges);
        assert_eq!(
            all_atoms.len(),
            15640,
            "None active_model should return all 15,640 atoms"
        );

        // 2. active_model が指定されたとき（第1モデル）: 該当モデルの 782 原子のみが描画対象
        let first_model = &ranges[0].model_key;
        let (model1_atoms, _) =
            collect_active_atoms(&molecule, Some(&group), &[], Some(first_model), &ranges);
        assert_eq!(
            model1_atoms.len(),
            782,
            "Specified active_model should filter to exactly 782 atoms"
        );

        // 描画対象の原子IDがすべてそのモデルの範囲（1..=782）に収まっていること
        for atom in &model1_atoms {
            assert!(
                ranges[0].contains(atom.id),
                "Atom {} must be within model range [{}, {}]",
                atom.id,
                ranges[0].start_id,
                ranges[0].end_id
            );
        }

        // 結合スティックもモデルの原子のみに絞り込まれること
        let sticks_model1 = generate_stick_instances(&molecule.bonds, &model1_atoms);
        let sticks_all = generate_stick_instances(&molecule.bonds, &all_atoms);
        assert!(
            sticks_model1.len() < sticks_all.len(),
            "Single model sticks ({}) must be fewer than all sticks ({})",
            sticks_model1.len(),
            sticks_all.len()
        );
    }

    fn styled_atom(id: u32, x: f32) -> Atom {
        Atom {
            id,
            element: "C".to_string(),
            position: [x, 0.0, 0.0],
            radius: 1.7,
            color: [1.0, 0.0, 0.0, 1.0],
            residue_name: "ALA".to_string(),
            residue_index: 1,
            chain_id: "A".to_string(),
        }
    }

    fn one_bond() -> Vec<yui_core::state::Bond> {
        vec![yui_core::state::Bond {
            atom_a: 1,
            atom_b: 2,
            order: 1.0,
        }]
    }

    fn color_of_instance(i: &InstanceRaw) -> [f32; 4] {
        i.color
    }

    /// 円柱インスタンスの半径（`from_cylinder` は、回転済みの x 軸に半径を掛けて入れる）
    fn radius_of_cylinder(i: &InstanceRaw) -> f32 {
        let m = i.model_0;
        (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt()
    }

    #[test]
    fn test_atom_style_of_kind_and_radii() {
        assert_eq!(
            AtomStyle::of_kind(RepresentationKind::BallAndStick),
            Some(AtomStyle::BallAndStick)
        );
        assert_eq!(
            AtomStyle::of_kind(RepresentationKind::Cpk),
            Some(AtomStyle::Spacefill)
        );
        assert_eq!(
            AtomStyle::of_kind(RepresentationKind::Licorice),
            Some(AtomStyle::Licorice)
        );
        assert_eq!(
            AtomStyle::of_kind(RepresentationKind::Line),
            Some(AtomStyle::Line)
        );
        assert_eq!(AtomStyle::of_kind(RepresentationKind::Cartoon), None);
        assert_eq!(AtomStyle::of_kind(RepresentationKind::Surface), None);
        assert_eq!(AtomStyle::of_kind(RepresentationKind::Interaction), None);

        let vdw = 1.7;
        assert_eq!(
            AtomStyle::BallAndStick.sphere_radius(vdw),
            Some(vdw * BALL_SCALE)
        );
        // スペースフィルはファンデルワールス半径そのもの（ボール・スティックより大きい）
        assert_eq!(AtomStyle::Spacefill.sphere_radius(vdw), Some(vdw));
        assert!(
            AtomStyle::Spacefill.sphere_radius(vdw) > AtomStyle::BallAndStick.sphere_radius(vdw)
        );
        assert_eq!(
            AtomStyle::Licorice.sphere_radius(vdw),
            Some(LICORICE_RADIUS)
        );
        // 線は球を描かないが、クリックで拾える大きさはある
        assert_eq!(AtomStyle::Line.sphere_radius(vdw), None);
        assert!(AtomStyle::Line.pick_radius(vdw) > LINE_RADIUS);
    }

    #[test]
    fn test_line_representation_selects_atoms_to_draw() {
        let molecule = Molecule {
            atoms: vec![styled_atom(1, 0.0), styled_atom(2, 1.4)],
            bonds: one_bond(),
        };
        let reps = vec![Representation::new(
            "l".to_string(),
            Selector::AtomIds(vec![1]),
            RepresentationKind::Line,
            ColorScheme::Element,
        )];
        let (atoms, _) = collect_active_atoms(&molecule, None, &reps, None, &[]);
        let ids: Vec<u32> = atoms.iter().map(|a| a.id).collect();
        assert_eq!(ids, vec![1]);
    }

    fn rep(id: &str, ids: Vec<u32>, kind: RepresentationKind, visible: bool) -> Representation {
        let mut r = Representation::new(
            id.to_string(),
            Selector::AtomIds(ids),
            kind,
            ColorScheme::Element,
        );
        r.visible = visible;
        r
    }

    #[test]
    fn test_style_layers_draw_each_representation_independently() {
        let molecule = Molecule {
            atoms: vec![
                styled_atom(1, 0.0),
                styled_atom(2, 1.4),
                styled_atom(3, 2.8),
            ],
            bonds: Vec::new(),
        };
        let reps = vec![
            rep("a", vec![1, 2, 3], RepresentationKind::Cpk, true),
            rep("b", vec![2, 3], RepresentationKind::BallAndStick, true),
            rep("c", vec![1], RepresentationKind::Line, false), // 非表示 → 自分だけ描かない
            rep("d", vec![1, 2], RepresentationKind::Cartoon, true), // 原子の表現ではない
        ];
        let layers = style_layers(&molecule, None, &reps, None, &[]);
        // 非表示の表現は層にならず、カートゥーンも層にならない。残る 2 つは独立に描く
        assert_eq!(layers.len(), 2);
        assert_eq!(layers[0].style, AtomStyle::Spacefill);
        assert_eq!(layers[1].style, AtomStyle::BallAndStick);
        // 非表示の表現は、ほかの表現の原子を隠さない
        let ids = |l: &StyleLayer| l.atoms.iter().map(|a| a.id).collect::<Vec<_>>();
        assert_eq!(ids(&layers[0]), vec![1, 2, 3]);
        assert_eq!(ids(&layers[1]), vec![2, 3]);
    }

    #[test]
    fn test_hide_card_hides_its_atoms_from_every_representation_until_turned_off() {
        let molecule = Molecule {
            atoms: vec![
                styled_atom(1, 0.0),
                styled_atom(2, 1.4),
                styled_atom(3, 2.8),
            ],
            bonds: Vec::new(),
        };
        let mut reps = vec![
            rep("a", vec![1, 2, 3], RepresentationKind::Cpk, true),
            rep("b", vec![2, 3], RepresentationKind::BallAndStick, true),
            rep("hide", vec![1, 2], RepresentationKind::BallAndStick, true).with_hides_atoms(true),
        ];
        let ids = |l: &StyleLayer| l.atoms.iter().map(|a| a.id).collect::<Vec<_>>();
        let layers = style_layers(&molecule, None, &reps, None, &[]);
        // 「隠す」カード自身は描かず、その原子はどの層からも消える
        assert_eq!(layers.len(), 2);
        assert_eq!(ids(&layers[0]), vec![3]);
        assert_eq!(ids(&layers[1]), vec![3]);
        // 目のアイコンで隠すのをやめると、元に戻る
        reps[2].visible = false;
        let layers = style_layers(&molecule, None, &reps, None, &[]);
        assert_eq!(layers.len(), 2);
        assert_eq!(ids(&layers[0]), vec![1, 2, 3]);
        assert_eq!(ids(&layers[1]), vec![2, 3]);
    }

    #[test]
    fn test_style_layers_without_representations_is_ball_and_stick() {
        let molecule = Molecule {
            atoms: vec![styled_atom(1, 0.0), styled_atom(2, 1.4)],
            bonds: Vec::new(),
        };
        let layers = style_layers(&molecule, None, &[], None, &[]);
        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].style, AtomStyle::BallAndStick);
        assert_eq!(layers[0].atoms.len(), 2);
    }

    #[test]
    fn test_style_layers_use_each_representations_own_colors() {
        let molecule = Molecule {
            atoms: vec![styled_atom(1, 0.0)],
            bonds: Vec::new(),
        };
        let mut spacefill = rep("a", vec![1], RepresentationKind::Cpk, true);
        spacefill.color = ColorScheme::Uniform([0.0, 1.0, 0.0, 1.0]);
        let ball = rep("b", vec![1], RepresentationKind::BallAndStick, true);
        let reps = [spacefill, ball];
        let layers = style_layers(&molecule, None, &reps, None, &[]);
        // 後の表現（元素色）が、先の表現（一様な緑）の色を上書きしない
        assert_eq!(layers[0].colors.get(&1), Some(&[0.0, 1.0, 0.0, 1.0]));
        assert!(layers[1].colors.is_empty());
    }

    #[test]
    fn test_pickable_atoms_take_the_largest_radius_of_the_layers() {
        let molecule = Molecule {
            atoms: vec![styled_atom(1, 0.0), styled_atom(2, 1.4)],
            bonds: Vec::new(),
        };
        let reps = vec![
            rep("a", vec![1, 2], RepresentationKind::BallAndStick, true),
            rep("b", vec![1], RepresentationKind::Cpk, true),
        ];
        let layers = style_layers(&molecule, None, &reps, None, &[]);
        let pickable = pickable_atoms(&layers);
        assert_eq!(pickable.len(), 2);
        // 原子 1 は、スペースフィルの球（ファンデルワールス半径）と同じ大きさ。順序は最初に現れた順
        assert_eq!(pickable[0].0.id, 1);
        assert_eq!(pickable[0].1, 1.7);
        assert_eq!(pickable[1].0.id, 2);
        assert_eq!(pickable[1].1, 1.7 * BALL_SCALE);
    }

    #[test]
    fn test_sticks_ball_and_stick_is_one_gray_cylinder() {
        let (a, b) = (styled_atom(1, 0.0), styled_atom(2, 1.4));
        let sticks = generate_styled_sticks(
            &one_bond(),
            &[&a, &b],
            AtomStyle::BallAndStick,
            &Default::default(),
        );
        assert_eq!(sticks.len(), 1);
        assert_eq!(color_of_instance(&sticks[0]), [0.7, 0.7, 0.7, 1.0]);
        assert!((radius_of_cylinder(&sticks[0]) - BALL_STICK_RADIUS).abs() < 1e-5);
    }

    #[test]
    fn test_sticks_spacefill_has_none_and_licorice_line_are_split_by_atom_color() {
        let (a, b) = (styled_atom(1, 0.0), styled_atom(2, 1.4));
        // スペースフィルは、スティックを描かない
        let none = generate_styled_sticks(
            &one_bond(),
            &[&a, &b],
            AtomStyle::Spacefill,
            &Default::default(),
        );
        assert!(none.is_empty());

        // ライコリスと線は、原子の色で半分ずつ（色の上書きも効く）
        let colors: std::collections::HashMap<u32, [f32; 4]> =
            [(2, [0.0, 0.0, 1.0, 1.0])].into_iter().collect();
        for style in [AtomStyle::Licorice, AtomStyle::Line] {
            let sticks = generate_styled_sticks(&one_bond(), &[&a, &b], style, &colors);
            assert_eq!(sticks.len(), 2, "{style:?}");
            assert_eq!(color_of_instance(&sticks[0]), a.color);
            let expected = if style == AtomStyle::Licorice {
                LICORICE_RADIUS
            } else {
                LINE_RADIUS
            };
            assert!((radius_of_cylinder(&sticks[0]) - expected).abs() < 1e-5);
            assert_eq!(color_of_instance(&sticks[1]), [0.0, 0.0, 1.0, 1.0]);
        }
    }

    #[test]
    fn test_sticks_without_bonds_use_the_distance_fallback() {
        let (a, b) = (styled_atom(1, 0.0), styled_atom(2, 1.4));
        let sticks =
            generate_styled_sticks(&[], &[&a, &b], AtomStyle::Licorice, &Default::default());
        assert_eq!(sticks.len(), 2);
    }
}
