//! アニメーションを、連番の PNG（と、あれば ffmpeg で MP4）に書き出す（File → Export Animation...）。
//!
//! 動きのもとは、視点の回転（Spin: 1 周、Rock: 首振りの 1 周期）か、再生できるフレーム（マルチモデルの
//! モデル、または XYZ の軌跡）。各フレームは、画面と同じ描画の準備（`Yui::offscreen_primitive`）を、
//! 画面外に描いて PNG にする（`viewport::offscreen`）。このモジュールには、何枚目に何を描くかの計画、
//! ダイアログ、1 枚の書き出し、ffmpeg の呼び出しがある。順に進める仕組みは `app/movie_ops.rs`。
//! デスクトップ版は、フォルダに連番の PNG を書き（`ffmpeg` があれば MP4 も）、ブラウザ版は、フォルダにも
//! `ffmpeg` にも頼れないので、PNG をメモリに溜めて、zip（`zip_store`）にまとめ、ダウンロードさせる。

#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use iced::widget::{button, column, container, row, text};
use iced::{Alignment, Element, Length};

use crate::camera_anim::{ROCK_AMPLITUDE_DEG, ROCK_PERIOD_S, rock_offset_deg};
use crate::canvas::{CanvasAction, CanvasOptions};
use crate::viewport::camera::orbit_camera;
use crate::viewport::offscreen::{OffscreenRenderer, downsample_box, encode_png};
use crate::viewport::primitive::ViewportPrimitive;
use yui_core::state::CameraState;

/// 動きのもと
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovieSource {
    /// 視点を、画面の縦軸のまわりに 1 周させる
    Spin,
    /// 視点を、左右に首振りさせる（1 周期）
    Rock,
    /// 再生できるフレーム（モデル、軌跡）を、先頭から末尾まで 1 回
    Playback,
}

/// 幅の候補（ピクセル。動画のために偶数）
pub const WIDTH_PRESETS: [u32; 3] = [960, 1280, 1920];
/// 1 秒あたりのフレーム数の候補
pub const FPS_PRESETS: [u32; 3] = [15, 24, 30];
/// 視点の動きの長さの候補（秒）
pub const SECONDS_PRESETS: [u32; 3] = [4, 8, 12];
/// なめらかさ（大きく描いて縮める倍率）の候補
pub const SMOOTHING_PRESETS: [u32; 2] = [1, 2];

/// 動画の書き出しの設定
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MovieOptions {
    pub source: MovieSource,
    pub fps: u32,
    /// 視点の動き（Spin / Rock）の長さ（秒）。Playback ではフレームの数で決まる。
    pub seconds: u32,
    pub width: u32,
    pub smoothing: u32,
    /// 連番の PNG のあと、ffmpeg があれば MP4 にする
    pub make_mp4: bool,
    /// キャンバスの形と構図。形が `Custom` のときは、`width` ではなく、指定の幅と高さを使う。
    pub canvas: CanvasOptions,
}

impl Default for MovieOptions {
    fn default() -> Self {
        Self {
            source: MovieSource::Spin,
            fps: 24,
            seconds: 8,
            width: 1280,
            smoothing: 2,
            make_mp4: true,
            canvas: CanvasOptions::default(),
        }
    }
}

/// ダイアログの操作
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovieDialogAction {
    SetSource(MovieSource),
    SetFps(u32),
    SetSeconds(u32),
    SetWidth(u32),
    SetSmoothing(u32),
    SetMakeMp4(bool),
    /// キャンバスの形と構図
    Canvas(CanvasAction),
    /// 保存先のフォルダを選んで、書き出す
    Start,
    Cancel,
}

