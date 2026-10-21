//! 表示中のビューを SVG にして保存する（File → Export Image (SVG)...）。
//!
//! 何を描くかは、画面と同じ `style_layers`（表示・非表示、配色、表示中のモデルを反映）で決め、
//! 投影とファイルの書き出しは分ける。SVG の文字列は `yui_core::render_svg` が作り、
//! 保存は `rfd` のダイアログ（ネイティブはファイル、ブラウザはダウンロード）で行う。

use std::path::PathBuf;

use iced::widget::{button, column, container, row, text};
use iced::{Alignment, Element, Length};

use crate::canvas::{CanvasAction, CanvasOptions};

use crate::viewport::scene::style_layers;
use yui_core::state::{AppState, CameraState};
use yui_core::{SvgAtom, SvgBond, SvgLabel, SvgMesh, SvgScene};

/// 幅の候補（SVG の `width`。ベクターなので、拡大しても粗くならない）
pub const WIDTH_PRESETS: [u32; 4] = [800, 1600, 2400, 3200];

/// SVG の書き出しの設定
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SvgOptions {
    /// 画像の幅（形が `Custom` のときは使わない）
    pub width: u32,
    pub canvas: CanvasOptions,
}

impl Default for SvgOptions {
    fn default() -> Self {
        Self {
            width: 1600,
            canvas: CanvasOptions::default(),
        }
    }
}

/// 書き出しのダイアログの操作
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvgDialogAction {
    SetWidth(u32),
    Canvas(CanvasAction),
    /// 保存先を選んで、書き出す
    Export,
    Cancel,
}

impl SvgOptions {
    /// ダイアログの操作を設定に反映する（`Export` と `Cancel` は、呼び出し側が扱う）。候補にない値は無視する。
    pub fn apply(&mut self, action: SvgDialogAction) {
        match action {
            SvgDialogAction::SetWidth(w) if WIDTH_PRESETS.contains(&w) => self.width = w,
            SvgDialogAction::Canvas(c) => self.canvas.apply(c),
            _ => {}
        }
    }
}

/// 書き出しのダイアログ（形と構図を選んで、「Export...」で保存先の選択へ進む）
pub fn view_svg_dialog<'a, Message: Clone + 'a>(
    options: SvgOptions,
    on_action: impl Fn(SvgDialogAction) -> Message + Copy + 'a,
) -> Element<'a, Message> {
    let mut body = column![text("Export Image (SVG)").size(14)].spacing(10);
    for r in crate::canvas::view_canvas_rows(options.canvas, crate::canvas::chip, move |a| {
        on_action(SvgDialogAction::Canvas(a))
    }) {
        body = body.push(r);
    }
    // 形が Custom のときは、指定の幅と高さを使うので、幅の候補は出さない
    if options.canvas.shape != crate::canvas::CanvasShape::Custom {
        let mut widths = row![text("Width").size(12).width(Length::Fixed(80.0))]
            .spacing(6)
            .align_y(Alignment::Center);
        for w in WIDTH_PRESETS {
            widths = widths.push(crate::canvas::chip(
                format!("{w} px"),
                options.width == w,
                on_action(SvgDialogAction::SetWidth(w)),
            ));
        }
        body = body.push(widths);
    }
    let card = container(
        body.push(
            row![
                button(text("Export...").size(12))
                    .padding([4, 12])
                    .style(crate::ui_style::accent_button)
                    .on_press(on_action(SvgDialogAction::Export)),
                button(text("Cancel").size(12))
                    .padding([4, 12])
                    .style(crate::ui_style::secondary_button)
                    .on_press(on_action(SvgDialogAction::Cancel)),
            ]
            .spacing(8),
        ),
    )
    .padding(16)
    .style(container::rounded_box);
    container(card).center(Length::Fill).into()
}

/// 頂点の並び（位置、法線、色）を、SVG のメッシュにする
fn mesh_of(
    vertices: &[crate::ribbon::RibbonVertex],
    indices: &[u32],
    cull_back: bool,
) -> Option<SvgMesh> {
    if vertices.is_empty() || indices.is_empty() {
        return None;
    }
    Some(SvgMesh {
        positions: vertices.iter().map(|v| v.position).collect(),
        normals: vertices.iter().map(|v| v.normal).collect(),
        colors: vertices.iter().map(|v| v.color).collect(),
        indices: indices.to_vec(),
        cull_back,
    })
}

