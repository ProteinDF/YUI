//! Gaussian CUBE ファイルの等値面を表示する（File → Open CUBE (Isosurface)...）。
//!
//! 読み込みと格子への変換（`yui_core::cube_iso`。内部で `proteindf_bridge::format::Cube`）、正負の等値面の抽出は、
//! バックグラウンドで行う。結果は、分子表面と同じ描画のバッチ（半透明）に足して描く。
//! 構造とは独立で、CUBE の原子は表示しない。等値レベルは、カード（Right ペインの上）のスライダーで変え、軌道の CUBE では軌道を選べる。

use std::sync::Arc;

use iced::widget::{Space, button, column, container, row, slider, text};
use iced::{Alignment, Border, Element, Length, Padding};

use crate::ribbon::RibbonVertex;
use crate::surface_render::SurfaceMeshEntry;
use crate::ui_style::palette;
use yui_core::cube_iso::{CubeSet, default_isolevel, extract_signed_isosurfaces, grid_peak};
use yui_core::surface::ScalarGrid;
use yui_core::surface::SurfaceMesh;
use yui_core::{ColorScheme, state::Color4};

/// 等値面の色の候補（名前、色）。正の面の既定は 0（青）、負の面の既定は 1（赤）。
pub const COLOR_PRESETS: [(&str, Color4); 6] = [
    ("Blue", [0.2, 0.45, 1.0, 1.0]),
    ("Red", [1.0, 0.3, 0.3, 1.0]),
    ("Green", [0.25, 0.75, 0.35, 1.0]),
    ("Orange", [1.0, 0.6, 0.15, 1.0]),
    ("Purple", [0.6, 0.35, 0.85, 1.0]),
    ("Yellow", [0.95, 0.85, 0.2, 1.0]),
];

/// 等値面の抽出まで済ませた結果
#[derive(Debug)]
pub struct CubeIsosurface {
    pub dataset: usize,
    pub grid: Arc<ScalarGrid>,
    /// 格子の最大の絶対値
    pub peak: f32,
    pub isolevel: f32,
    pub positive: SurfaceMesh,
    pub negative: SurfaceMesh,
}

/// 開いている CUBE の状態。`dataset` と `isolevel` は「表示したいもの」で、`shown` が今表示しているもの。
#[derive(Debug)]
pub struct CubeState {
    pub file_name: String,
    pub set: Arc<CubeSet>,
    pub shown: Option<Arc<CubeIsosurface>>,
    /// バックグラウンドの計算が走っているか
    pub busy: bool,
}

impl CubeState {
    pub fn new(file_name: String, set: Arc<CubeSet>) -> Self {
        Self {
            file_name,
            set,
            shown: None,
            busy: false,
        }
    }

    /// 表示したいもの（`view`）と、今表示しているものが違うか
    pub fn needs_update(&self, view: &yui_core::state::CubeView) -> bool {
        match &self.shown {
            None => true,
            Some(s) => {
                s.dataset != view.dataset || (view.isolevel > 0.0 && s.isolevel != view.isolevel)
            }
        }
    }

    /// `view` の「表示したいもの」を計算する依頼。格子が同じデータセットのものなら、変換をやり直さない。
    pub fn job(&self, view: &yui_core::state::CubeView) -> CubeJob {
        CubeJob {
            set: self.set.clone(),
            dataset: view.dataset,
            grid: self
                .shown
                .as_ref()
                .filter(|s| s.dataset == view.dataset)
                .map(|s| s.grid.clone()),
            isolevel: (view.isolevel > 0.0).then_some(view.isolevel),
        }
    }
}

/// カードから発行される操作
#[derive(Debug, Clone, PartialEq)]
pub enum CubeAction {
    /// 表示するデータセット（軌道）を選ぶ
    SetDataset(usize),
    /// 等値レベルを設定する（正のレベル。負の面は `-level`）
    SetIsolevel(f32),
    /// 正の面の色（`COLOR_PRESETS` の番号）
    SetPositiveColor(usize),
    /// 負の面の色（`COLOR_PRESETS` の番号）
    SetNegativeColor(usize),
    /// 等値面の不透明度（0.0〜1.0）
    SetOpacity(f32),
    /// 閉じる
    Close,
}

/// 計算の依頼。格子が同じデータセットのものなら、変換をやり直さない。
pub struct CubeJob {
    pub set: Arc<CubeSet>,
    pub dataset: usize,
    pub grid: Option<Arc<ScalarGrid>>,
    /// `None` なら、既定の等値レベル
    pub isolevel: Option<f32>,
}

