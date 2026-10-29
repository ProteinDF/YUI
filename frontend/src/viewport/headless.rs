//! 画面なしの描画テスト。
//!
//! 固定の構造（1HLS）を、決めた表現で画面外のテクスチャへ描き、背景と違う画素の数を数える。
//! 表現ごとの画素数の大小関係で、「描かれていること」を確かめる。
//! wgpu のアダプターが見つからない環境（GPU も Mesa のソフトウェア描画もない）では、
//! 失敗ではなく読み飛ばす。必要な環境は TESTING.md 7章を参照。
//!
//! 実行: `cargo test -p yui-frontend headless -- --nocapture`

use std::collections::BTreeSet;

use iced::futures::executor::block_on;
use iced::widget::shader::Program;
use iced::{Rectangle, Size, mouse};

use super::offscreen::{OffscreenGpu, render_rgba, request_gpu};
use super::pipeline::ViewportId;
use super::{ViewportProgram, ViewportState};
use yui_core::representation::{ColorScheme, Representation, RepresentationKind};
use yui_core::selector::Selector;
use yui_core::state::AppState;

const SIZE: u32 = 256;

fn rep(id: &str, kind: RepresentationKind, visible: bool) -> Representation {
    let mut r = Representation::new(id, Selector::All, kind, ColorScheme::Element);
    r.visible = visible;
    r
}

/// 構造を読み込み、表現を与えて 256x256 に描き、RGBA の画素列を返す。
fn render(gpu: &OffscreenGpu, state: &AppState, reps: &[Representation]) -> Vec<u8> {
    let ranges = state
        .atom_group
        .as_ref()
        .map(|g| yui_core::atom_group::calculate_model_atom_ranges(&g.0))
        .unwrap_or_default();
    let camera = crate::menu_bar::default_camera_state();
    let selection = BTreeSet::new();
    let sizes = crate::viewport::ViewportSizes::default();
    let program = ViewportProgram::<()> {
        sizes: &sizes,
        id: ViewportId::Primary,
        camera: &camera,
        molecule: &state.molecule,
        atom_group: state.atom_group.as_ref(),
        secondary_structure_map: &state.secondary_structure_map,
        representations: reps,
        resolved_interactions: &[],
        active_model: state.active_model.as_deref(),
        model_atom_ranges: &ranges,
        surface_batches: crate::surface_render::SurfaceBatches::empty_ref(),
        background_color: state.scene.background_color,
        selection: &selection,
        highlight: crate::selection::HighlightStyle::Off,
        selection_color: state.scene.background_color,
        camera_fit_token: 0,
        fit_pending: true,
        fit_selection: false,
        on_camera_changed: None,
        on_click: None,
        on_right_click: None,
    };
    let bounds = Rectangle::new(iced::Point::ORIGIN, Size::new(SIZE as f32, SIZE as f32));
    let primitive = program.draw(
        &ViewportState::default(),
        mouse::Cursor::Unavailable,
        bounds,
    );
    block_on(render_rgba(gpu, &primitive, SIZE, SIZE)).expect("render off-screen")
}

/// 基準の画像と違う画素の数
fn diff_pixels(base: &[u8], img: &[u8]) -> usize {
    let (base, _) = base.as_chunks::<4>();
    let (img, _) = img.as_chunks::<4>();
    base.iter().zip(img).filter(|(a, b)| a != b).count()
}

