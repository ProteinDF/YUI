//! リボン（カートゥーン）表示のメッシュ生成モジュール
//!
//! architecture.md 4.3節に準拠し、残基のCα座標列からCatmull-Romスプライン補間および
//! 二次構造（Helix/Sheet/Loop）に応じた断面プロファイルを適用して3Dリボンメッシュを生成する。

use bytemuck::{Pod, Zeroable};
use std::collections::{HashMap, HashSet};
use std::f32::consts::PI;
use yui_core::atom_group::{
    AtomGroup, SecondaryStructureKind, SecondaryStructureMap, iter_chains, iter_flat_atoms,
    iter_models, iter_residues,
};
use yui_core::representation::{ColorScheme, Representation, RepresentationKind};

/// リボンメッシュの頂点（位置 + 法線 + RGBAカラー）
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable, PartialEq)]
pub struct RibbonVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 4],
}

impl RibbonVertex {
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<RibbonVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                // 位置 (location 0)
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                // 法線 (location 1)
                wgpu::VertexAttribute {
                    offset: 12,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x3,
                },
                // カラー (location 2)
                wgpu::VertexAttribute {
                    offset: 24,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        }
    }
}

/// リボンメッシュ（頂点配列 + インデックス配列）
#[derive(Debug, Clone, Default)]
pub struct RibbonMesh {
    pub vertices: Vec<RibbonVertex>,
    pub indices: Vec<u32>,
}

/// 補間の入力となる残基情報
#[derive(Debug, Clone, PartialEq)]
pub struct RibbonResidue {
    pub ca_position: [f32; 3],
    pub secondary_structure: SecondaryStructureKind,
    pub color: [f32; 4],
}

/// 3次元ベクトルの加算
fn vec3_add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// 3次元ベクトルの減算
fn vec3_sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// 3次元ベクトルのスカラー倍
fn vec3_scale(v: [f32; 3], s: f32) -> [f32; 3] {
    [v[0] * s, v[1] * s, v[2] * s]
}

/// 3次元ベクトルのドット積
fn vec3_dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// 3次元ベクトルのクロス積
fn vec3_cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// 3次元ベクトルの正規化
fn vec3_normalize(v: [f32; 3]) -> [f32; 3] {
    let len = vec3_dot(v, v).sqrt();
    if len > 1e-6 {
        vec3_scale(v, 1.0 / len)
    } else {
        [0.0, 1.0, 0.0]
    }
}

/// 4点 p0, p1, p2, p3 による Catmull-Rom スプライン補間 (t in [0, 1])
pub fn catmull_rom(p0: [f32; 3], p1: [f32; 3], p2: [f32; 3], p3: [f32; 3], t: f32) -> [f32; 3] {
    let t2 = t * t;
    let t3 = t2 * t;

    let v0 = vec3_scale(p1, 2.0);
    let v1 = vec3_scale(vec3_sub(p2, p0), t);
    let v2 = vec3_scale(
        vec3_add(
            vec3_sub(vec3_scale(p0, 2.0), vec3_scale(p1, 5.0)),
            vec3_sub(vec3_scale(p2, 4.0), p3),
        ),
        t2,
    );
    let v3 = vec3_scale(
        vec3_add(
            vec3_sub(vec3_scale(p1, 3.0), p0),
            vec3_sub(p3, vec3_scale(p2, 3.0)),
        ),
        t3,
    );

    vec3_scale(vec3_add(vec3_add(v0, v1), vec3_add(v2, v3)), 0.5)
}

/// Catmull-Rom スプラインの接線ベクトル (d/dt)
pub fn catmull_rom_tangent(
    p0: [f32; 3],
    p1: [f32; 3],
    p2: [f32; 3],
    p3: [f32; 3],
    t: f32,
) -> [f32; 3] {
    let t2 = t * t;

    let v1 = vec3_sub(p2, p0);
    let v2 = vec3_scale(
        vec3_add(
            vec3_sub(vec3_scale(p0, 2.0), vec3_scale(p1, 5.0)),
            vec3_sub(vec3_scale(p2, 4.0), p3),
        ),
        2.0 * t,
    );
    let v3 = vec3_scale(
        vec3_add(
            vec3_sub(vec3_scale(p1, 3.0), p0),
            vec3_sub(p3, vec3_scale(p2, 3.0)),
        ),
        3.0 * t2,
    );

    let raw = vec3_scale(vec3_add(vec3_add(v1, v2), v3), 0.5);
    vec3_normalize(raw)
}