/// 格子の用意（必要なら）と、正負の等値面の抽出
pub fn run_job(job: CubeJob) -> Result<Arc<CubeIsosurface>, String> {
    let grid = match job.grid {
        Some(g) => g,
        None => Arc::new(job.set.grid(job.dataset)?),
    };
    let peak = grid_peak(&grid);
    let isolevel = job.isolevel.unwrap_or_else(|| default_isolevel(&grid));
    let (positive, negative) = extract_signed_isosurfaces(&grid, isolevel);
    Ok(Arc::new(CubeIsosurface {
        dataset: job.dataset,
        grid,
        peak,
        isolevel,
        positive,
        negative,
    }))
}

/// `run_job` を、ネイティブ版ではバックグラウンドのスレッドで行う
pub async fn run_job_async(job: CubeJob) -> Result<Arc<CubeIsosurface>, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        tokio::task::spawn_blocking(move || run_job(job))
            .await
            .map_err(|e| format!("The CUBE file could not be processed: {e}"))?
    }
    #[cfg(target_arch = "wasm32")]
    {
        run_job(job)
    }
}

/// スライダーは、等値レベルの常用対数で動かす（下限は最大値の 1/10000）
const SLIDER_DECADES: f32 = 4.0;

/// 等値レベル → スライダーの値（対数）
pub fn isolevel_to_slider(level: f32, peak: f32) -> f32 {
    let top = peak.max(f32::MIN_POSITIVE).log10();
    level
        .max(f32::MIN_POSITIVE)
        .log10()
        .clamp(top - SLIDER_DECADES, top)
}

/// スライダーの値（対数）→ 等値レベル
pub fn slider_to_isolevel(value: f32) -> f32 {
    10f32.powf(value)
}

/// 描画用のエントリ（正と負の 2 枚。空の面は除く）。色は一色で、頂点に直接入れる。
pub fn to_entries(iso: &CubeIsosurface, view: &yui_core::state::CubeView) -> Vec<SurfaceMeshEntry> {
    let opacity = view.opacity.clamp(0.0, 1.0);
    let color_of = |i: usize| COLOR_PRESETS[i.min(COLOR_PRESETS.len() - 1)].1;
    [
        (
            "cube_positive",
            &iso.positive,
            color_of(view.positive_color),
        ),
        (
            "cube_negative",
            &iso.negative,
            color_of(view.negative_color),
        ),
    ]
    .into_iter()
    .filter(|(_, mesh, _)| !mesh.indices.is_empty())
    .map(|(id, mesh, color)| {
        let color = [color[0], color[1], color[2], opacity];
        let vertices = mesh
            .positions
            .iter()
            .zip(&mesh.normals)
            .map(|(&position, &normal)| RibbonVertex {
                position,
                normal,
                color,
            })
            .collect();
        SurfaceMeshEntry {
            rep_id: id.to_string(),
            mesh: mesh.clone(),
            color_scheme: ColorScheme::Uniform(color),
            opacity,
            visible: true,
            vertices,
            indices: mesh.indices.clone(),
        }
    })
    .collect()
}