impl MovieOptions {
    /// ダイアログの操作を設定に反映する（`Start` と `Cancel` は、呼び出し側が扱う）。候補にない値は無視する。
    pub fn apply(&mut self, action: MovieDialogAction) {
        match action {
            MovieDialogAction::SetSource(s) => self.source = s,
            MovieDialogAction::SetFps(v) if FPS_PRESETS.contains(&v) => self.fps = v,
            MovieDialogAction::SetSeconds(v) if SECONDS_PRESETS.contains(&v) => self.seconds = v,
            MovieDialogAction::SetWidth(v) if WIDTH_PRESETS.contains(&v) => self.width = v,
            MovieDialogAction::SetSmoothing(v) if SMOOTHING_PRESETS.contains(&v) => {
                self.smoothing = v
            }
            MovieDialogAction::SetMakeMp4(v) => self.make_mp4 = v,
            MovieDialogAction::Canvas(c) => self.canvas.apply(c),
            _ => {}
        }
    }

    /// 書き出すフレームの数。`playback_frames` は、再生できるフレームの数。
    pub fn frame_count(&self, playback_frames: usize) -> usize {
        match self.source {
            MovieSource::Playback => playback_frames,
            MovieSource::Spin | MovieSource::Rock => (self.fps * self.seconds) as usize,
        }
    }
}

/// `k` 枚目（0 始まり。全部で `n` 枚）の視点。Spin は 1 周（最後の次が最初に戻る。最後の枚は 360° に
/// ならない）、Rock は首振りの 1 周期（同じく、最後の次が最初に戻る）。Playback は視点を変えない。
pub fn camera_for_frame(
    base: &CameraState,
    source: MovieSource,
    k: usize,
    n: usize,
) -> CameraState {
    let mut cam = base.clone();
    let t = if n == 0 { 0.0 } else { k as f32 / n as f32 };
    match source {
        MovieSource::Spin => orbit_camera(&mut cam, std::f32::consts::TAU * t, 0.0),
        MovieSource::Rock => {
            let yaw = rock_offset_deg(t * ROCK_PERIOD_S, ROCK_AMPLITUDE_DEG, ROCK_PERIOD_S);
            orbit_camera(&mut cam, yaw.to_radians(), 0.0);
        }
        MovieSource::Playback => {}
    }
    cam
}

/// 動画の大きさ（エンコーダーが偶数を求めるので、幅と高さを偶数に切り下げる）。小さすぎれば `None`。
pub fn even_size(width: u32, height: u32) -> Option<(u32, u32)> {
    let (w, h) = (width & !1, height & !1);
    (w >= 2 && h >= 2).then_some((w, h))
}

/// `k` 枚目（0 始まり）のファイル名
pub fn frame_file_name(k: usize) -> String {
    format!("frame_{:04}.png", k + 1)
}

/// ffmpeg の引数（連番の PNG から、H.264 の MP4 を作る）
#[cfg(not(target_arch = "wasm32"))]
pub fn ffmpeg_args(dir: &Path, fps: u32, output: &Path) -> Vec<String> {
    vec![
        "-y".to_string(),
        "-framerate".to_string(),
        fps.to_string(),
        "-i".to_string(),
        dir.join("frame_%04d.png").to_string_lossy().into_owned(),
        "-c:v".to_string(),
        "libx264".to_string(),
        "-pix_fmt".to_string(),
        "yuv420p".to_string(),
        output.to_string_lossy().into_owned(),
    ]
}

/// ffmpeg で MP4 にする。ffmpeg がなければ `Ok(None)`、できたら `Ok(Some(出力のパス))`。
#[cfg(not(target_arch = "wasm32"))]
pub fn run_ffmpeg(dir: &Path, fps: u32) -> Result<Option<PathBuf>, String> {
    let output = dir.join("movie.mp4");
    match std::process::Command::new("ffmpeg")
        .args(ffmpeg_args(dir, fps, &output))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
    {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("ffmpeg could not be started: {e}")),
        Ok(out) if out.status.success() => Ok(Some(output)),
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr);
            let tail: String = err.lines().rev().take(3).collect::<Vec<_>>().join(" / ");
            Err(format!("ffmpeg failed: {tail}"))
        }
    }
}

