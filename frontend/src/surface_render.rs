// SPDX-FileCopyrightText: The Yui development team
// SPDX-License-Identifier: GPL-3.0-or-later

//! 分子表面（`RepresentationKind::Surface`）の描画用データ構築および頂点カラー解決モジュール。
//!
//! architecture.md 4.4節に準拠し、`core::surface` で生成された `SurfaceMesh` を
//! 選択（Selection）・モデル（ActiveModel）・配色規則（ColorScheme）・透明度（Opacity）に
//! 従ってGPU描画用頂点配列（`RibbonVertex`）へ変換・集約する。

use std::collections::{HashMap, HashSet};
use yui_core::atom_group::{
    AtomGroup, ModelAtomRange, SecondaryStructureKind, SecondaryStructureMap, iter_chains,
    iter_flat_atoms, iter_models, iter_residues,
};
use yui_core::representation::ColorScheme;
use yui_core::state::Atom;
use yui_core::surface::{
    SurfaceAtom, SurfaceMesh, SurfaceParams, build_molecular_surface_from_atoms,
};

use crate::ribbon::{
    RibbonVertex, default_chain_color, default_residue_color, default_secondary_structure_color,
};

/// 表面生成の世代追跡器。
///
/// バックグラウンドタスクで非同期に生成される表面メッシュが、
/// 古い世代の結果によって新しい状態を上書きすることを防止する。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SurfaceGenerationTracker {
    current_generation: u64,
}

impl SurfaceGenerationTracker {
    pub fn new() -> Self {
        Self {
            current_generation: 0,
        }
    }

    /// 次の世代番号を発行する
    pub fn next_generation(&mut self) -> u64 {
        self.current_generation += 1;
        self.current_generation
    }

    /// 現在の世代番号を取得する
    #[allow(dead_code)]
    pub fn current_generation(&self) -> u64 {
        self.current_generation
    }

    /// 指定された世代番号が最新かどうかを判定する
    pub fn is_latest(&self, generation: u64) -> bool {
        generation == self.current_generation
    }
}

/// 表面Representationのジオメトリ影響シグネチャを抽出する。
///
/// 選択・可視性・種類・パラメータなどのジオメトリに関わる属性のみを含め、
/// 色や透明度などジオメトリ再構築が不要な変更と判別する。
pub fn extract_surface_geometry_signature(
    representations: &[yui_core::representation::Representation],
) -> Vec<(
    String,
    bool,
    yui_core::representation::RepresentationKind,
    String,
)> {
    representations
        .iter()
        .filter(|r| r.kind == yui_core::representation::RepresentationKind::Surface)
        .map(|r| {
            (
                r.id.clone(),
                r.visible,
                r.kind,
                format!("{:?}", r.selection),
            )
        })
        .collect()
}

/// 表面描画用のRepresentation別キャッシュエントリ
#[derive(Debug, Clone)]
pub struct SurfaceMeshEntry {
    pub rep_id: String,
    pub mesh: SurfaceMesh,
    pub color_scheme: ColorScheme,
    pub opacity: f32,
    pub visible: bool,
    pub vertices: Vec<RibbonVertex>,
    pub indices: Vec<u32>,
}

/// GPU描画用に不透明／半透明に集約された頂点・インデックスバッファ
#[derive(Debug, Clone, Default)]
pub struct SurfaceBatches {
    pub opaque_vertices: Vec<RibbonVertex>,
    pub opaque_indices: Vec<u32>,
    pub transparent_vertices: Vec<RibbonVertex>,
    pub transparent_indices: Vec<u32>,
}

impl SurfaceBatches {
    /// 何も描かないバッチへの参照（再生中に、分子表面を描かないために使う）
    pub fn empty_ref() -> &'static SurfaceBatches {
        static EMPTY: std::sync::OnceLock<SurfaceBatches> = std::sync::OnceLock::new();
        EMPTY.get_or_init(SurfaceBatches::default)
    }

    /// キャッシュエントリ一覧から不透明／半透明の描画バッチを構築する
    #[cfg(test)]
    pub fn build_from_entries(entries: &[SurfaceMeshEntry]) -> Self {
        Self::build_from_groups(&[entries])
    }

    /// 複数のエントリ一覧（分子表面、CUBE の等値面など）を、順に 1 つのバッチにまとめる
    pub fn build_from_groups(groups: &[&[SurfaceMeshEntry]]) -> Self {
        let mut batches = Self::default();

        for entry in groups.iter().flat_map(|g| g.iter()) {
            if !entry.visible || entry.indices.is_empty() || entry.vertices.is_empty() {
                continue;
            }

            if entry.opacity >= 1.0 {
                merge_mesh_buffers(
                    &mut batches.opaque_vertices,
                    &mut batches.opaque_indices,
                    &entry.vertices,
                    &entry.indices,
                );
            } else {
                merge_mesh_buffers(
                    &mut batches.transparent_vertices,
                    &mut batches.transparent_indices,
                    &entry.vertices,
                    &entry.indices,
                );
            }
        }

        batches
    }
}

