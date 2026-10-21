//! 描画のギャラリー（見た目の確認を自動化するための、画面なしの描画）。
//!
//! 機能ごとの場面（表現の種類、分子表面、相互作用、CUBE、隠すカード、書き出しの設定、アニメーションの
//! フレーム）を、画面と同じ描画の準備（`offscreen_primitive_with`）で、画面外に描いて PNG にし、
//! まとめた一覧の画像（コンタクトシート）も作る。各場面で「描かれていること」を機械的に確かめ、
//! 画像は人（や AI）が見て確かめる。画面を開かず、ソフトウェアの描画（llvmpipe など）で動く。
//!
//! 実行（通常のテストには含めない）: `scripts/render-gallery.sh [出力先]`
//! （内部では `YUI_GALLERY_DIR=<出力先> cargo test -p yui-frontend gallery -- --ignored --nocapture`）

use std::path::PathBuf;

use super::*;
use crate::file_open_dispatch::FileOpenHandler;
use crate::viewport::offscreen::{OffscreenRenderer, downsample_box, encode_png};
use yui_core::representation::{ColorScheme, Representation, RepresentationKind};
use yui_core::selector::Selector;

const TILE_W: u32 = 320;
const TILE_H: u32 = 240;
const COLUMNS: usize = 4;

/// 描いた 1 枚（名前と RGBA）
struct Tile {
    name: String,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

struct Gallery {
    renderer: OffscreenRenderer,
    dir: Option<PathBuf>,
    groups: Vec<(String, Vec<Tile>)>,
    /// 設定されていれば、描くときの SSAO の強さ（設定ファイルの値の代わり）
    ssao: Option<f32>,
}

impl Gallery {
    /// GPU のアダプターがなければ `None`（読み飛ばす）
    fn new() -> Option<Self> {
        let renderer = iced::futures::executor::block_on(OffscreenRenderer::new()).ok()?;
        eprintln!("gallery: adapter = {}", renderer.gpu.info);
        let dir = std::env::var("YUI_GALLERY_DIR").ok().map(PathBuf::from);
        if let Some(d) = &dir {
            std::fs::create_dir_all(d).expect("create the gallery directory");
        }
        Some(Self {
            renderer,
            dir,
            groups: Vec::new(),
            ssao: None,
        })
    }

    fn group(&mut self, name: &str) {
        self.groups.push((name.to_string(), Vec::new()));
    }

    /// 場面を描く。`width` x `height` で、2 倍で描いて縮める。背景と違う画素の割合（%）を返す。
    fn shoot(&mut self, name: &str, app: &Yui, width: u32, height: u32, transparent: bool) -> f32 {
        self.shoot_with(name, app, &app.state.camera, width, height, transparent)
    }

    /// `shoot` の、視点を指定する版
    fn shoot_with(
        &mut self,
        name: &str,
        app: &Yui,
        camera: &yui_core::state::CameraState,
        width: u32,
        height: u32,
        transparent: bool,
    ) -> f32 {
        let primitive = app.offscreen_primitive_with(
            width * 2,
            height * 2,
            transparent,
            camera,
            &app.surface_batches,
        );
        let mut primitive = primitive;
        if let Some(strength) = self.ssao {
            primitive.ssao_strength = strength;
            primitive.ssao_radius = 4.0;
        }
        let rgba = iced::futures::executor::block_on(self.renderer.render(
            &primitive,
            width * 2,
            height * 2,
        ))
        .expect("render the scene");
        let (small, w, h) = downsample_box(&rgba, width * 2, height * 2, 2, transparent);
        let covered = coverage_percent(&small, transparent);
        self.push(name, small, w, h);
        covered
    }

    fn push(&mut self, name: &str, rgba: Vec<u8>, width: u32, height: u32) {
        let index: usize = self.groups.iter().map(|(_, t)| t.len()).sum::<usize>() + 1;
        let file = format!("{index:02}_{name}.png");
        if let Some(dir) = &self.dir {
            let png = encode_png(&rgba, width, height).expect("encode the tile");
            std::fs::write(dir.join(&file), png).expect("write the tile");
        }
        self.groups
            .last_mut()
            .expect("a group is open")
            .1
            .push(Tile {
                name: file,
                width,
                height,
                rgba,
            });
    }