/// 画面に描かれている原子・結合・相互作用の線・注釈・リボン・分子表面（CUBE の等値面を含む）を、
/// SVG に書き出す要素にする。`surfaces` は、画面が描く分子表面と等値面のバッチ。
pub(crate) fn build_scene(
    state: &AppState,
    model_atom_ranges: &[yui_core::atom_group::ModelAtomRange],
    resolved_interactions: &[yui_core::interaction::ResolvedInteraction],
    surfaces: &crate::surface_render::SurfaceBatches,
) -> SvgScene {
    let layers = style_layers(
        &state.molecule,
        state.atom_group.as_ref(),
        &state.representations,
        state.active_model.as_deref(),
        model_atom_ranges,
    );
    let mut scene = SvgScene {
        background: state.scene.background_color,
        ..Default::default()
    };
    for layer in &layers {
        for atom in &layer.atoms {
            if let Some(radius) = layer.style.sphere_radius(atom.radius) {
                scene.atoms.push(SvgAtom {
                    position: atom.position,
                    radius,
                    color: layer.colors.get(&atom.id).copied().unwrap_or(atom.color),
                });
            }
        }
        scene.bonds.extend(
            crate::viewport::scene::styled_stick_segments(
                &state.molecule.bonds,
                &layer.atoms,
                layer.style,
                &layer.colors,
            )
            .into_iter()
            .map(|s| SvgBond {
                start: s.start,
                end: s.end,
                radius: s.radius,
                color: s.color,
                dashed: false,
            }),
        );
    }
    // 非共有結合相互作用の線（水素結合、塩橋、CH-π、ジスルフィド）。画面と同じ絞り込みと色で、破線にする。
    scene.bonds.extend(
        crate::interaction_render::build_interaction_lines_from_resolved(
            state.atom_group.as_ref(),
            resolved_interactions,
            &state.representations,
            state.active_model.as_deref(),
            model_atom_ranges,
        )
        .into_iter()
        .map(|line| SvgBond {
            start: line.start,
            end: line.end,
            radius: crate::user_settings::startup_render().interaction_radius,
            color: line.color,
            dashed: true,
        }),
    );
    // リボン（カートゥーン）。画面と同じメッシュで、両面が見える
    if let Some(atom_group) = state.atom_group.as_ref() {
        let ribbons = crate::ribbon::build_all_cartoons(
            atom_group,
            &state.representations,
            &state.secondary_structure_map,
            state.active_model.as_deref(),
        );
        scene
            .meshes
            .extend(mesh_of(&ribbons.vertices, &ribbons.indices, false));
    }
    // 分子表面と CUBE の等値面。不透明な面は閉じているので背面を省き、半透明の面は両面を出す
    scene.meshes.extend(mesh_of(
        &surfaces.opaque_vertices,
        &surfaces.opaque_indices,
        true,
    ));
    scene.meshes.extend(mesh_of(
        &surfaces.transparent_vertices,
        &surfaces.transparent_indices,
        false,
    ));
    scene.labels = state
        .annotations
        .iter()
        .filter(|a| a.visible)
        .map(|a| SvgLabel {
            position: a.position,
            text: a.text.clone(),
            color: a.color,
        })
        .collect();
    scene
}

/// 指定のカメラで、`size`（幅、高さ）のキャンバスに見えるビューの SVG。
pub(crate) fn render_view_svg(
    state: &AppState,
    model_atom_ranges: &[yui_core::atom_group::ModelAtomRange],
    resolved_interactions: &[yui_core::interaction::ResolvedInteraction],
    surfaces: &crate::surface_render::SurfaceBatches,
    camera: &CameraState,
    size: (u32, u32),
) -> Result<String, String> {
    if size.0 == 0 || size.1 == 0 {
        return Err("The view has no size.".to_string());
    }
    let aspect = size.0 as f32 / size.1 as f32;
    let m = crate::viewport::camera::view_proj_matrix(camera, aspect);
    let view_proj = [
        [m[0], m[1], m[2], m[3]],
        [m[4], m[5], m[6], m[7]],
        [m[8], m[9], m[10], m[11]],
        [m[12], m[13], m[14], m[15]],
    ];
    let mut scene = build_scene(state, model_atom_ranges, resolved_interactions, surfaces);
    scene.eye = camera.eye;
    yui_core::render_svg(&scene, view_proj, size.0 as f32, size.1 as f32)
}