/// 何枚も描くための、共有の描画器（フレームごとの非同期の処理の間で使い回す）
#[derive(Clone)]
pub struct SharedRenderer(pub Arc<Mutex<OffscreenRenderer>>);

impl std::fmt::Debug for SharedRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SharedRenderer")
    }
}

/// GPU を用意する
pub async fn create_renderer() -> Result<SharedRenderer, String> {
    OffscreenRenderer::new()
        .await
        .map(|r| SharedRenderer(Arc::new(Mutex::new(r))))
}

/// そのフレームの分子表面を、描く前に作るための材料（再生のフレームの書き出し）
pub struct SurfaceWork {
    /// 分子表面の表現ごとの生成の依頼（そのフレームの原子で作ったもの）
    pub requests: Vec<crate::surface_render::SurfaceBuildRequest>,
    /// 頂点の色を決める情報（書き出しの間、変わらない）
    pub ctx: Arc<crate::surface_render::SurfaceColorContext>,
    /// 一緒に描く、CUBE の等値面
    pub cube_entries: Vec<crate::surface_render::SurfaceMeshEntry>,
}

/// 1 枚を描いて、縮めて、PNG のバイト列にする。
/// `surface_work` があれば、描く前に、そのフレームの分子表面を作って、`primitive` の分子表面にする。
/// デスクトップ版は、読み戻しの待ちで止まるので、専用のスレッドで行う。ブラウザ版は、ブラウザが進めるので、
/// そのまま待つ。
pub async fn render_frame_png(
    renderer: SharedRenderer,
    primitive: ViewportPrimitive,
    width: u32,
    height: u32,
    smoothing: u32,
    surface_work: Option<SurfaceWork>,
) -> Result<Vec<u8>, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        tokio::task::spawn_blocking(move || {
            iced::futures::executor::block_on(render_frame_png_inner(
                renderer,
                primitive,
                width,
                height,
                smoothing,
                surface_work,
            ))
        })
        .await
        .map_err(|e| format!("The frame could not be rendered: {e}"))?
    }
    #[cfg(target_arch = "wasm32")]
    {
        render_frame_png_inner(renderer, primitive, width, height, smoothing, surface_work).await
    }
}

// 描画器の錠は、描いている間ずっと持つ（1 枚ずつ順に描くので、取り合いにならない）
#[allow(clippy::await_holding_lock)]
async fn render_frame_png_inner(
    renderer: SharedRenderer,
    mut primitive: ViewportPrimitive,
    width: u32,
    height: u32,
    smoothing: u32,
    surface_work: Option<SurfaceWork>,
) -> Result<Vec<u8>, String> {
    if let Some(work) = surface_work {
        let results = crate::surface_render::process_surface_build_requests(&work.requests);
        let entries = work.ctx.entries_from_results(results);
        primitive.surface_batches = crate::surface_render::SurfaceBatches::build_from_groups(&[
            &entries,
            &work.cube_entries,
        ]);
    }
    let mut r = renderer
        .0
        .lock()
        .map_err(|_| "The renderer is no longer usable".to_string())?;
    let max = r.gpu.device.limits().max_texture_dimension_2d;
    let factor = crate::viewport::offscreen::fit_supersample(width, height, smoothing, max);
    let rgba = r
        .render(&primitive, width * factor, height * factor)
        .await?;
    let (small, w, h) = downsample_box(&rgba, width * factor, height * factor, factor, false);
    encode_png(&small, w, h)
}

/// 1 枚を描いて、PNG にして `path` に書く（デスクトップ版）。
#[cfg(not(target_arch = "wasm32"))]
pub async fn render_frame_to_file(
    renderer: SharedRenderer,
    primitive: ViewportPrimitive,
    width: u32,
    height: u32,
    smoothing: u32,
    path: PathBuf,
    surface_work: Option<SurfaceWork>,
) -> Result<(), String> {
    let png = render_frame_png(renderer, primitive, width, height, smoothing, surface_work).await?;
    std::fs::write(&path, png).map_err(|e| format!("Failed to write '{}': {e}", path.display()))
}