/// 二次構造に応じた断面プロファイル（半幅, 半厚）の取得
///
/// architecture.md 4.3節:
/// - Helix: 幅広の楕円断面
/// - Sheet: 平たいリボン断面
/// - Loop: 細い円筒チューブ断面
pub fn profile_dimensions(sec_struct: SecondaryStructureKind) -> (f32, f32) {
    // 既定値は、半幅・半厚が Helix (1.3, 0.35)、Sheet (1.2, 0.15)、Loop (0.25, 0.25)。
    // 設定（render:）には、幅と厚さ（全体の長さ）と、ループの半径で書く
    let r = crate::user_settings::startup_render();
    match sec_struct {
        SecondaryStructureKind::Helix => {
            (r.ribbon_helix_width / 2.0, r.ribbon_helix_thickness / 2.0)
        }
        SecondaryStructureKind::Sheet => {
            (r.ribbon_sheet_width / 2.0, r.ribbon_sheet_thickness / 2.0)
        }
        SecondaryStructureKind::Loop => (r.ribbon_loop_radius, r.ribbon_loop_radius),
    }
}

/// 二次構造の既定カラー（RGBA）
pub fn default_secondary_structure_color(sec_struct: SecondaryStructureKind) -> [f32; 4] {
    match sec_struct {
        SecondaryStructureKind::Helix => [0.85, 0.20, 0.50, 1.0], // マゼンタ・赤紫系
        SecondaryStructureKind::Sheet => [0.95, 0.80, 0.15, 1.0], // イエロー・ゴールド系
        SecondaryStructureKind::Loop => [0.20, 0.75, 0.35, 1.0],  // グリーン系
    }
}

/// 鎖インデックスに応じた既定カラー
pub fn default_chain_color(chain_idx: usize) -> [f32; 4] {
    let colors = [
        [0.25, 0.55, 0.85, 1.0], // 青
        [0.85, 0.40, 0.25, 1.0], // 橙
        [0.35, 0.75, 0.45, 1.0], // 緑
        [0.75, 0.35, 0.75, 1.0], // 紫
        [0.85, 0.75, 0.25, 1.0], // 黄
    ];
    colors[chain_idx % colors.len()]
}

/// 残基インデックスに応じた既定カラー（10色パレット）
pub const RESIDUE_PALETTE: [[f32; 4]; 10] = [
    [0.25, 0.55, 0.85, 1.0], // 青
    [0.85, 0.40, 0.25, 1.0], // 橙
    [0.35, 0.75, 0.45, 1.0], // 緑
    [0.75, 0.35, 0.75, 1.0], // 紫
    [0.85, 0.75, 0.25, 1.0], // 黄
    [0.20, 0.70, 0.80, 1.0], // シアン
    [0.90, 0.35, 0.55, 1.0], // ピンク
    [0.60, 0.80, 0.20, 1.0], // 黄緑
    [0.70, 0.50, 0.35, 1.0], // 茶
    [0.50, 0.50, 0.75, 1.0], // 青灰
];

pub fn default_residue_color(res_idx: usize) -> [f32; 4] {
    RESIDUE_PALETTE[res_idx % RESIDUE_PALETTE.len()]
}