    /// グループごとのコンタクトシートと、一覧（manifest.txt）を書く
    fn finish(self) {
        let Some(dir) = self.dir else { return };
        let mut manifest = String::new();
        for (gi, (group, tiles)) in self.groups.iter().enumerate() {
            if tiles.is_empty() {
                continue;
            }
            let rows = tiles.len().div_ceil(COLUMNS);
            let gap = 8u32;
            let sheet_w = COLUMNS as u32 * TILE_W + (COLUMNS as u32 + 1) * gap;
            let sheet_h = rows as u32 * TILE_H + (rows as u32 + 1) * gap;
            let mut sheet = vec![0u8; (sheet_w * sheet_h * 4) as usize];
            for px in sheet.as_chunks_mut::<4>().0 {
                *px = [96, 96, 104, 255];
            }
            manifest.push_str(&format!("sheet_{:02}_{group}.png:\n", gi + 1));
            for (i, t) in tiles.iter().enumerate() {
                let (col, row) = ((i % COLUMNS) as u32, (i / COLUMNS) as u32);
                let (x0, y0) = (gap + col * (TILE_W + gap), gap + row * (TILE_H + gap));
                blit(&mut sheet, sheet_w, t, x0, y0);
                manifest.push_str(&format!("  [{},{}] {}\n", row + 1, col + 1, t.name));
            }
            let png = encode_png(&sheet, sheet_w, sheet_h).expect("encode the sheet");
            std::fs::write(dir.join(format!("sheet_{:02}_{group}.png", gi + 1)), png)
                .expect("write the sheet");
        }
        std::fs::write(dir.join("manifest.txt"), manifest).expect("write the manifest");
        eprintln!("gallery: wrote the images to {}", dir.display());
    }
}

/// タイルを、透明なところを市松模様にして、シートに貼る
fn blit(sheet: &mut [u8], sheet_w: u32, tile: &Tile, x0: u32, y0: u32) {
    for y in 0..tile.height.min(TILE_H) {
        for x in 0..tile.width.min(TILE_W) {
            let s = ((y * tile.width + x) * 4) as usize;
            let a = tile.rgba[s + 3] as f32 / 255.0;
            let check = if ((x / 16) + (y / 16)) % 2 == 0 {
                200.0
            } else {
                150.0
            };
            let d = (((y0 + y) * sheet_w + x0 + x) * 4) as usize;
            for c in 0..3 {
                sheet[d + c] = (tile.rgba[s + c] as f32 * a + check * (1.0 - a)).round() as u8;
            }
            sheet[d + 3] = 255;
        }
    }
}

/// 背景（隅の画素。透明な背景では、透明度 0）と違う画素の割合（%）
fn coverage_percent(rgba: &[u8], transparent: bool) -> f32 {
    let (pixels, _) = rgba.as_chunks::<4>();
    let corner = pixels[0];
    let differ = pixels
        .iter()
        .filter(|p| if transparent { p[3] > 8 } else { **p != corner })
        .count();
    100.0 * differ as f32 / pixels.len() as f32
}

/// 構造を開いたアプリ。視点は、構造全体が `aspect` に収まるように合わせる。
fn app_with(file: &str, aspect: f32) -> Yui {
    let mut app = Yui::default();
    let path = format!("{}/../core/tests/data/{file}", env!("CARGO_MANIFEST_DIR"));
    let bytes = std::fs::read(path).expect("read the test structure");
    let _ = app.update(Message::FileContentFetched {
        path: file.to_string(),
        handler: FileOpenHandler::StructureViewer,
        result: Ok(bytes),
    });
    fit_camera(&mut app, aspect);
    app
}

fn fit_camera(app: &mut Yui, aspect: f32) {
    let points = viewport::fit_target_positions(
        &app.state.molecule,
        app.state.active_model.as_deref(),
        &app.model_atom_ranges,
    );
    let fov = crate::menu_bar::default_camera_state().fov;
    if let Some(cam) = crate::viewport::camera::fit_camera_to_points(points, fov, aspect) {
        app.state.camera = cam;
    }
}

fn rep(id: &str, kind: RepresentationKind, color: ColorScheme) -> Representation {
    Representation::new(id, Selector::All, kind, color)
}

/// 表現を入れ替える
fn with_reps(app: &mut Yui, reps: Vec<Representation>) {
    app.state.representations = reps;
}

/// 分子表面を、今の状態から同期的に作って、描画のバッチに入れる（画面では、非同期で行う）
fn build_surfaces(app: &mut Yui) {
    let requests = app.surface_build_requests();
    let ctx = crate::surface_render::SurfaceColorContext::new(
        &app.state.molecule.atoms,
        app.state.atom_group.as_ref(),
        &app.state.secondary_structure_map,
    );
    let results = crate::surface_render::process_surface_build_requests(&requests);
    app.surface_entries = ctx.entries_from_results(results);
    app.refresh_surface_batches();
}

/// CUBE を開いて、等値面まで計算する（画面では、非同期で行う）
fn open_cube(app: &mut Yui, file: &str, dataset: usize) {
    let text = std::fs::read_to_string(format!(
        "{}/../core/tests/data/{file}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("read the CUBE");
    let set = std::sync::Arc::new(yui_core::cube_iso::CubeSet::parse(&text).expect("parse"));
    let _ = app.update(Message::CubeOpened(Ok(Some((file.to_string(), set, None)))));
    if dataset != 0 {
        let _ = app.update(Message::CubeAction(
            crate::cube_view::CubeAction::SetDataset(dataset),
        ));
    }
    finish_cube_jobs(app);
}

#[test]
#[ignore = "renders the gallery; run scripts/render-gallery.sh"]
fn gallery_renders_all_scenes() {
    let Some(mut g) = Gallery::new() else {
        eprintln!("gallery: skipped (no graphics adapter)");
        return;
    };
    let aspect = TILE_W as f32 / TILE_H as f32;
    let check = |name: &str, covered: f32, min_percent: f32| {
        assert!(
            covered >= min_percent,
            "scene '{name}': only {covered:.2}% of the pixels are drawn (expected at least {min_percent}%)"
        );
    };

    // --- 表現の種類と配色（1HLS）
    g.group("representations");
    let mut app = app_with("1hls.pdb", aspect);
    for (name, reps) in [
        (
            "ball_and_stick",
            vec![rep(
                "a",
                RepresentationKind::BallAndStick,
                ColorScheme::Element,
            )],
        ),
        (
            "cartoon_and_ball_and_stick",
            yui_core::representation::default_representations(),
        ),
        (
            "cartoon_only",
            vec![rep(
                "a",
                RepresentationKind::Cartoon,
                ColorScheme::SecondaryStructure,
            )],
        ),
        (
            "spacefill",
            vec![rep("a", RepresentationKind::Cpk, ColorScheme::Element)],
        ),
        (
            "licorice",
            vec![rep("a", RepresentationKind::Licorice, ColorScheme::Element)],
        ),
        (
            "line",
            vec![rep("a", RepresentationKind::Line, ColorScheme::Element)],
        ),
        (
            "ball_and_stick_chain_colors",
            vec![rep(
                "a",
                RepresentationKind::BallAndStick,
                ColorScheme::Chain,
            )],
        ),
        (
            "ball_and_stick_residue_colors",
            vec![rep(
                "a",
                RepresentationKind::BallAndStick,
                ColorScheme::Residue,
            )],
        ),
    ] {
        with_reps(&mut app, reps);
        let c = g.shoot(name, &app, TILE_W, TILE_H, false);
        check(name, c, 0.5);
    }

    // --- 重ねる、隠す、目のアイコン
    g.group("layers_and_hiding");
    let mut mixed = vec![
        rep(
            "cartoon",
            RepresentationKind::Cartoon,
            ColorScheme::SecondaryStructure,
        ),
        Representation::new(
            "licorice_a",
            Selector::ResidueRange {
                chain: Some("A".to_string()),
                start: 1,
                end: 60,
            },
            RepresentationKind::Licorice,
            ColorScheme::Element,
        ),
        Representation::new(
            "spacefill_b",
            Selector::ResidueRange {
                chain: Some("B".to_string()),
                start: 1,
                end: 60,
            },
            RepresentationKind::Cpk,
            ColorScheme::Chain,
        ),
    ];
    with_reps(&mut app, mixed.clone());
    let c = g.shoot(
        "cartoon_licorice_spacefill_layers",
        &app,
        TILE_W,
        TILE_H,
        false,
    );
    check("layers", c, 0.5);
    // 目のアイコンでカートゥーンだけを消す（ほかのカードは残る）
    mixed[0].visible = false;
    with_reps(&mut app, mixed.clone());
    let c = g.shoot(
        "eye_off_cartoon_only_that_card",
        &app,
        TILE_W,
        TILE_H,
        false,
    );
    check("eye off", c, 0.2);
    // 「隠す」カード（ツリーの Hide）: 選んだ原子がすべてのカードから消える
    mixed[0].visible = true;
    mixed.push(
        Representation::new(
            "hide",
            Selector::ResidueRange {
                chain: Some("A".to_string()),
                start: 1,
                end: 60,
            },
            RepresentationKind::BallAndStick,
            ColorScheme::Element,
        )
        .with_hides_atoms(true),
    );
    with_reps(&mut app, mixed);
    let c = g.shoot("hide_card_hides_chain_a_atoms", &app, TILE_W, TILE_H, false);
    check("hide", c, 0.2);

    // --- 分子表面
    g.group("surfaces");
    let mut app = app_with("1hls.pdb", aspect);
    for (name, reps) in [
        (
            "surface_chain_colors",
            vec![rep("s", RepresentationKind::Surface, ColorScheme::Chain)],
        ),
        (
            "surface_opacity_50_over_ball_and_stick",
            vec![
                rep("bb", RepresentationKind::BallAndStick, ColorScheme::Element),
                rep(
                    "s",
                    RepresentationKind::Surface,
                    ColorScheme::Uniform([0.3, 0.6, 1.0, 1.0]),
                )
                .with_opacity(0.5),
            ],
        ),
        (
            "surface_and_cartoon",
            vec![
                rep(
                    "c",
                    RepresentationKind::Cartoon,
                    ColorScheme::SecondaryStructure,
                ),
                rep("s", RepresentationKind::Surface, ColorScheme::Residue).with_opacity(0.6),
            ],
        ),
    ] {
        with_reps(&mut app, reps);
        build_surfaces(&mut app);
        let c = g.shoot(name, &app, TILE_W, TILE_H, false);
        check(name, c, 1.0);
    }

    // --- SSAO（隅や隙間を暗くする）。同じ構造を、オフとオンで、大きく描き比べる
    g.group("ssao");
    let mut app = app_with("1hls.pdb", 4.0 / 3.0);
    let (sw, sh) = (640, 480);
    for (name, reps) in [
        (
            "spacefill",
            vec![rep("c", RepresentationKind::Cpk, ColorScheme::Chain)],
        ),
        (
            "ball_and_stick_cartoon",
            vec![
                rep(
                    "c",
                    RepresentationKind::Cartoon,
                    ColorScheme::SecondaryStructure,
                ),
                rep("b", RepresentationKind::BallAndStick, ColorScheme::Element),
            ],
        ),
        (
            "surface",
            vec![rep("s", RepresentationKind::Surface, ColorScheme::Chain)],
        ),
    ] {
        with_reps(&mut app, reps);
        build_surfaces(&mut app);
        let cam = app.state.camera.clone();
        let mut sums = Vec::new();
        for (label, strength) in [("off", None), ("on", Some(0.9f32))] {
            g.ssao = strength;
            let c = g.shoot_with(&format!("ssao_{name}_{label}"), &app, &cam, sw, sh, false);
            check(name, c, 0.5);
            let tile = &g.groups.last().unwrap().1[g.groups.last().unwrap().1.len() - 1];
            let sum: u64 = tile
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| p[0] as u64 + p[1] as u64 + p[2] as u64)
                .sum();
            sums.push(sum);
        }
        g.ssao = None;
        // SSAO は暗くするだけで、真っ黒にはしない
        assert!(
            sums[1] < sums[0],
            "ssao '{name}': the image did not get darker ({} -> {})",
            sums[0],
            sums[1]
        );
        assert!(
            sums[1] * 100 > sums[0] * 55,
            "ssao '{name}': the image got much too dark ({} -> {})",
            sums[0],
            sums[1]
        );
        eprintln!(
            "gallery: ssao {name}: brightness {:.1}% of the plain image",
            100.0 * sums[1] as f64 / sums[0] as f64
        );
    }

    // --- SVG の書き出し（リボンと分子表面）。SVG は画像にしないので、ファイルに書いて、数を確かめる
    let mut svg_app = app_with("1hls.pdb", aspect);
    let write_svg = |name: &str, app: &Yui, polygons: std::ops::RangeInclusive<usize>| {
        let ((w, h), cam) = app
            .export_canvas(&crate::canvas::CanvasOptions::default(), 800)
            .expect("canvas");
        let svg = crate::svg_export::render_view_svg(
            &app.state,
            &app.model_atom_ranges,
            &app.resolved_interactions,
            &app.surface_batches,
            &cam,
            (w, h),
        )
        .expect("svg");
        let n = svg.matches("<polygon").count();
        assert!(
            polygons.contains(&n),
            "svg '{name}': {n} polygons, expected {polygons:?}"
        );
        if let Some(dir) = &g.dir {
            std::fs::write(dir.join(format!("svg_{name}.svg")), &svg).expect("write the svg");
        }
        eprintln!("gallery: svg {name}: {n} polygons, {} bytes", svg.len());
    };
    with_reps(
        &mut svg_app,
        vec![rep(
            "c",
            RepresentationKind::Cartoon,
            ColorScheme::SecondaryStructure,
        )],
    );
    write_svg("cartoon", &svg_app, 1000..=200_000);
    with_reps(
        &mut svg_app,
        vec![rep("s", RepresentationKind::Surface, ColorScheme::Chain)],
    );
    build_surfaces(&mut svg_app);
    write_svg("surface", &svg_app, 1000..=200_000);
    with_reps(
        &mut svg_app,
        vec![
            rep(
                "c",
                RepresentationKind::Cartoon,
                ColorScheme::SecondaryStructure,
            ),
            rep("s", RepresentationKind::Surface, ColorScheme::Residue).with_opacity(0.4),
        ],
    );
    build_surfaces(&mut svg_app);
    write_svg("cartoon_in_translucent_surface", &svg_app, 2000..=400_000);

    // --- 非共有結合相互作用（カードがあるときと、ないときの違いを確かめる。大きく描く）
    g.group("interactions");
    let mut app = app_with("1hls.pdb", 4.0 / 3.0);
    let base_reps = vec![
        rep(
            "c",
            RepresentationKind::Cartoon,
            ColorScheme::SecondaryStructure,
        ),
        rep("bb", RepresentationKind::BallAndStick, ColorScheme::Element),
    ];
    with_reps(&mut app, base_reps.clone());
    g.shoot("without_interaction_card", &app, 640, 480, false);
    let mut with_card = base_reps;
    with_card.push(rep(
        "i",
        RepresentationKind::Interaction,
        ColorScheme::Element,
    ));
    with_reps(&mut app, with_card);
    g.shoot("with_interaction_card_dashed_lines", &app, 640, 480, false);
    let tiles = &g.groups.last().unwrap().1;
    let (without, with) = (&tiles[0].rgba, &tiles[1].rgba);
    // 黄・橙・青緑・紫（既定の相互作用の色）に近い、新しく加わった画素がある
    let added = without
        .as_chunks::<4>()
        .0
        .iter()
        .zip(with.as_chunks::<4>().0)
        .filter(|(a, b)| a != b)
        .count();
    assert!(
        added > 100,
        "the interaction card adds lines ({added} pixels differ)"
    );

    // --- CUBE
    g.group("cube");
    let mut app = Yui::default();
    open_cube(&mut app, "sphere.cube", 0);
    fit_camera(&mut app, aspect);
    let c = g.shoot(
        "cube_density_sphere_with_its_atom",
        &app,
        TILE_W,
        TILE_H,
        false,
    );
    check("cube sphere", c, 1.0);
    let mut app = Yui::default();
    open_cube(&mut app, "orbitals.cube", 1);
    fit_camera(&mut app, aspect);
    let c = g.shoot(
        "cube_p_orbital_blue_and_red_lobes",
        &app,
        TILE_W,
        TILE_H,
        false,
    );
    check("cube p orbital", c, 1.0);
    // 等値レベルを上げると、面が小さくなる（スライダーの操作と同じ）
    let low = g.shoot("cube_p_orbital_default_level", &app, TILE_W, TILE_H, false);
    let _ = app.update(Message::CubeAction(
        crate::cube_view::CubeAction::SetIsolevel(0.12),
    ));
    finish_cube_jobs(&mut app);
    let high = g.shoot(
        "cube_p_orbital_isolevel_0_12_smaller",
        &app,
        TILE_W,
        TILE_H,
        false,
    );
    check("cube level", high, 0.2);
    assert!(
        high < low,
        "a higher isolevel gives a smaller surface ({high} vs {low})"
    );
    // 色を替える
    let _ = app.update(Message::CubeAction(
        crate::cube_view::CubeAction::SetPositiveColor(2),
    ));
    let _ = app.update(Message::CubeAction(
        crate::cube_view::CubeAction::SetNegativeColor(3),
    ));
    let c = g.shoot(
        "cube_p_orbital_green_and_orange",
        &app,
        TILE_W,
        TILE_H,
        false,
    );
    check("cube colors", c, 0.2);

    // --- 書き出し: 透明な背景、全体に合わせる縦長のキャンバス、16:9
    g.group("export_options");
    let mut app = app_with("1hls.pdb", aspect);
    with_reps(
        &mut app,
        yui_core::representation::default_representations(),
    );
    let c = g.shoot(
        "png_transparent_background_on_checkerboard",
        &app,
        TILE_W,
        TILE_H,
        true,
    );
    check("transparent", c, 0.5);
    // 縦長（150 x 200）: 画面のまま / 全体に合わせる
    let mut canvas = crate::canvas::CanvasOptions {
        shape: crate::canvas::CanvasShape::Custom,
        custom_width: 150,
        custom_height: 200,
        ..crate::canvas::CanvasOptions::default()
    };
    let ((w, h), cam) = app.export_canvas(&canvas, 1000).expect("canvas");
    let c = g.shoot_with("portrait_as_on_screen", &app, &cam, w, h, false);
    check("portrait as on screen", c, 0.5);
    canvas.fit_all = true;
    let ((w, h), cam) = app.export_canvas(&canvas, 1000).expect("canvas");
    let c = g.shoot_with("portrait_fit_all", &app, &cam, w, h, false);
    check("portrait fit all", c, 0.5);
    canvas.shape = crate::canvas::CanvasShape::R16x9;
    let ((w, h), cam) = app.export_canvas(&canvas, 300).expect("canvas");
    let c = g.shoot_with("wide_16_9_fit_all", &app, &cam, w, h, false);
    check("16:9 fit all", c, 0.5);

    // --- アニメーションのフレーム: Spin（90° ずつ）
    g.group("animation_frames");
    let base = app.state.camera.clone();
    let mut previous: Option<Vec<u8>> = None;
    for k in 0..4 {
        let cam = crate::movie_export::camera_for_frame(
            &base,
            crate::movie_export::MovieSource::Spin,
            k,
            4,
        );
        let c = g.shoot_with(
            &format!("spin_frame_{}_of_4", k + 1),
            &app,
            &cam,
            TILE_W,
            TILE_H,
            false,
        );
        check("spin", c, 0.5);
        let current = g.groups.last().unwrap().1.last().unwrap().rgba.clone();
        if let Some(prev) = &previous {
            assert_ne!(prev, &current, "spin frames differ");
        }
        previous = Some(current);
    }
    // Frames: マルチモデルの各モデルの分子表面（フレームごとに作り直す）
    let mut multi = app_with("1HLS.cif", aspect);
    with_reps(
        &mut multi,
        vec![
            rep(
                "c",
                RepresentationKind::Cartoon,
                ColorScheme::SecondaryStructure,
            ),
            rep("s", RepresentationKind::Surface, ColorScheme::Chain).with_opacity(0.55),
        ],
    );
    let mut surfaces = Vec::new();
    for frame in [0usize, 7, 15] {
        multi.show_frame(frame);
        fit_camera(&mut multi, aspect);
        build_surfaces(&mut multi);
        let c = g.shoot(
            &format!("frames_model_{}_with_surface", frame + 1),
            &multi,
            TILE_W,
            TILE_H,
            false,
        );
        check("frames", c, 0.5);
        surfaces.push(g.groups.last().unwrap().1.last().unwrap().rgba.clone());
    }
    assert!(
        surfaces[0] != surfaces[1] && surfaces[1] != surfaces[2],
        "each model is its own picture"
    );

    g.finish();
}

/// 計算中の CUBE の依頼を、同期的に終わらせる
fn finish_cube_jobs(app: &mut Yui) {
    for _ in 0..4 {
        let Some(cube) = app.cube.as_ref() else { break };
        if !cube.busy {
            break;
        }
        let job = cube.job(&app.state.cube_view);
        let _ = app.update(Message::CubeJobDone(crate::cube_view::run_job(job)));
    }
}