/// 保存先のフォルダをダイアログで選ぶ。キャンセルは `Ok(None)`。
#[cfg(not(target_arch = "wasm32"))]
pub async fn choose_folder() -> Result<Option<PathBuf>, String> {
    Ok(rfd::AsyncFileDialog::new()
        .set_title("Choose a folder for the frames")
        .pick_folder()
        .await
        .map(|h| h.path().to_path_buf()))
}

/// 溜めた PNG の zip を、ブラウザにダウンロードさせる。戻りは、ファイル名（取り消しなら `None`）。
#[cfg(target_arch = "wasm32")]
pub async fn save_zip(zip: Vec<u8>, file_name: String) -> Result<Option<String>, String> {
    let Some(handle) = rfd::AsyncFileDialog::new()
        .add_filter("ZIP archive (*.zip)", &["zip"])
        .set_file_name(file_name)
        .save_file()
        .await
    else {
        return Ok(None);
    };
    let name = handle.file_name();
    handle
        .write(&zip)
        .await
        .map_err(|e| format!("Failed to save the zip: {e}"))?;
    Ok(Some(name))
}

/// ブラウザ版が、メモリに溜めてよい PNG の合計（バイト）。zip にまとめるとき、もう一度同じ大きさが要る。
pub const WEB_MAX_FRAME_BYTES: usize = 400 * 1024 * 1024;

/// ブラウザ版でダウンロードさせる zip の名前
#[cfg(any(target_arch = "wasm32", test))]
pub fn zip_file_name(version: &str) -> String {
    format!("yui-animation-{version}.zip")
}