/// 保存ダイアログを開いて、SVG を書き出す。キャンセルは `Ok(None)`。
pub async fn save_svg_file(
    suggested_name: Option<String>,
    svg: String,
) -> Result<Option<(String, Option<PathBuf>)>, String> {
    let dialog = rfd::AsyncFileDialog::new()
        .add_filter("SVG Image (*.svg)", &["svg"])
        .set_file_name(suggested_name.unwrap_or_else(|| "view.svg".to_string()));
    let Some(handle) = dialog.save_file().await else {
        return Ok(None);
    };
    let file_name = handle.file_name();

    #[cfg(not(target_arch = "wasm32"))]
    let path = Some(handle.path().to_path_buf());
    #[cfg(target_arch = "wasm32")]
    let path = None;

    handle
        .write(svg.as_bytes())
        .await
        .map_err(|e| format!("Failed to write SVG file: {e}"))?;
    Ok(Some((file_name, path)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use yui_core::representation::{ColorScheme, Representation, RepresentationKind};
    use yui_core::selector::Selector;
    use yui_core::state::{Atom, Bond, Molecule};

    fn atom(id: u32, x: f32) -> Atom {
        Atom {
            id,
            element: "C".to_string(),
            position: [x, 0.0, 0.0],
            radius: 1.7,
            color: [0.5, 0.5, 0.5, 1.0],
            residue_name: "GLY".to_string(),
            residue_index: 1,
            chain_id: "A".to_string(),
        }
    }

    fn state_with(reps: Vec<Representation>) -> AppState {
        AppState {
            molecule: Molecule {
                atoms: vec![atom(1, 0.0), atom(2, 1.5), atom(3, 3.0)],
                bonds: vec![
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
                ],
            },
            representations: reps,
            ..Default::default()
        }
    }

    fn rep(id: &str, ids: Vec<u32>, kind: RepresentationKind, visible: bool) -> Representation {
        Representation::new(id, Selector::AtomIds(ids), kind, ColorScheme::Element)
            .with_visible(visible)
    }

    #[test]
    fn test_scene_follows_the_visible_representations() {
        // 表現なし: 全原子をボール・スティックで
        let scene = build_scene(
            &state_with(vec![]),
            &[],
            &[],
            crate::surface_render::SurfaceBatches::empty_ref(),
        );
        assert_eq!((scene.atoms.len(), scene.bonds.len()), (3, 2));

        // 非表示のカードは描かれない。Spacefill は球だけで結合なし
        let scene = build_scene(
            &state_with(vec![
                rep("a", vec![1, 2, 3], RepresentationKind::Cpk, true),
                rep("b", vec![1, 2], RepresentationKind::BallAndStick, false),
            ]),
            &[],
            &[],
            crate::surface_render::SurfaceBatches::empty_ref(),
        );
        assert_eq!((scene.atoms.len(), scene.bonds.len()), (3, 0));

        // Licorice: 結合は中点で 2 つに分かれる。球は端に 1 つずつ
        let scene = build_scene(
            &state_with(vec![rep(
                "l",
                vec![1, 2],
                RepresentationKind::Licorice,
                true,
            )]),
            &[],
            &[],
            crate::surface_render::SurfaceBatches::empty_ref(),
        );
        assert_eq!((scene.atoms.len(), scene.bonds.len()), (2, 2));
    }

    #[test]
    fn test_render_view_svg_draws_the_scene_from_the_camera() {
        let state = state_with(vec![]);
        let camera = crate::menu_bar::default_camera_state();
        let svg = render_view_svg(
            &state,
            &[],
            &[],
            crate::surface_render::SurfaceBatches::empty_ref(),
            &camera,
            (1600, 1200),
        )
        .unwrap();
        assert!(svg.starts_with("<svg "));
        assert!(svg.trim_end().ends_with("</svg>"));
        assert!(svg.contains("viewBox=\"0 0 1600 1200\""), "{svg}");
        assert_eq!(svg.matches("<circle ").count(), 3);
        assert_eq!(svg.matches("<line ").count(), 2);
        assert!(
            render_view_svg(
                &state,
                &[],
                &[],
                crate::surface_render::SurfaceBatches::empty_ref(),
                &camera,
                (0, 0)
            )
            .is_err()
        );
        // 大きさを指定すると、その大きさのキャンバスになる
        let wide = render_view_svg(
            &state,
            &[],
            &[],
            crate::surface_render::SurfaceBatches::empty_ref(),
            &camera,
            (1920, 1080),
        )
        .unwrap();
        assert!(wide.contains("viewBox=\"0 0 1920 1080\""), "{wide}");
    }

    #[test]
    fn test_interaction_lines_are_dashed_lines_that_follow_the_interaction_cards() {
        use yui_core::interaction::resolve_all_interactions;
        use yui_core::structure_loader::load_structure_into_state;
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/tests/data/1hls.pdb");
        let mut state = AppState::default();
        load_structure_into_state(&mut state, &std::fs::read(path).unwrap(), "1hls.pdb").unwrap();
        let ag = state.atom_group.as_ref().unwrap();
        let resolved = resolve_all_interactions(ag, &state.interactions);
        assert!(!resolved.is_empty(), "1HLS has interactions");

        let count = |state: &AppState| {
            let scene = build_scene(
                state,
                &[],
                &resolved,
                crate::surface_render::SurfaceBatches::empty_ref(),
            );
            (
                scene.bonds.iter().filter(|b| b.dashed).count(),
                scene.bonds.iter().filter(|b| !b.dashed).count(),
            )
        };
        // 相互作用のカードがなければ、線は入らない
        let (dashed, solid) = count(&state);
        assert_eq!(dashed, 0);
        assert!(solid > 0);

        // カードを足すと、画面と同じ本数の破線が入る（結合は変わらない）
        state.representations.push(Representation::new(
            "inter",
            Selector::All,
            RepresentationKind::Interaction,
            ColorScheme::Element,
        ));
        let expected = crate::interaction_render::build_interaction_lines_from_resolved(
            state.atom_group.as_ref(),
            &resolved,
            &state.representations,
            None,
            &[],
        );
        assert!(!expected.is_empty());
        let (dashed, solid_after) = count(&state);
        assert_eq!(dashed, expected.len());
        assert_eq!(solid_after, solid);
        let scene = build_scene(
            &state,
            &[],
            &resolved,
            crate::surface_render::SurfaceBatches::empty_ref(),
        );
        let line = scene.bonds.iter().find(|b| b.dashed).unwrap();
        assert_eq!(
            line.radius,
            crate::viewport::scene::INTERACTION_STICK_RADIUS
        );

        // 透明度は色の透明度に乗る。非表示のカードは入らない
        state.representations.last_mut().unwrap().opacity = 0.5;
        let faded = build_scene(
            &state,
            &[],
            &resolved,
            crate::surface_render::SurfaceBatches::empty_ref(),
        );
        assert!(
            faded
                .bonds
                .iter()
                .filter(|b| b.dashed)
                .all(|b| b.color[3] <= 0.5 + 1e-6)
        );
        state.representations.last_mut().unwrap().visible = false;
        assert_eq!(count(&state).0, 0);

        // SVG の文字列にも、破線として出る
        state.representations.last_mut().unwrap().visible = true;
        let camera = crate::menu_bar::default_camera_state();
        let svg = render_view_svg(
            &state,
            &[],
            &resolved,
            crate::surface_render::SurfaceBatches::empty_ref(),
            &camera,
            (1024, 768),
        )
        .unwrap();
        assert!(svg.contains("stroke-dasharray"));
    }

    #[test]
    fn test_svg_dialog_options() {
        let mut o = SvgOptions::default();
        o.apply(SvgDialogAction::SetWidth(3200));
        o.apply(SvgDialogAction::Canvas(CanvasAction::FitAll(true)));
        assert_eq!((o.width, o.canvas.fit_all), (3200, true));
        let before = o;
        o.apply(SvgDialogAction::SetWidth(1234));
        o.apply(SvgDialogAction::Export);
        o.apply(SvgDialogAction::Cancel);
        assert_eq!(o, before);
        let _: Element<'_, SvgDialogAction> = view_svg_dialog(o, |a| a);
    }
}