/// 頂点バッファ・インデックスバッファをオフセットを考慮してマージする純粋関数
pub fn merge_mesh_buffers(
    target_vertices: &mut Vec<RibbonVertex>,
    target_indices: &mut Vec<u32>,
    source_vertices: &[RibbonVertex],
    source_indices: &[u32],
) {
    let base_vertex = target_vertices.len() as u32;
    target_vertices.extend_from_slice(source_vertices);
    target_indices.reserve(source_indices.len());
    for &idx in source_indices {
        target_indices.push(base_vertex + idx);
    }
}

/// 表面生成の対象となる原子を選択集合およびアクティブモデルから絞り込む純粋関数
pub fn filter_surface_atoms<'a>(
    all_atoms: &'a [Atom],
    selected_ids: &[u32],
    active_model: Option<&str>,
    model_atom_ranges: &[ModelAtomRange],
) -> Vec<&'a Atom> {
    let target_range =
        active_model.and_then(|model| model_atom_ranges.iter().find(|r| r.model_key == model));

    let in_active_model = |id: u32| -> bool {
        match (active_model, target_range) {
            (Some(_), Some(range)) => range.contains(id),
            (Some(_), None) => false,
            (None, _) => true,
        }
    };

    let selected_set: HashSet<u32> = selected_ids.iter().copied().collect();

    all_atoms
        .iter()
        .filter(|a| selected_set.contains(&a.id) && in_active_model(a.id))
        .collect()
}

/// `AtomGroup` から原子IDごとの二次構造種別マップを構築するヘルパー
pub fn build_atom_secondary_structure_map(
    atom_group: &AtomGroup,
    sec_map: &SecondaryStructureMap,
) -> HashMap<u32, SecondaryStructureKind> {
    let mut map = HashMap::new();
    let atom_path_to_id: HashMap<String, u32> = iter_flat_atoms(atom_group)
        .map(|fa| (fa.atom.path.clone(), fa.id))
        .collect();

    for (model_key, model) in iter_models(atom_group) {
        for (chain_key, chain) in iter_chains(model) {
            for (res_key, residue) in iter_residues(chain) {
                let res_path = format!("/{model_key}/{chain_key}/{res_key}/");
                let sec = sec_map
                    .get(&res_path)
                    .copied()
                    .unwrap_or(SecondaryStructureKind::Loop);
                for (_atom_name, atom_rec) in residue.atoms() {
                    if let Some(&id) = atom_path_to_id.get(&atom_rec.path) {
                        map.insert(id, sec);
                    }
                }
            }
        }
    }
    map
}

/// 表面頂点の色（RGBA）を `ColorScheme` および `opacity` に従って解決する純粋関数
pub fn resolve_surface_vertex_colors(
    vertex_atom_ids: &[u32],
    color_scheme: &ColorScheme,
    opacity: f32,
    atom_map: &HashMap<u32, &Atom>,
    unique_chains: &[&str],
    unique_residues: &[(&str, i32)],
    atom_sec_map: &HashMap<u32, SecondaryStructureKind>,
) -> Vec<[f32; 4]> {
    let clamped_opacity = opacity.clamp(0.0, 1.0);

    vertex_atom_ids
        .iter()
        .map(|&atom_id| {
            let mut base_color = match color_scheme {
                ColorScheme::Uniform(c) => *c,
                ColorScheme::Element => atom_map
                    .get(&atom_id)
                    .map(|a| a.color)
                    .unwrap_or([0.7, 0.7, 0.7, 1.0]),
                ColorScheme::Chain => {
                    if let Some(atom) = atom_map.get(&atom_id) {
                        let chain_idx = unique_chains
                            .iter()
                            .position(|&c| c == atom.chain_id.as_str())
                            .unwrap_or(0);
                        default_chain_color(chain_idx)
                    } else {
                        [0.7, 0.7, 0.7, 1.0]
                    }
                }
                ColorScheme::Residue => {
                    if let Some(atom) = atom_map.get(&atom_id) {
                        let key = (atom.chain_id.as_str(), atom.residue_index);
                        let res_idx = unique_residues.iter().position(|&k| k == key).unwrap_or(0);
                        default_residue_color(res_idx)
                    } else {
                        [0.7, 0.7, 0.7, 1.0]
                    }
                }
                ColorScheme::SecondaryStructure => {
                    let sec = atom_sec_map
                        .get(&atom_id)
                        .copied()
                        .unwrap_or(SecondaryStructureKind::Loop);
                    default_secondary_structure_color(sec)
                }
            };
            base_color[3] *= clamped_opacity;
            base_color
        })
        .collect()
}