#[test]
fn representations_draw_expected_pixels() {
    let _gpu = crate::test_gpu::lock();
    let Ok(gpu) = block_on(request_gpu()) else {
        eprintln!("headless: skipped (no wgpu adapter found; see TESTING.md section 7)");
        return;
    };
    eprintln!("headless: adapter = {}", gpu.info);

    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/tests/data/1hls.pdb");
    let bytes = std::fs::read(path).expect("read 1hls.pdb");
    let mut state = AppState::default();
    yui_core::structure_loader::load_structure_into_state(&mut state, &bytes, "1hls.pdb")
        .expect("load 1hls.pdb");

    use RepresentationKind::*;
    let count = |reps: Vec<Representation>, base: &[u8]| -> usize {
        diff_pixels(base, &render(&gpu, &state, &reps))
    };

    // 基準: すべての表現を非表示にした画像（背景だけ）
    let base = render(&gpu, &state, &[rep("off", BallAndStick, false)]);
    let bb = count(vec![rep("bb", BallAndStick, true)], &base);
    let cartoon_bb = count(
        vec![rep("c", Cartoon, true), rep("bb", BallAndStick, true)],
        &base,
    );
    let cpk_bb = count(
        vec![rep("cpk", Cpk, true), rep("bb", BallAndStick, true)],
        &base,
    );
    let licorice = count(vec![rep("l", Licorice, true)], &base);
    let line = count(vec![rep("ln", Line, true)], &base);
    eprintln!(
        "headless: pixels bb={bb} cartoon+bb={cartoon_bb} cpk+bb={cpk_bb} licorice={licorice} line={line}"
    );

    assert!(bb > 1000, "ball-and-stick should draw many pixels");
    assert!(cartoon_bb > bb, "adding a cartoon should add pixels");
    assert!(cpk_bb > bb, "adding spacefill should add pixels");
    assert!(
        licorice > 0 && licorice < bb,
        "licorice is thinner than ball-and-stick"
    );
    assert!(
        line > 0 && line < licorice,
        "lines are thinner than licorice"
    );
}

/// `draw` は、描いたペインの大きさを `ViewportSizes` に残す（書き出しの `3D pane` の縦横比に使う）。
/// GPU は要らない（`draw` は描画の命令を作るだけ）。
#[test]
fn draw_records_the_size_of_the_pane() {
    let state = AppState::default();
    let camera = crate::menu_bar::default_camera_state();
    let selection = BTreeSet::new();
    let sizes = super::ViewportSizes::default();
    assert_eq!(sizes.get(ViewportId::Primary), None, "nothing drawn yet");
    let draw = |id: ViewportId, w: f32, h: f32| {
        let program = ViewportProgram::<()> {
            sizes: &sizes,
            id,
            camera: &camera,
            molecule: &state.molecule,
            atom_group: None,
            secondary_structure_map: &state.secondary_structure_map,
            representations: &[],
            resolved_interactions: &[],
            active_model: None,
            model_atom_ranges: &[],
            surface_batches: crate::surface_render::SurfaceBatches::empty_ref(),
            background_color: state.scene.background_color,
            selection: &selection,
            highlight: crate::selection::HighlightStyle::Off,
            selection_color: state.scene.background_color,
            camera_fit_token: 0,
            fit_pending: false,
            fit_selection: false,
            on_camera_changed: None,
            on_click: None,
            on_right_click: None,
        };
        let bounds = Rectangle::new(iced::Point::ORIGIN, Size::new(w, h));
        let _ = program.draw(
            &ViewportState::default(),
            mouse::Cursor::Unavailable,
            bounds,
        );
    };
    draw(ViewportId::Primary, 640.0, 320.0);
    assert_eq!(sizes.get(ViewportId::Primary), Some((640.0, 320.0)));
    assert_eq!(sizes.get(ViewportId::Secondary), None, "per viewport");
    draw(ViewportId::Secondary, 300.5, 200.25);
    draw(ViewportId::Primary, 801.0, 599.0);
    assert_eq!(sizes.get(ViewportId::Primary), Some((801.0, 599.0)));
    assert_eq!(sizes.get(ViewportId::Secondary), Some((300.5, 200.25)));
    // 大きさが使えないとき（0 や NaN）は、記録を使わない
    draw(ViewportId::Primary, 0.0, 0.0);
    assert_eq!(sizes.get(ViewportId::Primary), None);
    draw(ViewportId::Primary, f32::NAN, 100.0);
    assert_eq!(sizes.get(ViewportId::Primary), None);
}