/// 書き出しのダイアログ。`playback_frames` が 2 未満なら、Playback は選べない。
pub fn view_movie_dialog<'a, Message: Clone + 'a>(
    options: MovieOptions,
    playback_frames: usize,
    on_action: impl Fn(MovieDialogAction) -> Message + Copy + 'a,
) -> Element<'a, Message> {
    let chip = |label: String, selected: bool, action: Option<MovieDialogAction>| {
        let b = button(text(label).size(12))
            .padding([3, 10])
            .style(crate::ui_style::choice_button(selected));
        match action {
            Some(a) => b.on_press(on_action(a)),
            None => b,
        }
    };
    let label = |s: &'static str| text(s).size(12).width(Length::Fixed(80.0));

    let can_play = playback_frames >= 2;
    let source = row![
        label("Motion"),
        chip(
            "Spin".to_string(),
            options.source == MovieSource::Spin,
            Some(MovieDialogAction::SetSource(MovieSource::Spin))
        ),
        chip(
            "Rock".to_string(),
            options.source == MovieSource::Rock,
            Some(MovieDialogAction::SetSource(MovieSource::Rock))
        ),
        chip(
            format!("Frames ({playback_frames})"),
            options.source == MovieSource::Playback,
            can_play.then_some(MovieDialogAction::SetSource(MovieSource::Playback))
        ),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    let mut fps = row![label("Frame rate")]
        .spacing(6)
        .align_y(Alignment::Center);
    for v in FPS_PRESETS {
        fps = fps.push(chip(
            format!("{v} fps"),
            options.fps == v,
            Some(MovieDialogAction::SetFps(v)),
        ));
    }
    let mut seconds = row![label("Length")].spacing(6).align_y(Alignment::Center);
    for v in SECONDS_PRESETS {
        seconds = seconds.push(chip(
            format!("{v} s"),
            options.seconds == v,
            (options.source != MovieSource::Playback).then_some(MovieDialogAction::SetSeconds(v)),
        ));
    }
    let canvas_rows =
        crate::canvas::view_canvas_rows(options.canvas, crate::canvas::chip, move |a| {
            on_action(MovieDialogAction::Canvas(a))
        });
    let mut width = row![label("Width")].spacing(6).align_y(Alignment::Center);
    for v in WIDTH_PRESETS {
        width = width.push(chip(
            format!("{v} px"),
            options.width == v,
            Some(MovieDialogAction::SetWidth(v)),
        ));
    }
    let mut smoothing = row![label("Smoothing")]
        .spacing(6)
        .align_y(Alignment::Center);
    for v in SMOOTHING_PRESETS {
        smoothing = smoothing.push(chip(
            if v == 1 {
                "Off".to_string()
            } else {
                format!("{v}x")
            },
            options.smoothing == v,
            Some(MovieDialogAction::SetSmoothing(v)),
        ));
    }
    let mp4 = row![
        label("MP4"),
        chip(
            "Make with ffmpeg".to_string(),
            options.make_mp4,
            Some(MovieDialogAction::SetMakeMp4(true))
        ),
        chip(
            "Frames only".to_string(),
            !options.make_mp4,
            Some(MovieDialogAction::SetMakeMp4(false))
        ),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    let frames = options.frame_count(playback_frames);
    let mut body = column![text("Export Animation").size(14), source, fps, seconds].spacing(10);
    for r in canvas_rows {
        body = body.push(r);
    }
    // 形が Custom のときは、指定の幅と高さを使うので、幅の候補は出さない
    if options.canvas.shape != crate::canvas::CanvasShape::Custom {
        body = body.push(width);
    }
    // ブラウザ版は、フォルダにも ffmpeg にも頼れないので、PNG を zip にまとめてダウンロードさせる
    let web = cfg!(target_arch = "wasm32");
    let note = if web {
        format!(
            "{frames} PNG frames are kept in memory and downloaded as one zip when they are all drawn (the limit is {} MB; use a smaller width or fewer frames if it is exceeded). To make a video, run ffmpeg on the unzipped frames. Do not use the window while it runs.",
            WEB_MAX_FRAME_BYTES / (1024 * 1024)
        )
    } else {
        format!(
            "{frames} frames of PNG go to a folder you choose; the MP4 needs ffmpeg on the PATH. Do not use the window while it runs."
        )
    };
    let mut body = body.push(smoothing);
    if !web {
        body = body.push(mp4);
    }
    let card = container(
        body.push(text(note).size(11)).push(
            row![
                button(text(if web { "Start" } else { "Choose folder..." }).size(12))
                    .padding([4, 12])
                    .style(crate::ui_style::accent_button)
                    .on_press(on_action(MovieDialogAction::Start)),
                button(text("Cancel").size(12))
                    .padding([4, 12])
                    .style(crate::ui_style::secondary_button)
                    .on_press(on_action(MovieDialogAction::Cancel)),
            ]
            .spacing(8),
        ),
    )
    .padding(16)
    .style(container::rounded_box);

    container(card).center(Length::Fill).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam() -> CameraState {
        CameraState {
            eye: [10.0, 0.0, 0.0],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            fov: 45.0,
        }
    }

    fn azimuth_deg(c: &CameraState) -> f32 {
        let d = [
            c.eye[0] - c.target[0],
            c.eye[1] - c.target[1],
            c.eye[2] - c.target[2],
        ];
        d[2].atan2(d[0]).to_degrees()
    }

    #[test]
    fn test_frame_counts() {
        let o = MovieOptions::default();
        assert_eq!(o.frame_count(20), 24 * 8, "Spin: fps x seconds");
        let p = MovieOptions {
            source: MovieSource::Playback,
            ..o
        };
        assert_eq!(p.frame_count(20), 20, "Playback: one per frame");
    }

    #[test]
    fn test_spin_goes_around_once_and_the_next_would_be_the_first() {
        let n = 8;
        let a0 = azimuth_deg(&camera_for_frame(&cam(), MovieSource::Spin, 0, n));
        assert!(a0.abs() < 1e-3, "frame 0 is the base view");
        // 1 枚ごとに 360/8 = 45° ずつ回り、距離は変わらない
        let a1 = azimuth_deg(&camera_for_frame(&cam(), MovieSource::Spin, 1, n));
        let a2 = azimuth_deg(&camera_for_frame(&cam(), MovieSource::Spin, 2, n));
        assert!(((a2 - a1) - (a1 - a0)).abs() < 1e-2, "{a0} {a1} {a2}");
        assert!((a1 - a0).abs() - 45.0 < 1e-2 && (a1 - a0).abs() > 44.9);
        let last = camera_for_frame(&cam(), MovieSource::Spin, n - 1, n);
        assert!(
            (azimuth_deg(&last).abs() - 45.0).abs() < 1e-2,
            "{}",
            azimuth_deg(&last)
        );
        let d = |c: &CameraState| c.eye[0].hypot(c.eye[2]);
        assert!((d(&last) - 10.0).abs() < 1e-3);
    }

    #[test]
    fn test_rock_stays_within_the_amplitude_and_playback_keeps_the_view() {
        let n = 48;
        let max = (0..n)
            .map(|k| azimuth_deg(&camera_for_frame(&cam(), MovieSource::Rock, k, n)).abs())
            .fold(0.0f32, f32::max);
        assert!(
            max <= ROCK_AMPLITUDE_DEG + 0.1 && max > ROCK_AMPLITUDE_DEG - 3.0,
            "{max}"
        );
        assert_eq!(camera_for_frame(&cam(), MovieSource::Playback, 5, n), cam());
        // 枚数が 0 でも、落ちない
        let _ = camera_for_frame(&cam(), MovieSource::Spin, 0, 0);
    }

    #[test]
    fn test_movie_size_is_rounded_down_to_even() {
        assert_eq!(even_size(1280, 960), Some((1280, 960)));
        assert_eq!(even_size(1001, 563), Some((1000, 562)));
        assert_eq!(even_size(1, 100), None);
        assert_eq!(even_size(100, 1), None);
    }

    #[test]
    fn test_file_names_and_ffmpeg_arguments() {
        assert_eq!(frame_file_name(0), "frame_0001.png");
        assert_eq!(frame_file_name(11), "frame_0012.png");
        let args = ffmpeg_args(Path::new("/out"), 24, Path::new("/out/movie.mp4"));
        assert_eq!(&args[..4], ["-y", "-framerate", "24", "-i"]);
        assert!(args[4].ends_with("frame_%04d.png"));
        assert!(args.contains(&"yuv420p".to_string()));
        assert_eq!(args.last().unwrap(), "/out/movie.mp4");
    }

    #[test]
    fn test_dialog_options_change_only_known_values() {
        let mut o = MovieOptions::default();
        o.apply(MovieDialogAction::SetSource(MovieSource::Rock));
        o.apply(MovieDialogAction::SetFps(30));
        o.apply(MovieDialogAction::SetWidth(1920));
        o.apply(MovieDialogAction::SetMakeMp4(false));
        assert_eq!(
            (o.source, o.fps, o.width, o.make_mp4),
            (MovieSource::Rock, 30, 1920, false)
        );
        let before = o;
        o.apply(MovieDialogAction::SetFps(7));
        o.apply(MovieDialogAction::SetWidth(1111));
        o.apply(MovieDialogAction::SetSmoothing(5));
        o.apply(MovieDialogAction::Start);
        o.apply(MovieDialogAction::Cancel);
        assert_eq!(o, before);
        let _: Element<'_, MovieDialogAction> = view_movie_dialog(o, 20, |a| a);
        let _: Element<'_, MovieDialogAction> = view_movie_dialog(o, 1, |a| a);
    }
}