/// 1つの鎖に属する連続残基列からリボンメッシュを生成する
pub fn build_ribbon_mesh_for_chain(residues: &[RibbonResidue]) -> RibbonMesh {
    let mut mesh = RibbonMesh::default();
    if residues.len() < 2 {
        return mesh;
    }

    let sub_steps = 8; // 1残基あたりの分割数
    let profile_segments = 8; // 断面の角数（円・楕円の分割数）
    let n_residues = residues.len();

    // 初期法線ベクトルの決定
    let initial_tangent = {
        let p0 = residues[0].ca_position;
        let p1 = residues[1].ca_position;
        vec3_normalize(vec3_sub(p1, p0))
    };

    let mut current_normal = {
        let up = if initial_tangent[1].abs() > 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        let side = vec3_cross(initial_tangent, up);
        vec3_normalize(vec3_cross(side, initial_tangent))
    };

    let mut ring_count = 0;

    for i in 0..(n_residues - 1) {
        let p0 = if i == 0 {
            residues[0].ca_position
        } else {
            residues[i - 1].ca_position
        };
        let p1 = residues[i].ca_position;
        let p2 = residues[i + 1].ca_position;
        let p3 = if i + 2 < n_residues {
            residues[i + 2].ca_position
        } else {
            residues[i + 1].ca_position
        };

        let res1 = &residues[i];
        let res2 = &residues[i + 1];

        // 各セグメント内の補間点を生成
        let steps = if i == n_residues - 2 {
            sub_steps + 1 // 最終セグメントは終点(t=1.0)も含める
        } else {
            sub_steps
        };

        for s in 0..steps {
            let t = s as f32 / sub_steps as f32;
            let center = catmull_rom(p0, p1, p2, p3, t);
            let tangent = catmull_rom_tangent(p0, p1, p2, p3, t);

            // Parallel Transport: 前の法線を現在の接線に直交射影してねじれを防ぐ
            let proj = vec3_scale(tangent, vec3_dot(current_normal, tangent));
            let ortho = vec3_sub(current_normal, proj);
            current_normal = vec3_normalize(ortho);
            let binormal = vec3_normalize(vec3_cross(tangent, current_normal));

            // 二次構造とプロファイル寸法の補間（前半はres1、後半はres2）
            let (sec_struct, color) = if t < 0.5 {
                (res1.secondary_structure, res1.color)
            } else {
                (res2.secondary_structure, res2.color)
            };

            let (half_w, half_h) = profile_dimensions(sec_struct);

            // 断面リングの頂点を生成
            for k in 0..profile_segments {
                let angle = (k as f32 / profile_segments as f32) * PI * 2.0;
                let cos_a = angle.cos();
                let sin_a = angle.sin();

                let offset = vec3_add(
                    vec3_scale(current_normal, half_w * cos_a),
                    vec3_scale(binormal, half_h * sin_a),
                );
                let pos = vec3_add(center, offset);

                let norm_unscaled = vec3_add(
                    vec3_scale(current_normal, cos_a / half_w),
                    vec3_scale(binormal, sin_a / half_h),
                );
                let norm = vec3_normalize(norm_unscaled);

                mesh.vertices.push(RibbonVertex {
                    position: pos,
                    normal: norm,
                    color,
                });
            }

            ring_count += 1;
        }
    }

    // リング間の三角形インデックス生成
    let segs = profile_segments as u32;
    for r in 0..(ring_count - 1) {
        let r1 = r as u32 * segs;
        let r2 = (r + 1) as u32 * segs;

        for k in 0..segs {
            let next_k = (k + 1) % segs;

            let v1 = r1 + k;
            let v2 = r2 + k;
            let v3 = r1 + next_k;
            let v4 = r2 + next_k;

            // 2つの三角形でクワッドを構成
            mesh.indices.push(v1);
            mesh.indices.push(v2);
            mesh.indices.push(v3);

            mesh.indices.push(v3);
            mesh.indices.push(v2);
            mesh.indices.push(v4);
        }
    }

    mesh
}

/// `AtomGroup` と `Representation` から描画対象の鎖別残基列を抽出する
pub fn extract_ribbon_chains(
    atom_group: &AtomGroup,
    representation: &Representation,
    sec_map: &SecondaryStructureMap,
    active_model: Option<&str>,
    hidden_atom_ids: &HashSet<u32>,
) -> Vec<Vec<RibbonResidue>> {
    if representation.kind != RepresentationKind::Cartoon || !representation.visible {
        return Vec::new();
    }

    let selected_atom_ids: HashSet<u32> = representation
        .selection
        .select_atom_ids(atom_group)
        .into_iter()
        .collect();

    let atom_path_to_id: HashMap<String, u32> = iter_flat_atoms(atom_group)
        .map(|fa| (fa.atom.path.clone(), fa.id))
        .collect();

    let mut chains_result = Vec::new();

    for (model_key, model) in iter_models(atom_group) {
        if let Some(active) = active_model
            && model_key != active
        {
            continue;
        }
        for (chain_idx, (chain_key, chain)) in iter_chains(model).enumerate() {
            let mut chain_residues = Vec::new();

            for (res_key, residue) in iter_residues(chain) {
                // CA 原子が選択集合に含まれている残基を対象とする
                if let Some(ca_atom) = residue.get_atom("CA")
                    && let Some(&atom_id) = atom_path_to_id.get(&ca_atom.path)
                    && selected_atom_ids.contains(&atom_id)
                {
                    // 「隠す」カードで CA が隠された残基は描かない。リボンが隠れた部分をまたいで
                    // つながらないよう、そこで鎖を区切る。
                    if hidden_atom_ids.contains(&atom_id) {
                        if !chain_residues.is_empty() {
                            chains_result.push(std::mem::take(&mut chain_residues));
                        }
                        continue;
                    }
                    let res_path = format!("/{model_key}/{chain_key}/{res_key}/");
                    let sec_struct = sec_map
                        .get(&res_path)
                        .copied()
                        .unwrap_or(SecondaryStructureKind::Loop);

                    let color = match &representation.color {
                        ColorScheme::SecondaryStructure => {
                            default_secondary_structure_color(sec_struct)
                        }
                        ColorScheme::Chain => default_chain_color(chain_idx),
                        ColorScheme::Uniform(c) => *c,
                        ColorScheme::Element | ColorScheme::Residue => {
                            // 元素または残基色はCAの規定色を使用
                            [0.6, 0.6, 0.6, 1.0]
                        }
                    };

                    chain_residues.push(RibbonResidue {
                        ca_position: [
                            ca_atom.xyz.x as f32,
                            ca_atom.xyz.y as f32,
                            ca_atom.xyz.z as f32,
                        ],
                        secondary_structure: sec_struct,
                        color,
                    });
                }
            }

            if !chain_residues.is_empty() {
                chains_result.push(chain_residues);
            }
        }
    }

    chains_result
}