/// `SurfaceMesh` と頂点カラーから `RibbonVertex` 配列を構築する純粋関数
pub fn build_surface_vertices(mesh: &SurfaceMesh, colors: &[[f32; 4]]) -> Vec<RibbonVertex> {
    assert_eq!(
        mesh.positions.len(),
        colors.len(),
        "positions and colors length must match"
    );
    assert_eq!(
        mesh.positions.len(),
        mesh.normals.len(),
        "positions and normals length must match"
    );

    mesh.positions
        .iter()
        .zip(mesh.normals.iter())
        .zip(colors.iter())
        .map(|((&position, &normal), &color)| RibbonVertex {
            position,
            normal,
            color,
        })
        .collect()
}

/// 非同期生成用のビルドリクエスト
#[derive(Debug, Clone)]
pub struct SurfaceBuildRequest {
    pub rep_id: String,
    pub atoms: Vec<SurfaceAtom>,
    pub params: SurfaceParams,
    pub color_scheme: ColorScheme,
    pub opacity: f32,
    pub visible: bool,
}

/// 非同期生成用のビルド結果
#[derive(Debug, Clone)]
pub struct SurfaceBuildResult {
    pub rep_id: String,
    pub mesh: SurfaceMesh,
    pub color_scheme: ColorScheme,
    pub opacity: f32,
    pub visible: bool,
}

/// 頂点の色を決めるのに要る、構造についての情報（原子、鎖、残基、二次構造）。
/// 所有する形なので、別のスレッドへ渡せる（アニメーションの書き出しで、フレームごとに分子表面を作るとき）。
/// 原子の ID・元素・鎖・残基・二次構造は、フレームが変わっても変わらない（座標だけが変わる）。
#[derive(Debug, Clone, Default)]
pub struct SurfaceColorContext {
    atom_map: HashMap<u32, Atom>,
    unique_chains: Vec<String>,
    unique_residues: Vec<(String, i32)>,
    atom_sec_map: HashMap<u32, SecondaryStructureKind>,
}

impl SurfaceColorContext {
    pub fn new(
        atoms: &[Atom],
        atom_group: Option<&AtomGroup>,
        sec_map: &SecondaryStructureMap,
    ) -> Self {
        let mut unique_chains: Vec<String> = Vec::new();
        let mut unique_residues: Vec<(String, i32)> = Vec::new();
        for a in atoms {
            if !unique_chains.contains(&a.chain_id) {
                unique_chains.push(a.chain_id.clone());
            }
            let key = (a.chain_id.clone(), a.residue_index);
            if !unique_residues.contains(&key) {
                unique_residues.push(key);
            }
        }
        Self {
            atom_map: atoms.iter().map(|a| (a.id, a.clone())).collect(),
            unique_chains,
            unique_residues,
            atom_sec_map: atom_group
                .map(|ag| build_atom_secondary_structure_map(ag, sec_map))
                .unwrap_or_default(),
        }
    }