/// 開いている CUBE のカード（Right ペインの上）
pub fn view_cube_card<'a, Message: Clone + 'a>(
    state: &'a CubeState,
    view: &yui_core::state::CubeView,
    on_action: impl Fn(CubeAction) -> Message + Copy + 'a,
) -> Element<'a, Message> {
    let small_button = |label: &'static str, action: Option<CubeAction>| {
        let b = button(text(label).size(11))
            .padding(Padding::from([2, 6]))
            .style(|_theme, status| button::Style {
                background: Some(if status == button::Status::Hovered {
                    palette().item_hover_bg.into()
                } else {
                    palette().chip_bg.into()
                }),
                text_color: palette().text_secondary,
                border: Border::default().rounded(3),
                ..button::Style::default()
            });
        match action {
            Some(a) => b.on_press(on_action(a)),
            None => b,
        }
    };

    let title = row![
        text("CUBE").size(13).color(palette().text_secondary),
        Space::new().width(Length::Fixed(6.0)),
        text(&state.file_name).size(11).color(palette().text_faint),
        Space::new().width(Length::Fill),
        small_button("✕", Some(CubeAction::Close)),
    ]
    .align_y(Alignment::Center);

    let mut card = column![title].spacing(5);

    let n = state.set.num_datasets();
    if n > 1 {
        let label = match state.set.orbital_number(view.dataset) {
            Some(o) => format!("Orbital {o} ({}/{n})", view.dataset + 1),
            None => format!("Dataset {}/{n}", view.dataset + 1),
        };
        card = card.push(
            row![
                small_button(
                    "◀",
                    (view.dataset > 0).then(|| CubeAction::SetDataset(view.dataset - 1))
                ),
                text(label).size(11).color(palette().text_value),
                small_button(
                    "▶",
                    (view.dataset + 1 < n).then(|| CubeAction::SetDataset(view.dataset + 1))
                ),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        );
    }

    match &state.shown {
        Some(shown) => {
            let top = shown.peak.max(f32::MIN_POSITIVE).log10();
            let level = if view.isolevel > 0.0 {
                view.isolevel
            } else {
                shown.isolevel
            };
            card = card.push(
                row![
                    text("Isolevel:").size(10).color(palette().text_subtle),
                    slider(
                        (top - SLIDER_DECADES)..=top,
                        isolevel_to_slider(level, shown.peak),
                        move |v| on_action(CubeAction::SetIsolevel(slider_to_isolevel(v)))
                    )
                    .step(0.01_f32)
                    .width(Length::Fill),
                    text(format!("±{level:.4}"))
                        .size(10)
                        .color(palette().text_value),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            );
            let swatches = |label: &'static str, selected: usize, make: fn(usize) -> CubeAction| {
                let mut r = row![text(label).size(10).color(palette().text_subtle)]
                    .spacing(4)
                    .align_y(Alignment::Center);
                for (i, (_, c)) in COLOR_PRESETS.iter().enumerate() {
                    let color = iced::Color::from_rgb(c[0], c[1], c[2]);
                    let is_selected = i == selected;
                    r = r.push(
                        button(
                            Space::new()
                                .width(Length::Fixed(12.0))
                                .height(Length::Fixed(12.0)),
                        )
                        .padding(0)
                        .style(move |_theme, _status| button::Style {
                            background: Some(color.into()),
                            border: Border {
                                color: if is_selected {
                                    palette().text
                                } else {
                                    palette().panel_border
                                },
                                width: if is_selected { 2.0 } else { 1.0 },
                                radius: 3.0.into(),
                            },
                            ..button::Style::default()
                        })
                        .on_press(on_action(make(i))),
                    );
                }
                r
            };
            card = card.push(swatches(
                "+ color:",
                view.positive_color,
                CubeAction::SetPositiveColor,
            ));
            card = card.push(swatches(
                "- color:",
                view.negative_color,
                CubeAction::SetNegativeColor,
            ));
            card = card.push(
                row![
                    text("Opacity:").size(10).color(palette().text_subtle),
                    slider(0.1..=1.0, view.opacity, move |v| on_action(
                        CubeAction::SetOpacity(v)
                    ))
                    .step(0.05_f32)
                    .width(Length::Fill),
                    text(format!("{:.0}%", view.opacity * 100.0))
                        .size(10)
                        .color(palette().text_value),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            );
            if shown.positive.indices.is_empty() && shown.negative.indices.is_empty() {
                card = card.push(
                    text("No surface at this level")
                        .size(10)
                        .color(palette().text_faint),
                );
            }
        }
        None => {
            card = card.push(text("Loading...").size(10).color(palette().text_faint));
        }
    }

    container(card.padding(Padding::from([6, 8])))
        .style(|_theme| container::Style {
            background: Some(palette().panel_bg.into()),
            border: Border {
                color: palette().panel_border,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..container::Style::default()
        })
        .width(Length::Fill)
        .into()
}

/// ダイアログで CUBE を選んで読む。キャンセルは `Ok(None)`。
pub async fn open_cube_dialog() -> Result<Option<CubeOpenResult>, String> {
    let Some(handle) = rfd::AsyncFileDialog::new()
        .add_filter("Gaussian CUBE", &["cube", "cub"])
        .pick_file()
        .await
    else {
        return Ok(None);
    };
    let name = handle.file_name();
    #[cfg(not(target_arch = "wasm32"))]
    let path = Some(handle.path().to_string_lossy().into_owned());
    #[cfg(target_arch = "wasm32")]
    let path = None;
    let bytes = handle.read().await;
    parse_in_background(bytes)
        .await
        .map(|set| Some((name, set, path)))
}

/// ワークスペースに保存されたパスから CUBE を読む（ネイティブ版）
#[cfg(not(target_arch = "wasm32"))]
pub async fn open_cube_path(path: String) -> Result<Option<CubeOpenResult>, String> {
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| format!("The CUBE file '{path}' of the workspace could not be read: {e}"))?;
    let name = std::path::Path::new(&path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.clone());
    parse_in_background(bytes)
        .await
        .map(|set| Some((name, set, Some(path))))
}

/// CUBE を開いた結果（ファイル名、内容、ファイルのパス。ブラウザ版はパスなし）
pub type CubeOpenResult = (String, Arc<CubeSet>, Option<String>);

/// バイト列（CUBE のテキスト）を読む。重い読み込みは、ネイティブ版ではバックグラウンドで行う。
pub async fn parse_in_background(bytes: Vec<u8>) -> Result<Arc<CubeSet>, String> {
    let work = move || {
        let text = String::from_utf8(bytes)
            .map_err(|_| "The CUBE file is not valid UTF-8 text".to_string())?;
        CubeSet::parse(&text).map(Arc::new)
    };
    #[cfg(not(target_arch = "wasm32"))]
    {
        tokio::task::spawn_blocking(work)
            .await
            .map_err(|e| format!("The CUBE file could not be processed: {e}"))?
    }
    #[cfg(target_arch = "wasm32")]
    {
        work()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yui_core::state::CubeView;

    /// 色だけを指定した見た目（不透明度は既定）
    fn look(positive_color: usize, negative_color: usize) -> CubeView {
        CubeView {
            positive_color,
            negative_color,
            ..Default::default()
        }
    }

    /// 原点に球対称の ±exp(-r^2) を持つ CUBE（Bohr）。`orbitals` が 2 以上なら、軌道の CUBE
    fn sphere_cube(orbitals: usize, sign: f64) -> String {
        let (n, h) = (15usize, 0.4f64);
        let o = -(n as f64 - 1.0) * h / 2.0;
        let natoms = if orbitals > 1 { -1 } else { 1 };
        let mut s = format!("t\nt\n{natoms:5} {o:.6} {o:.6} {o:.6}\n");
        s.push_str(&format!(
            "{n:5} {h} 0.0 0.0\n{n:5} 0.0 {h} 0.0\n{n:5} 0.0 0.0 {h}\n"
        ));
        s.push_str("    1 1.0 0.0 0.0 0.0\n");
        if orbitals > 1 {
            s.push_str(&format!("{orbitals:5}"));
            for k in 0..orbitals {
                s.push_str(&format!("{:5}", k + 3));
            }
            s.push('\n');
        }
        for ix in 0..n {
            for iy in 0..n {
                for iz in 0..n {
                    let (x, y, z) = (o + ix as f64 * h, o + iy as f64 * h, o + iz as f64 * h);
                    let v = (-(x * x + y * y + z * z)).exp();
                    for k in 0..orbitals {
                        // 偶数番目の軌道は正、奇数番目は負
                        let sg = if k % 2 == 0 { sign } else { -sign };
                        s.push_str(&format!(" {:.8e}", sg * v));
                    }
                    s.push('\n');
                }
            }
        }
        s
    }

    fn job(set: &Arc<CubeSet>, dataset: usize, isolevel: Option<f32>) -> CubeJob {
        CubeJob {
            set: set.clone(),
            dataset,
            grid: None,
            isolevel,
        }
    }

    #[test]
    fn test_job_uses_the_default_level_or_the_given_one() {
        let set = Arc::new(CubeSet::parse(&sphere_cube(1, 1.0)).unwrap());
        let auto = run_job(job(&set, 0, None)).unwrap();
        assert_eq!(auto.isolevel, 0.05);
        assert!((auto.peak - 1.0).abs() < 1e-6);
        let given = run_job(job(&set, 0, Some(0.5))).unwrap();
        assert_eq!(given.isolevel, 0.5);
        // 高いレベルほど、球は小さい（頂点の位置の最大の距離）
        let extent = |iso: &CubeIsosurface| {
            iso.positive
                .positions
                .iter()
                .map(|p| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt())
                .fold(0.0f32, f32::max)
        };
        assert!(extent(&given) < extent(&auto));
        // 格子を渡すと、それを使う（変換をやり直さない）
        let again = run_job(CubeJob {
            grid: Some(auto.grid.clone()),
            ..job(&set, 0, Some(0.5))
        })
        .unwrap();
        assert!(Arc::ptr_eq(&again.grid, &auto.grid));
    }

    #[test]
    fn test_each_orbital_gets_its_own_lobe_color() {
        let set = Arc::new(CubeSet::parse(&sphere_cube(2, 1.0)).unwrap());
        let first = to_entries(&run_job(job(&set, 0, None)).unwrap(), &look(0, 1));
        let second = to_entries(&run_job(job(&set, 1, None)).unwrap(), &look(0, 1));
        assert_eq!(first[0].rep_id, "cube_positive");
        assert_eq!(second[0].rep_id, "cube_negative");
        assert!(run_job(job(&set, 2, None)).is_err());
    }

    #[test]
    fn test_needs_update_follows_the_wanted_dataset_and_level() {
        let set = Arc::new(CubeSet::parse(&sphere_cube(2, 1.0)).unwrap());
        let mut state = CubeState::new("a.cube".into(), set.clone());
        let mut view = CubeView::default();
        assert!(state.needs_update(&view), "nothing shown yet");
        state.shown = Some(run_job(state.job(&view)).unwrap());
        assert!(
            !state.needs_update(&view),
            "level 0 means the default, which is shown"
        );
        view.isolevel = 0.2;
        assert!(state.needs_update(&view));
        let grid = state.shown.as_ref().unwrap().grid.clone();
        let job = state.job(&view);
        assert_eq!(job.isolevel, Some(0.2));
        assert!(
            Arc::ptr_eq(job.grid.as_ref().unwrap(), &grid),
            "the same data set reuses the grid"
        );
        state.shown = Some(run_job(job).unwrap());
        assert!(!state.needs_update(&view));
        view.dataset = 1;
        assert!(state.needs_update(&view));
        assert!(
            state.job(&view).grid.is_none(),
            "another data set needs its own grid"
        );
    }

    #[test]
    fn test_slider_mapping_is_logarithmic_and_clamped() {
        let peak = 10.0;
        assert!((isolevel_to_slider(1.0, peak) - 0.0).abs() < 1e-6);
        assert!((slider_to_isolevel(isolevel_to_slider(0.05, peak)) - 0.05).abs() < 1e-6);
        // 最大値より上、最大値の 1/10000 より下は、端に丸める
        assert!((isolevel_to_slider(100.0, peak) - 1.0).abs() < 1e-6);
        assert!((isolevel_to_slider(1e-9, peak) - (1.0 - SLIDER_DECADES)).abs() < 1e-6);
    }

    #[test]
    fn test_density_cube_gives_one_blue_translucent_entry() {
        let set = Arc::new(CubeSet::parse(&sphere_cube(1, 1.0)).unwrap());
        let iso = run_job(job(&set, 0, None)).unwrap();
        let entries = to_entries(&iso, &look(0, 1));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].rep_id, "cube_positive");
        assert!(entries[0].opacity < 1.0, "goes to the translucent pass");
        assert_eq!(entries[0].vertices.len(), entries[0].mesh.positions.len());
        assert_eq!(entries[0].vertices[0].color[..3], COLOR_PRESETS[0].1[..3]);
        let neg = to_entries(
            &run_job(job(
                &Arc::new(CubeSet::parse(&sphere_cube(1, -1.0)).unwrap()),
                0,
                None,
            ))
            .unwrap(),
            &look(0, 1),
        );
        assert_eq!(neg[0].vertices[0].color[..3], COLOR_PRESETS[1].1[..3]);
        assert!(CubeSet::parse("not a cube file").is_err());
        // 色を替えると、頂点の色が替わる。範囲外の番号は最後の色に丸める
        let green = to_entries(&iso, &look(2, 1));
        assert_eq!(green[0].vertices[0].color[..3], COLOR_PRESETS[2].1[..3]);
        let clamped = to_entries(&iso, &look(99, 1));
        assert_eq!(
            clamped[0].vertices[0].color[..3],
            COLOR_PRESETS[COLOR_PRESETS.len() - 1].1[..3]
        );
        // 不透明度は見た目の設定から来る（範囲外は 0〜1 に丸める）
        let faint = to_entries(
            &iso,
            &CubeView {
                opacity: 0.25,
                ..Default::default()
            },
        );
        assert_eq!(faint[0].opacity, 0.25);
        assert_eq!(faint[0].vertices[0].color[3], 0.25);
        let over = to_entries(
            &iso,
            &CubeView {
                opacity: 7.0,
                ..Default::default()
            },
        );
        assert_eq!(over[0].opacity, 1.0);
    }
}