/// `AtomGroup` と `[Representation]` から全カートゥーンメッシュを合成生成する
pub fn build_all_cartoons(
    atom_group: &AtomGroup,
    representations: &[Representation],
    sec_map: &SecondaryStructureMap,
    active_model: Option<&str>,
) -> RibbonMesh {
    let mut combined = RibbonMesh::default();
    let hidden = crate::viewport::scene::hidden_atom_ids(Some(atom_group), representations);

    for rep in representations {
        if rep.kind != RepresentationKind::Cartoon || !rep.visible || rep.hides_atoms {
            continue;
        }

        let chains = extract_ribbon_chains(atom_group, rep, sec_map, active_model, &hidden);
        for chain_residues in chains {
            let sub_mesh = build_ribbon_mesh_for_chain(&chain_residues);
            let base_index = combined.vertices.len() as u32;

            combined.vertices.extend(sub_mesh.vertices);
            combined
                .indices
                .extend(sub_mesh.indices.into_iter().map(|idx| idx + base_index));
        }
    }

    combined
}

#[cfg(test)]
mod tests {
    use super::*;
    use yui_core::AtomGroupExt;
    use yui_core::atom_group::AtomGroup;
    use yui_core::selector::Selector;

    #[test]
    fn test_catmull_rom_endpoints() {
        let p0 = [0.0, 0.0, 0.0];
        let p1 = [1.0, 2.0, 3.0];
        let p2 = [4.0, 5.0, 6.0];
        let p3 = [7.0, 8.0, 9.0];

        let start = catmull_rom(p0, p1, p2, p3, 0.0);
        assert!((start[0] - p1[0]).abs() < 1e-5);
        assert!((start[1] - p1[1]).abs() < 1e-5);
        assert!((start[2] - p1[2]).abs() < 1e-5);

        let end = catmull_rom(p0, p1, p2, p3, 1.0);
        assert!((end[0] - p2[0]).abs() < 1e-5);
        assert!((end[1] - p2[1]).abs() < 1e-5);
        assert!((end[2] - p2[2]).abs() < 1e-5);
    }

    #[test]
    fn test_catmull_rom_tangent_direction() {
        let p0 = [0.0, 0.0, 0.0];
        let p1 = [1.0, 0.0, 0.0];
        let p2 = [2.0, 0.0, 0.0];
        let p3 = [3.0, 0.0, 0.0];

        let tangent = catmull_rom_tangent(p0, p1, p2, p3, 0.5);
        assert!((tangent[0] - 1.0).abs() < 1e-4);
        assert!(tangent[1].abs() < 1e-4);
        assert!(tangent[2].abs() < 1e-4);
    }

    #[test]
    fn test_build_ribbon_mesh_empty_or_single() {
        assert_eq!(build_ribbon_mesh_for_chain(&[]).vertices.len(), 0);

        let single = vec![RibbonResidue {
            ca_position: [0.0, 0.0, 0.0],
            secondary_structure: SecondaryStructureKind::Helix,
            color: [1.0, 0.0, 0.0, 1.0],
        }];
        assert_eq!(build_ribbon_mesh_for_chain(&single).vertices.len(), 0);
    }