    /// 生成の結果から、描画用のエントリ（頂点の色つき）を作る
    pub fn entries_from_results(&self, results: Vec<SurfaceBuildResult>) -> Vec<SurfaceMeshEntry> {
        let atom_map: HashMap<u32, &Atom> = self.atom_map.iter().map(|(k, v)| (*k, v)).collect();
        let chains: Vec<&str> = self.unique_chains.iter().map(String::as_str).collect();
        let residues: Vec<(&str, i32)> = self
            .unique_residues
            .iter()
            .map(|(c, r)| (c.as_str(), *r))
            .collect();
        results
            .into_iter()
            .map(|res| {
                let colors = resolve_surface_vertex_colors(
                    &res.mesh.vertex_atom_ids,
                    &res.color_scheme,
                    res.opacity,
                    &atom_map,
                    &chains,
                    &residues,
                    &self.atom_sec_map,
                );
                let vertices = build_surface_vertices(&res.mesh, &colors);
                let indices = res.mesh.indices.clone();
                SurfaceMeshEntry {
                    rep_id: res.rep_id,
                    mesh: res.mesh,
                    color_scheme: res.color_scheme,
                    opacity: res.opacity,
                    visible: res.visible,
                    vertices,
                    indices,
                }
            })
            .collect()
    }
}

/// リクエスト一覧からメッシュを生成する処理（ワーカースレッドまたは同期的実行用）
pub fn process_surface_build_requests(requests: &[SurfaceBuildRequest]) -> Vec<SurfaceBuildResult> {
    requests
        .iter()
        .map(|req| {
            let mesh = if req.atoms.is_empty() {
                SurfaceMesh::default()
            } else {
                build_molecular_surface_from_atoms(&req.atoms, &req.params)
            };
            SurfaceBuildResult {
                rep_id: req.rep_id.clone(),
                mesh,
                color_scheme: req.color_scheme.clone(),
                opacity: req.opacity,
                visible: req.visible,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use yui_core::AtomGroupExt;
    use yui_core::atom_group::calculate_model_atom_ranges;
    use yui_core::representation::ColorScheme;
    use yui_core::selector::Selector;
    use yui_core::structure_loader::parse_structure_bytes;
    use yui_core::surface::{SurfaceParams, build_molecular_surface};

    #[test]
    fn test_surface_generation_tracker() {
        let mut tracker = SurfaceGenerationTracker::new();
        assert_eq!(tracker.current_generation(), 0);

        let gen1 = tracker.next_generation();
        assert_eq!(gen1, 1);
        assert_eq!(tracker.current_generation(), 1);
        assert!(tracker.is_latest(1));
        assert!(!tracker.is_latest(0));

        let gen2 = tracker.next_generation();
        assert_eq!(gen2, 2);
        assert_eq!(tracker.current_generation(), 2);
        assert!(!tracker.is_latest(1));
        assert!(tracker.is_latest(2));
    }

    #[test]
    fn test_visible_false_not_rendered() {
        let dummy_vertex = RibbonVertex {
            position: [0.0, 0.0, 0.0],
            normal: [0.0, 1.0, 0.0],
            color: [1.0, 1.0, 1.0, 1.0],
        };

        let invisible_entry = SurfaceMeshEntry {
            rep_id: "surf_invis".to_string(),
            mesh: SurfaceMesh::default(),
            color_scheme: ColorScheme::Element,
            opacity: 1.0,
            visible: false,
            vertices: vec![dummy_vertex],
            indices: vec![0, 1, 2],
        };

        let batches = SurfaceBatches::build_from_entries(std::slice::from_ref(&invisible_entry));
        assert!(
            batches.opaque_indices.is_empty(),
            "Invisible surface must not produce opaque indices"
        );
        assert!(
            batches.transparent_indices.is_empty(),
            "Invisible surface must not produce transparent indices"
        );

        let mut visible_entry = invisible_entry;
        visible_entry.visible = true;
        let batches_vis = SurfaceBatches::build_from_entries(std::slice::from_ref(&visible_entry));
        assert_eq!(
            batches_vis.opaque_indices.len(),
            3,
            "Visible opaque surface must produce indices"
        );
    }

    #[test]
    fn test_filter_surface_atoms_active_model() {
        let cif_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/tests/data/1HLS.cif");
        let bytes = std::fs::read(cif_path).expect("failed to read 1HLS.cif");
        let (group, _sec_map, _interactions) =
            parse_structure_bytes(&bytes, "1HLS.cif").expect("parse 1HLS.cif");
        let molecule = group.to_flat_molecule();
        let ranges = calculate_model_atom_ranges(&group);

        assert_eq!(molecule.atoms.len(), 15640);
        assert_eq!(ranges.len(), 20);

        let all_atom_ids: Vec<u32> = molecule.atoms.iter().map(|a| a.id).collect();

        // 1. active_model が指定されたとき（第1モデル "1"）: 該当モデルの 782 原子のみに絞り込まれること
        let first_model = &ranges[0].model_key;
        let model1_atoms =
            filter_surface_atoms(&molecule.atoms, &all_atom_ids, Some(first_model), &ranges);
        assert_eq!(
            model1_atoms.len(),
            782,
            "Specified active_model should filter to exactly 782 atoms"
        );

        for atom in &model1_atoms {
            assert!(
                ranges[0].contains(atom.id),
                "Atom {} must be within model range [{}, {}]",
                atom.id,
                ranges[0].start_id,
                ranges[0].end_id
            );
        }

        // 第1モデルの原子群から表面メッシュを生成したとき、頂点の原子IDがすべて第1モデルの範囲に入ること
        let surface_atoms: Vec<SurfaceAtom> = model1_atoms.iter().map(|&a| a.into()).collect();
        let params = SurfaceParams::default();
        let mesh = build_molecular_surface_from_atoms(&surface_atoms, &params);
        assert!(!mesh.positions.is_empty());
        for &atom_id in &mesh.vertex_atom_ids {
            assert!(
                ranges[0].contains(atom_id),
                "Surface vertex atom ID {atom_id} must be in model 1 range [{}, {}]",
                ranges[0].start_id,
                ranges[0].end_id
            );
        }
    }

    #[test]
    fn test_filter_surface_atoms_selection_chain() {
        let cif_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/tests/data/1HLS.cif");
        let bytes = std::fs::read(cif_path).expect("failed to read 1HLS.cif");
        let (group, _sec_map, _interactions) =
            parse_structure_bytes(&bytes, "1HLS.cif").expect("parse 1HLS.cif");
        let molecule = group.to_flat_molecule();
        let ranges = calculate_model_atom_ranges(&group);

        let first_model = &ranges[0].model_key;

        // 鎖 A の原子IDを選択
        let chain_a_selector = Selector::ResidueRange {
            chain: Some("A".to_string()),
            start: i32::MIN,
            end: i32::MAX,
        };
        let chain_a_ids = chain_a_selector.select_atom_ids(&group);

        let filtered_atoms =
            filter_surface_atoms(&molecule.atoms, &chain_a_ids, Some(first_model), &ranges);

        assert!(!filtered_atoms.is_empty());
        for atom in &filtered_atoms {
            assert_eq!(atom.chain_id, "A", "Atom must belong to chain A");
            assert!(ranges[0].contains(atom.id), "Atom must belong to model 1");
        }

        let surface_atoms: Vec<SurfaceAtom> = filtered_atoms.iter().map(|&a| a.into()).collect();
        let params = SurfaceParams::default();
        let mesh = build_molecular_surface_from_atoms(&surface_atoms, &params);
        assert!(!mesh.positions.is_empty());
        let chain_a_set: HashSet<u32> = filtered_atoms.iter().map(|a| a.id).collect();
        for &atom_id in &mesh.vertex_atom_ids {
            assert!(
                chain_a_set.contains(&atom_id),
                "Surface vertex atom ID {atom_id} must belong to chain A"
            );
        }
    }

    #[test]
    fn test_resolve_surface_vertex_colors_1hls() {
        let pdb_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/tests/data/1hls.pdb");
        let bytes = std::fs::read(pdb_path).expect("failed to read 1hls.pdb");
        let (group, sec_map, _interactions) =
            parse_structure_bytes(&bytes, "1hls.pdb").expect("parse 1hls.pdb");
        let molecule = group.to_flat_molecule();

        let params = SurfaceParams::default();
        let mesh = build_molecular_surface(&molecule.atoms, &params);
        assert!(!mesh.positions.is_empty());

        let atom_map: HashMap<u32, &Atom> = molecule.atoms.iter().map(|a| (a.id, a)).collect();

        let mut unique_chains: Vec<&str> = Vec::new();
        for a in &molecule.atoms {
            if !unique_chains.contains(&a.chain_id.as_str()) {
                unique_chains.push(&a.chain_id);
            }
        }

        let mut unique_residues: Vec<(&str, i32)> = Vec::new();
        for a in &molecule.atoms {
            let key = (a.chain_id.as_str(), a.residue_index);
            if !unique_residues.contains(&key) {
                unique_residues.push(key);
            }
        }

        let atom_sec_map = build_atom_secondary_structure_map(&group, &sec_map);

        // 1. Uniform 配色
        let uniform_color = [0.2, 0.4, 0.8, 1.0];
        let opacity = 0.75;
        let colors_uniform = resolve_surface_vertex_colors(
            &mesh.vertex_atom_ids,
            &ColorScheme::Uniform(uniform_color),
            opacity,
            &atom_map,
            &unique_chains,
            &unique_residues,
            &atom_sec_map,
        );
        assert_eq!(colors_uniform.len(), mesh.positions.len());
        for &c in &colors_uniform {
            assert_eq!(c[0], 0.2);
            assert_eq!(c[1], 0.4);
            assert_eq!(c[2], 0.8);
            assert!((c[3] - 0.75).abs() < 1e-4);
        }

        // 2. Element 配色
        let colors_element = resolve_surface_vertex_colors(
            &mesh.vertex_atom_ids,
            &ColorScheme::Element,
            1.0,
            &atom_map,
            &unique_chains,
            &unique_residues,
            &atom_sec_map,
        );
        assert_eq!(colors_element.len(), mesh.positions.len());
        for (i, &c) in colors_element.iter().enumerate() {
            let atom_id = mesh.vertex_atom_ids[i];
            let expected_color = atom_map.get(&atom_id).unwrap().color;
            assert_eq!(c, expected_color);
        }

        // 3. Chain 配色
        let colors_chain = resolve_surface_vertex_colors(
            &mesh.vertex_atom_ids,
            &ColorScheme::Chain,
            1.0,
            &atom_map,
            &unique_chains,
            &unique_residues,
            &atom_sec_map,
        );
        assert_eq!(colors_chain.len(), mesh.positions.len());
        for (i, &c) in colors_chain.iter().enumerate() {
            let atom_id = mesh.vertex_atom_ids[i];
            let atom = atom_map.get(&atom_id).unwrap();
            let chain_idx = unique_chains
                .iter()
                .position(|&ch| ch == atom.chain_id.as_str())
                .unwrap();
            assert_eq!(c, default_chain_color(chain_idx));
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn test_color_context_gives_the_same_colors_as_the_borrowed_lookup() {
        let pdb_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/tests/data/1hls.pdb");
        let bytes = std::fs::read(pdb_path).unwrap();
        let (group, sec_map, _) = parse_structure_bytes(&bytes, "1hls.pdb").unwrap();
        let molecule = group.to_flat_molecule();
        let atoms: Vec<yui_core::surface::SurfaceAtom> =
            molecule.atoms.iter().take(120).map(|a| a.into()).collect();
        let mesh = build_molecular_surface_from_atoms(&atoms, &SurfaceParams::default());
        assert!(!mesh.positions.is_empty());

        let ctx = SurfaceColorContext::new(&molecule.atoms, Some(&group), &sec_map);
        for scheme in [
            ColorScheme::Element,
            ColorScheme::Chain,
            ColorScheme::Residue,
            ColorScheme::SecondaryStructure,
            ColorScheme::Uniform([0.1, 0.2, 0.3, 1.0]),
        ] {
            let entries = ctx.entries_from_results(vec![SurfaceBuildResult {
                rep_id: "s".to_string(),
                mesh: mesh.clone(),
                color_scheme: scheme.clone(),
                opacity: 0.7,
                visible: true,
            }]);
            // 画面の更新（`update_surface_colors`）と同じ、借りた形の参照で求めた色
            let atom_map: HashMap<u32, &Atom> = molecule.atoms.iter().map(|a| (a.id, a)).collect();
            let mut chains: Vec<&str> = Vec::new();
            let mut residues: Vec<(&str, i32)> = Vec::new();
            for a in &molecule.atoms {
                if !chains.contains(&a.chain_id.as_str()) {
                    chains.push(&a.chain_id);
                }
                let key = (a.chain_id.as_str(), a.residue_index);
                if !residues.contains(&key) {
                    residues.push(key);
                }
            }
            let sec = build_atom_secondary_structure_map(&group, &sec_map);
            let expected = build_surface_vertices(
                &mesh,
                &resolve_surface_vertex_colors(
                    &mesh.vertex_atom_ids,
                    &scheme,
                    0.7,
                    &atom_map,
                    &chains,
                    &residues,
                    &sec,
                ),
            );
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].vertices.len(), expected.len());
            assert!(
                entries[0]
                    .vertices
                    .iter()
                    .zip(&expected)
                    .all(|(a, b)| a.color == b.color && a.position == b.position),
                "{scheme:?}"
            );
            assert_eq!(entries[0].indices, mesh.indices);
        }
    }
}