    #[test]
    fn test_build_ribbon_mesh_with_synthetic_fragment() {
        let (group, sec_map) = AtomGroup::create_test_protein_fragment();
        let rep = Representation::new(
            "cartoon_all",
            Selector::All,
            RepresentationKind::Cartoon,
            ColorScheme::SecondaryStructure,
        );

        let chains = extract_ribbon_chains(&group, &rep, &sec_map, None, &HashSet::new());
        assert_eq!(chains.len(), 1);
        assert_eq!(chains[0].len(), 16);

        let mesh = build_ribbon_mesh_for_chain(&chains[0]);
        assert!(!mesh.vertices.is_empty());
        assert!(!mesh.indices.is_empty());

        // インデックスが頂点範囲内に収まっていることの検証
        let max_vert = mesh.vertices.len() as u32;
        for &idx in &mesh.indices {
            assert!(idx < max_vert);
        }

        // NaN が含まれていないことの検証
        for v in &mesh.vertices {
            assert!(!v.position[0].is_nan());
            assert!(!v.position[1].is_nan());
            assert!(!v.position[2].is_nan());
            assert!(!v.normal[0].is_nan());
            assert!(!v.normal[1].is_nan());
            assert!(!v.normal[2].is_nan());
        }
    }

    #[test]
    fn test_hidden_ca_removes_its_residue_and_splits_the_chain() {
        let (group, sec_map) = AtomGroup::create_test_protein_fragment();
        let rep = Representation::new(
            "cartoon_all",
            Selector::All,
            RepresentationKind::Cartoon,
            ColorScheme::SecondaryStructure,
        );
        // 8 番目あたりの CA を隠す
        let ca_ids: Vec<u32> = Selector::Name(vec!["CA".to_string()]).select_atom_ids(&group);
        let hidden: HashSet<u32> = [ca_ids[8]].into_iter().collect();
        let chains = extract_ribbon_chains(&group, &rep, &sec_map, None, &hidden);
        assert_eq!(chains.len(), 2, "the chain is split at the hidden residue");
        assert_eq!(chains.iter().map(Vec::len).sum::<usize>(), 15);
    }

    #[test]
    fn test_build_all_cartoons_with_empty_and_active_rep() {
        let (group, sec_map) = AtomGroup::create_test_protein_fragment();

        // 非表示の表現
        let mut rep_hidden = Representation::new(
            "cartoon_hidden",
            Selector::All,
            RepresentationKind::Cartoon,
            ColorScheme::SecondaryStructure,
        );
        rep_hidden.visible = false;
        let mesh_hidden = build_all_cartoons(&group, &[rep_hidden], &sec_map, None);
        assert_eq!(mesh_hidden.vertices.len(), 0);

        // 表示有効な表現
        let rep_active = Representation::new(
            "cartoon_active",
            Selector::All,
            RepresentationKind::Cartoon,
            ColorScheme::SecondaryStructure,
        );
        let mesh_active = build_all_cartoons(&group, &[rep_active], &sec_map, None);
        assert!(mesh_active.vertices.len() > 100);
        assert!(mesh_active.indices.len() > 200);
    }

    #[test]
    fn test_build_all_cartoons_multi_model_filtering() {
        use yui_core::structure_loader::parse_structure_bytes;

        let cif_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/tests/data/1HLS.cif");
        let bytes = std::fs::read(cif_path).expect("failed to read 1HLS.cif");
        let (group, sec_map, _) =
            parse_structure_bytes(&bytes, "1HLS.cif").expect("parse 1HLS.cif");

        let rep = Representation::new(
            "cartoon_all",
            Selector::All,
            RepresentationKind::Cartoon,
            ColorScheme::SecondaryStructure,
        );

        // 1. active_model が None のとき: 全 20 モデル分のリボンが構築される
        let mesh_all = build_all_cartoons(&group, std::slice::from_ref(&rep), &sec_map, None);
        assert!(!mesh_all.vertices.is_empty());

        // 2. active_model を第1モデル ("1") に指定したとき: 1モデル分のみ
        let first_model = yui_core::atom_group::iter_models(&group).next().unwrap().0;
        let mesh_m1 = build_all_cartoons(&group, &[rep], &sec_map, Some(first_model));
        assert!(!mesh_m1.vertices.is_empty());

        // 20モデル全体の頂点数・インデックス数は、1モデル分の約20倍（各モデルは同構造）
        assert!(
            mesh_all.vertices.len() > mesh_m1.vertices.len() * 15,
            "All models cartoon vertices ({}) must be significantly larger than single model ({})",
            mesh_all.vertices.len(),
            mesh_m1.vertices.len()
        );
    }
}
