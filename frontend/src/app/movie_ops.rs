//! アニメーションの書き出し（File → Export Animation...）を、1 枚ずつ順に進める。
//!
//! フレームごとに、UI のスレッドで「描くものの準備」（視点を決める、再生のフレームを表示する、
//! `offscreen_primitive_with`）をして、描画と PNG の書き出しは非同期で行う。終わったら、次のフレームへ進む。
//! 途中で取り消せる。進行中は、ウィンドウを操作しないこと（再生のフレームの書き出しは、アプリの表示を
//! 実際にフレームへ動かして描くため）。分子表面は、視点の回転では今のものを入れ、再生のフレームの書き出しでは、
//! フレームごとに、そのフレームの原子で、描く直前に作る（`SurfaceWork`。作る処理は描画と同じスレッドで行う）。

use std::path::PathBuf;

use crate::zip_store::ZipEntry;

use iced::Task;

use super::{Message, Yui};
use crate::movie_export::{
    MovieDialogAction, MovieOptions, MovieSource, SharedRenderer, SurfaceWork, camera_for_frame,
    even_size, frame_file_name,
};
use crate::status_bar::StatusMessage;
use yui_core::state::CameraState;

/// 進行中の書き出し
pub(crate) struct MovieJob {
    pub options: MovieOptions,
    /// 書き出すフレームの数
    pub total: usize,
    /// 次に描くフレーム（0 始まり）
    pub next: usize,
    /// 画像の大きさ（偶数）
    pub size: (u32, u32),
    /// 書き出しを始めたときの視点
    pub base_camera: CameraState,
    /// 書き出しを始めたときに表示していたフレーム（再生のフレームの書き出しが終わったら戻す）
    pub restore_frame: Option<usize>,
    pub dir: Option<PathBuf>,
    /// フォルダに書かず、PNG をメモリに溜めて、終わったら zip にする（ブラウザ版）
    pub to_memory: bool,
    /// メモリに溜めた PNG（`to_memory` のとき）と、その合計の大きさ（バイト）
    pub frames: Vec<ZipEntry>,
    pub bytes_kept: usize,
    pub renderer: Option<SharedRenderer>,
    /// 再生のフレームの書き出しで、分子表面の表現があるときの、頂点の色の情報
    pub surface_ctx: Option<std::sync::Arc<crate::surface_render::SurfaceColorContext>>,
}

/// 1 枚ぶんの依頼（描くものの準備ができたもの）
pub(crate) struct FrameRequest {
    pub primitive: crate::viewport::primitive::ViewportPrimitive,
    /// 書き込み先のファイル。メモリに溜める書き出しでは `None`（ブラウザ版は、いつも `None`）
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub path: Option<PathBuf>,
    pub size: (u32, u32),
    pub smoothing: u32,
    /// そのフレームの分子表面（再生のフレームの書き出しで、分子表面の表現があるとき）
    pub surface_work: Option<SurfaceWork>,
}

impl Yui {
    /// File → Export Animation... : 設定のダイアログを開く
    pub(super) fn open_movie_dialog(&mut self) {
        if !super::has_structure(&self.state) {
            self.status_message = Some(StatusMessage::warning(
                "No structure loaded to export.".to_string(),
            ));
            return;
        }
        if self.movie.is_some() {
            self.status_message = Some(StatusMessage::warning(
                "An animation is already being exported.".to_string(),
            ));
            return;
        }
        // フレームが 2 つ未満なら、再生のフレームは選べない
        if self.movie_options.source == MovieSource::Playback && self.frame_count() < 2 {
            self.movie_options.source = MovieSource::Spin;
        }
        self.movie_dialog = true;
    }

    /// ダイアログの操作。「Choose folder...」で、保存先のフォルダの選択へ進む。
    pub(super) fn handle_movie_dialog(&mut self, action: MovieDialogAction) -> Task<Message> {
        match action {
            MovieDialogAction::Cancel => {
                self.movie_dialog = false;
                Task::none()
            }
            MovieDialogAction::Start => {
                self.movie_dialog = false;
                let options = self.movie_options;
                let (size, camera) = match self
                    .export_canvas(&options.canvas, options.width)
                    .and_then(|((w, h), cam)| {
                        even_size(w, h)
                            .map(|s| (s, cam))
                            .ok_or_else(|| "The image size is too small for a movie.".to_string())
                    }) {
                    Ok(v) => v,
                    Err(e) => {
                        self.status_message =
                            Some(StatusMessage::error(format!("Export failed: {e}")));
                        return Task::none();
                    }
                };
                let total = options.frame_count(self.frame_count());
                if total == 0 {
                    self.status_message = Some(StatusMessage::error(
                        "Export failed: there are no frames to export.".to_string(),
                    ));
                    return Task::none();
                }
                self.movie = Some(MovieJob {
                    options,
                    total,
                    next: 0,
                    size,
                    base_camera: camera,
                    restore_frame: None,
                    dir: None,
                    to_memory: cfg!(target_arch = "wasm32"),
                    frames: Vec::new(),
                    bytes_kept: 0,
                    renderer: None,
                    surface_ctx: None,
                });
                // デスクトップ版は、保存先のフォルダを選ぶ。ブラウザ版は、選ぶものがないので、そのまま始める
                #[cfg(not(target_arch = "wasm32"))]
                {
                    Task::perform(
                        crate::movie_export::choose_folder(),
                        Message::MovieFolderChosen,
                    )
                }
                #[cfg(target_arch = "wasm32")]
                {
                    self.movie_after_destination()
                }
            }
            other => {
                self.movie_options.apply(other);
                Task::none()
            }
        }
    }

    /// 保存先のフォルダが決まった（キャンセルなら、やめる）。次は、GPU の用意。
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn handle_movie_folder_chosen(
        &mut self,
        result: Result<Option<PathBuf>, String>,
    ) -> Task<Message> {
        let Some(job) = self.movie.as_mut() else {
            return Task::none();
        };
        match result {
            Ok(Some(dir)) => {
                job.dir = Some(dir);
            }
            Ok(None) => {
                self.movie = None;
                return Task::none();
            }
            Err(e) => return self.movie_failed(e),
        }
        self.movie_after_destination()
    }

    /// 書き出し先が決まった（フォルダ、またはメモリ）。次は、GPU の用意。
    fn movie_after_destination(&mut self) -> Task<Message> {
        let Some(job) = self.movie.as_mut() else {
            return Task::none();
        };
        // 再生のフレームを書き出すときは、再生を止め、終わったら今のフレームに戻す
        if job.options.source == MovieSource::Playback {
            let restore = self.current_frame();
            self.reset_playback();
            // 分子表面の表現があれば、フレームごとに作り直す（色の情報は、フレームが変わっても同じ）
            let ctx = (!self.surface_build_requests().is_empty()).then(|| {
                std::sync::Arc::new(crate::surface_render::SurfaceColorContext::new(
                    &self.state.molecule.atoms,
                    self.state.atom_group.as_ref(),
                    &self.state.secondary_structure_map,
                ))
            });
            if let Some(job) = self.movie.as_mut() {
                job.restore_frame = restore;
                job.surface_ctx = ctx;
            }
        }
        self.status_message = Some(StatusMessage::info(
            "Preparing the renderer for the animation...".to_string(),
        ));
        Task::perform(
            crate::movie_export::create_renderer(),
            Message::MovieRendererReady,
        )
    }

    pub(super) fn handle_movie_renderer_ready(
        &mut self,
        result: Result<SharedRenderer, String>,
    ) -> Task<Message> {
        if self.movie.is_none() {
            return Task::none(); // 準備の間に取り消された
        }
        match result {
            Ok(renderer) => {
                if let Some(job) = self.movie.as_mut() {
                    job.renderer = Some(renderer);
                }
                self.movie_step()
            }
            Err(e) => self.movie_failed(e),
        }
    }

    pub(super) fn handle_movie_frame_done(
        &mut self,
        result: Result<Option<Vec<u8>>, String>,
    ) -> Task<Message> {
        let Some(job) = self.movie.as_mut() else {
            return Task::none(); // 取り消したあとに、描いていた 1 枚が終わった
        };
        match result {
            Ok(png) => {
                // メモリに溜める書き出しでは、PNG を zip の 1 ファイルとして持つ。溜めすぎるなら、止める
                if let Some(data) = png {
                    job.bytes_kept += data.len();
                    if job.bytes_kept > crate::movie_export::WEB_MAX_FRAME_BYTES {
                        let (n, mb) = (job.next + 1, job.bytes_kept / (1024 * 1024));
                        return self.movie_failed(format!(
                            "{n} frames already take {mb} MB, more than the browser can hold ({} MB). Use a smaller width or fewer frames, or the desktop app.",
                            crate::movie_export::WEB_MAX_FRAME_BYTES / (1024 * 1024)
                        ));
                    }
                    job.frames.push(ZipEntry {
                        name: frame_file_name(job.next),
                        data,
                    });
                }
                job.next += 1;
                self.movie_step()
            }
            Err(e) => self.movie_failed(e),
        }
    }

    /// 次の 1 枚を描かせる。全部描いたら、MP4 にして（頼まれていれば）終わる。
    fn movie_step(&mut self) -> Task<Message> {
        let Some(job) = self.movie.as_ref() else {
            return Task::none();
        };
        if job.next >= job.total {
            return self.movie_encode();
        }
        let (k, total) = (job.next, job.total);
        let Some(request) = self.movie_next_request() else {
            return self.movie_failed("The animation job is incomplete.".to_string());
        };
        let renderer = self
            .movie
            .as_ref()
            .and_then(|j| j.renderer.clone())
            .expect("the renderer is ready before the first frame");
        self.status_message = Some(StatusMessage::info(format!(
            "Exporting the animation: frame {}/{total} (Cancel in the bar above)",
            k + 1
        )));
        Task::perform(render_one(renderer, request), Message::MovieFrameDone)
    }

    /// 次のフレームの準備（視点を決める、再生のフレームを表示する、描くものを作る）
    pub(super) fn movie_next_request(&mut self) -> Option<FrameRequest> {
        let job = self.movie.as_ref()?;
        let (k, total, size, options) = (job.next, job.total, job.size, job.options);
        // フォルダへ書くときは、先にフォルダが決まっている。メモリに溜めるときは、いらない
        let path = if job.to_memory {
            None
        } else {
            Some(job.dir.clone()?.join(frame_file_name(k)))
        };
        let base = job.base_camera.clone();
        let camera = camera_for_frame(&base, options.source, k, total);
        let mut surface_work = None;
        let surfaces = if options.source == MovieSource::Playback {
            self.show_frame(k);
            // 分子表面は、そのフレームの原子で、描く直前に作る（`render_frame_to_file`）。ここでは CUBE の面だけ。
            if let Some(ctx) = self.movie.as_ref().and_then(|j| j.surface_ctx.clone()) {
                let requests = self.surface_build_requests();
                if !requests.is_empty() {
                    surface_work = Some(SurfaceWork {
                        requests,
                        ctx,
                        cube_entries: self.cube_entries.clone(),
                    });
                }
            }
            crate::surface_render::SurfaceBatches::build_from_groups(&[&self.cube_entries])
        } else {
            self.surface_batches.clone()
        };
        let primitive = self.offscreen_primitive_with(size.0, size.1, false, &camera, &surfaces);
        Some(FrameRequest {
            primitive,
            path,
            size,
            smoothing: options.smoothing,
            surface_work,
        })
    }

    /// 全部描いた。ffmpeg で MP4 にするよう頼まれていれば、それを行う。
    fn movie_encode(&mut self) -> Task<Message> {
        let Some(job) = self.movie.as_ref() else {
            return Task::none();
        };
        // メモリに溜めたものは、zip にして、ブラウザにダウンロードさせる
        if job.to_memory {
            #[cfg(target_arch = "wasm32")]
            {
                return self.movie_download_zip();
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                return self.movie_failed(
                    "Saving to memory is only used by the browser build.".to_string(),
                );
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.movie_failed("The animation job is incomplete.".to_string())
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.movie_encode_to_folder()
    }

    /// 溜めた PNG を、zip の 1 つのファイルにして返す（名前つき）。溜めたものは、取り出して空にする。
    #[cfg(any(target_arch = "wasm32", test))]
    pub(super) fn movie_zip(&mut self) -> Result<(Vec<u8>, String), String> {
        let job = self
            .movie
            .as_mut()
            .ok_or_else(|| "The animation job is incomplete.".to_string())?;
        let entries = std::mem::take(&mut job.frames);
        job.bytes_kept = 0;
        let zip = crate::zip_store::build_zip(&entries)?;
        Ok((
            zip,
            crate::movie_export::zip_file_name(env!("CARGO_PKG_VERSION")),
        ))
    }

    /// （ブラウザ版）全部描いた。zip にして、ダウンロードさせる。
    #[cfg(target_arch = "wasm32")]
    fn movie_download_zip(&mut self) -> Task<Message> {
        let (zip, name) = match self.movie_zip() {
            Ok(v) => v,
            Err(e) => return self.movie_failed(e),
        };
        self.status_message = Some(StatusMessage::info(format!(
            "Saving '{name}' ({} MB)...",
            zip.len() / (1024 * 1024)
        )));
        Task::perform(
            crate::movie_export::save_zip(zip, name),
            Message::MovieZipSaved,
        )
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn handle_movie_zip_saved(
        &mut self,
        result: Result<Option<String>, String>,
    ) -> Task<Message> {
        let Some(job) = self.movie.as_ref() else {
            return Task::none();
        };
        let (total, fps) = (job.total, job.options.fps);
        let message = match result {
            Ok(Some(name)) => StatusMessage::info(format!(
                "Downloaded '{name}': {total} PNG frames ({fps} fps). To make a video, unzip it and run: ffmpeg -framerate {fps} -i frame_%04d.png -c:v libx264 -pix_fmt yuv420p movie.mp4"
            )),
            Ok(None) => StatusMessage::warning("The animation was not saved.".to_string()),
            Err(e) => StatusMessage::error(format!("Animation export failed: {e}")),
        };
        self.restore_after_movie();
        self.movie = None;
        self.status_message = Some(message);
        Task::none()
    }

    /// （デスクトップ版）全部描いた。ffmpeg で MP4 にするよう頼まれていれば、それを行う。
    #[cfg(not(target_arch = "wasm32"))]
    fn movie_encode_to_folder(&mut self) -> Task<Message> {
        let Some(job) = self.movie.as_ref() else {
            return Task::none();
        };
        let (dir, fps, make_mp4) = (job.dir.clone(), job.options.fps, job.options.make_mp4);
        let Some(dir) = dir else {
            return self.movie_failed("The animation job is incomplete.".to_string());
        };
        if !make_mp4 {
            return self.movie_finished(None);
        }
        self.status_message = Some(StatusMessage::info(
            "Making the MP4 with ffmpeg...".to_string(),
        ));
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || crate::movie_export::run_ffmpeg(&dir, fps))
                    .await
                    .map_err(|e| format!("ffmpeg could not be run: {e}"))?
            },
            Message::MovieEncoded,
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn handle_movie_encoded(
        &mut self,
        result: Result<Option<PathBuf>, String>,
    ) -> Task<Message> {
        if self.movie.is_none() {
            return Task::none();
        }
        match result {
            Ok(mp4) => self.movie_finished(Some(mp4)),
            // PNG はできている。MP4 だけ失敗したことを、エラーとして知らせる
            Err(e) => {
                let dir = self.movie.as_ref().and_then(|j| j.dir.clone());
                self.restore_after_movie();
                self.movie = None;
                self.status_message = Some(StatusMessage::error(format!(
                    "The frames were written{}, but the MP4 failed: {e}",
                    dir.map(|d| format!(" to '{}'", d.display()))
                        .unwrap_or_default()
                )));
                Task::none()
            }
        }
    }

    /// 終わった。`mp4` は、MP4 を頼んだときだけ `Some`（ffmpeg がなければ中身が `None`）。
    #[cfg(not(target_arch = "wasm32"))]
    fn movie_finished(&mut self, mp4: Option<Option<PathBuf>>) -> Task<Message> {
        let Some(job) = self.movie.as_ref() else {
            return Task::none();
        };
        let (total, fps) = (job.total, job.options.fps);
        let dir = job.dir.clone().unwrap_or_default();
        let message = match mp4 {
            Some(Some(path)) => format!(
                "Exported {total} frames and the MP4 '{}' ({fps} fps)",
                path.display()
            ),
            Some(None) => format!(
                "Exported {total} frames to '{}'. ffmpeg was not found, so there is no MP4; with ffmpeg: {}",
                dir.display(),
                ffmpeg_hint(&dir, fps)
            ),
            None => format!(
                "Exported {total} frames to '{}'. To make an MP4: {}",
                dir.display(),
                ffmpeg_hint(&dir, fps)
            ),
        };
        self.restore_after_movie();
        self.movie = None;
        self.status_message = Some(StatusMessage::info(message));
        Task::none()
    }

    /// 取り消し（上のバーのボタン）。書き出したフレームのファイルは、そのまま残す。
    pub(super) fn handle_movie_cancel(&mut self) -> Task<Message> {
        if let Some(job) = self.movie.as_ref() {
            let written = job.next;
            self.restore_after_movie();
            self.movie = None;
            self.status_message = Some(StatusMessage::warning(format!(
                "The animation export was cancelled ({written} frames were written)."
            )));
        }
        Task::none()
    }

    fn movie_failed(&mut self, error: String) -> Task<Message> {
        self.restore_after_movie();
        self.movie = None;
        self.status_message = Some(StatusMessage::error(format!(
            "Animation export failed: {error}"
        )));
        Task::none()
    }

    /// 再生のフレームの書き出しで動かした表示を、元のフレームに戻す
    fn restore_after_movie(&mut self) {
        let restore = self.movie.as_ref().and_then(|j| {
            (j.options.source == MovieSource::Playback)
                .then_some(j.restore_frame)
                .flatten()
        });
        if let Some(frame) = restore {
            self.show_frame(frame);
        }
    }
}

/// 1 枚ぶんを描いて返す。フォルダへ書く書き出しでは、ファイルに書いて `None`。
/// メモリに溜める書き出しでは、PNG のバイト列を `Some` で返す。
async fn render_one(
    renderer: SharedRenderer,
    request: FrameRequest,
) -> Result<Option<Vec<u8>>, String> {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(path) = request.path {
        return crate::movie_export::render_frame_to_file(
            renderer,
            request.primitive,
            request.size.0,
            request.size.1,
            request.smoothing,
            path,
            request.surface_work,
        )
        .await
        .map(|()| None);
    }
    crate::movie_export::render_frame_png(
        renderer,
        request.primitive,
        request.size.0,
        request.size.1,
        request.smoothing,
        request.surface_work,
    )
    .await
    .map(Some)
}

/// ffmpeg を自分で動かすための、1 行の例
#[cfg(not(target_arch = "wasm32"))]
fn ffmpeg_hint(dir: &std::path::Path, fps: u32) -> String {
    let args = crate::movie_export::ffmpeg_args(dir, fps, &dir.join("movie.mp4"));
    format!("ffmpeg {}", args.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_open_dispatch::FileOpenHandler;
    use crate::menu_bar::MenuAction;

    fn app_with(file: &str) -> Yui {
        let mut app = Yui::default();
        let path = format!("{}/../core/tests/data/{file}", env!("CARGO_MANIFEST_DIR"));
        let bytes = std::fs::read(path).expect("read test structure");
        let _ = app.update(Message::FileContentFetched {
            path: file.to_string(),
            handler: FileOpenHandler::StructureViewer,
            result: Ok(bytes),
        });
        app
    }

    fn job(app: &Yui, source: MovieSource, total: usize, dir: PathBuf) -> MovieJob {
        MovieJob {
            options: MovieOptions {
                source,
                smoothing: 1,
                ..MovieOptions::default()
            },
            total,
            next: 0,
            size: (128, 96),
            base_camera: app.state.camera.clone(),
            restore_frame: None,
            dir: Some(dir),
            to_memory: false,
            frames: Vec::new(),
            bytes_kept: 0,
            renderer: None,
            surface_ctx: None,
        }
    }

    #[test]
    fn test_dialog_needs_a_structure_and_goes_on_to_the_folder_choice() {
        use MovieDialogAction as A;
        let mut app = Yui::default();
        let _ = app.update(Message::MenuAction(MenuAction::FileExportAnimation));
        assert!(!app.movie_dialog, "no structure");

        let mut app = app_with("1hls.pdb");
        let _ = app.update(Message::MenuAction(MenuAction::FileExportAnimation));
        assert!(app.movie_dialog);
        // 1 つのモデルしかないので、再生のフレームは選べない。選んであっても Spin に戻す
        app.movie_options.source = MovieSource::Playback;
        app.movie_dialog = false;
        let _ = app.update(Message::MenuAction(MenuAction::FileExportAnimation));
        assert_eq!(app.movie_options.source, MovieSource::Spin);

        let _ = app.update(Message::MovieDialog(A::SetFps(30)));
        let _ = app.update(Message::MovieDialog(A::Cancel));
        assert!(!app.movie_dialog && app.movie.is_none());
        assert_eq!(app.movie_options.fps, 30, "the choice is remembered");

        let _ = app.update(Message::MenuAction(MenuAction::FileExportAnimation));
        let _ = app.update(Message::MovieDialog(A::Start));
        let job = app.movie.as_ref().expect("a job waits for the folder");
        assert_eq!(job.total, 30 * 8);
        assert_eq!(job.size.0 % 2 + job.size.1 % 2, 0);
        // フォルダの選択を取り消すと、やめる
        let _ = app.update(Message::MovieFolderChosen(Ok(None)));
        assert!(app.movie.is_none());
        // 進行中は、もう 1 つ始められない
        app.movie = Some(job_stub(&app));
        let _ = app.update(Message::MenuAction(MenuAction::FileExportAnimation));
        assert!(!app.movie_dialog);
        // 取り消しと、取り消したあとに届いた結果
        let _ = app.update(Message::MovieCancel);
        assert!(app.movie.is_none());
        let _ = app.update(Message::MovieFrameDone(Ok(None)));
        let _ = app.update(Message::MovieEncoded(Ok(None)));
        assert!(app.movie.is_none());
    }

    fn job_stub(app: &Yui) -> MovieJob {
        job(app, MovieSource::Spin, 4, PathBuf::from("."))
    }

    #[test]
    fn test_spin_frames_are_rendered_to_numbered_pngs_that_differ() {
        let mut app = app_with("1hls.pdb");
        app.request_camera_fit();
        // 視点を、構造が見える位置にする（読み込み直後の視点でよい）
        let dir = std::env::temp_dir().join(format!("yui_movie_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let renderer = match runtime.block_on(crate::movie_export::create_renderer()) {
            Ok(r) => r,
            Err(e) if e.contains("No graphics adapter") => {
                eprintln!("movie export: skipped ({e})");
                return;
            }
            Err(e) => panic!("{e}"),
        };
        let mut j = job(&app, MovieSource::Spin, 4, dir.clone());
        j.renderer = Some(renderer.clone());
        app.movie = Some(j);
        for _ in 0..4 {
            let req = app.movie_next_request().expect("a request");
            assert_eq!(req.size, (128, 96));
            runtime
                .block_on(crate::movie_export::render_frame_to_file(
                    renderer.clone(),
                    req.primitive,
                    req.size.0,
                    req.size.1,
                    req.smoothing,
                    req.path.expect("a file path"),
                    req.surface_work,
                ))
                .unwrap();
            app.movie.as_mut().unwrap().next += 1;
        }
        let read = |k: usize| std::fs::read(dir.join(frame_file_name(k))).expect("frame file");
        let frames: Vec<Vec<u8>> = (0..4).map(read).collect();
        for f in &frames {
            assert_eq!(&f[..8], b"\x89PNG\r\n\x1a\n");
        }
        // 90° ずつ回るので、どの 2 枚も同じではない
        for a in 0..4 {
            for b in a + 1..4 {
                assert_ne!(frames[a], frames[b], "frames {a} and {b} are the same");
            }
        }
        assert!(!dir.join(frame_file_name(4)).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ブラウザ版と同じ経路（フォルダに書かず、PNG をメモリに溜めて、zip にする）を、デスクトップの
    /// 実際の描画で通す。ダウンロードだけは、ブラウザでしか動かないので、ここでは試さない。
    #[test]
    fn test_frames_are_collected_in_memory_and_become_a_zip_like_the_browser_build() {
        let mut app = app_with("1hls.pdb");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let renderer = match runtime.block_on(crate::movie_export::create_renderer()) {
            Ok(r) => r,
            Err(e) if e.contains("No graphics adapter") => {
                eprintln!("movie export: skipped ({e})");
                return;
            }
            Err(e) => panic!("{e}"),
        };
        // 4 枚のうち、3 枚を描く（最後の 1 枚の次に zip にする経路は、ブラウザでだけ動く）
        let mut j = job(&app, MovieSource::Spin, 4, PathBuf::new());
        j.dir = None;
        j.to_memory = true;
        j.renderer = Some(renderer.clone());
        app.movie = Some(j);
        for k in 0..3 {
            let req = app.movie_next_request().expect("a request");
            assert!(req.path.is_none(), "no file in memory mode");
            let png = runtime
                .block_on(render_one(renderer.clone(), req))
                .unwrap()
                .expect("memory mode returns the PNG");
            assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
            let _ = app.handle_movie_frame_done(Ok(Some(png)));
            let job = app.movie.as_ref().expect("still running");
            assert_eq!((job.next, job.frames.len()), (k + 1, k + 1));
        }
        let kept = app.movie.as_ref().unwrap().bytes_kept;
        assert!(kept > 0);

        let (zip, name) = app.movie_zip().unwrap();
        assert_eq!(
            name,
            format!("yui-animation-{}.zip", env!("CARGO_PKG_VERSION"))
        );
        let files = crate::zip_store::read_zip(&zip);
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            ["frame_0001.png", "frame_0002.png", "frame_0003.png"]
        );
        assert_eq!(files.iter().map(|(_, d)| d.len()).sum::<usize>(), kept);
        for (_, data) in &files {
            assert_eq!(&data[..8], b"\x89PNG\r\n\x1a\n");
        }
        // 90° ずつ回るので、どの 2 枚も同じではない
        assert_ne!(files[0].1, files[1].1);
        assert_ne!(files[1].1, files[2].1);
        // 溜めたものは、取り出して空になる
        let job = app.movie.as_ref().unwrap();
        assert_eq!((job.frames.len(), job.bytes_kept), (0, 0));
        if let Ok(dump) = std::env::var("MOVIE_ZIP_DUMP") {
            std::fs::write(dump, &zip).unwrap();
        }
    }

    #[test]
    fn test_memory_sink_stops_before_the_browser_runs_out_of_memory() {
        let mut app = app_with("1hls.pdb");
        let mut j = job(&app, MovieSource::Spin, 4, PathBuf::new());
        j.dir = None;
        j.to_memory = true;
        j.bytes_kept = crate::movie_export::WEB_MAX_FRAME_BYTES - 10;
        app.movie = Some(j);
        let _ = app.handle_movie_frame_done(Ok(Some(vec![0u8; 100])));
        assert!(app.movie.is_none(), "the export stopped");
        let msg = app.status_message.as_ref().expect("a message").text.clone();
        assert!(
            msg.contains("more than the browser can hold") && msg.contains("desktop app"),
            "{msg}"
        );
    }

    #[test]
    fn test_playback_frames_show_each_model_and_the_view_is_restored() {
        let mut app = app_with("1HLS.cif");
        assert!(app.frame_count() >= 3);
        let before = app.state.active_model.clone();
        assert_eq!(before.as_deref(), Some("model_1"));
        let mut j = job(&app, MovieSource::Playback, 3, PathBuf::from("."));
        j.restore_frame = app.current_frame();
        app.movie = Some(j);
        let mut seen = Vec::new();
        for k in 0..3 {
            app.movie.as_mut().unwrap().next = k;
            let req = app.movie_next_request().expect("a request");
            assert!(req.path.expect("a file path").ends_with(frame_file_name(k)));
            seen.push(app.state.active_model.clone());
        }
        assert_eq!(
            seen,
            vec![
                Some("model_1".to_string()),
                Some("model_2".to_string()),
                Some("model_3".to_string())
            ]
        );
        // 取り消すと、書き出しの前のフレームに戻る
        let _ = app.update(Message::MovieCancel);
        assert_eq!(app.state.active_model, before);
        assert!(app.movie.is_none());
    }

    #[test]
    fn test_playback_frames_get_a_surface_built_for_each_frame() {
        let mut app = app_with("1HLS.cif");
        app.state
            .representations
            .push(yui_core::representation::Representation::new(
                "surf",
                yui_core::selector::Selector::All,
                yui_core::representation::RepresentationKind::Surface,
                yui_core::representation::ColorScheme::Chain,
            ));
        let dir = std::env::temp_dir().join(format!("yui_movie_surf_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut j = job(&app, MovieSource::Playback, 2, dir.clone());
        j.restore_frame = app.current_frame();
        j.surface_ctx = Some(std::sync::Arc::new(
            crate::surface_render::SurfaceColorContext::new(
                &app.state.molecule.atoms,
                app.state.atom_group.as_ref(),
                &app.state.secondary_structure_map,
            ),
        ));
        app.movie = Some(j);

        // 2 つのフレームは、別のモデルの原子から、別の分子表面を作る
        let mut meshes = Vec::new();
        let mut requests = Vec::new();
        for k in 0..2 {
            app.movie.as_mut().unwrap().next = k;
            let req = app.movie_next_request().expect("a request");
            let work = req.surface_work.expect("a surface is built for this frame");
            assert_eq!(work.requests.len(), 1);
            let results = crate::surface_render::process_surface_build_requests(&work.requests);
            assert!(!results[0].mesh.positions.is_empty());
            meshes.push(results[0].mesh.positions.clone());
            requests.push((
                req.primitive,
                work.requests,
                work.ctx,
                work.cube_entries,
                req.path.expect("a file path"),
            ));
        }
        assert_ne!(meshes[0], meshes[1], "each frame has its own surface");

        // 描いてみる（GPU のアダプターがなければ、読み飛ばす）。分子表面があるフレームは、ないものと違う
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let renderer = match runtime.block_on(crate::movie_export::create_renderer()) {
            Ok(r) => r,
            Err(e) if e.contains("No graphics adapter") => {
                eprintln!("movie export: skipped ({e})");
                let _ = std::fs::remove_dir_all(&dir);
                return;
            }
            Err(e) => panic!("{e}"),
        };
        let (primitive, reqs, ctx, cube, path) = requests.remove(0);
        let plain = dir.join("plain.png");
        let with_surface = path;
        let app2_primitive = app.offscreen_primitive_with(
            128,
            96,
            false,
            &app.state.camera,
            &crate::surface_render::SurfaceBatches::default(),
        );
        for (prim, out, work) in [
            (app2_primitive, plain.clone(), None),
            (
                primitive,
                with_surface.clone(),
                Some(SurfaceWork {
                    requests: reqs,
                    ctx,
                    cube_entries: cube,
                }),
            ),
        ] {
            runtime
                .block_on(crate::movie_export::render_frame_to_file(
                    renderer.clone(),
                    prim,
                    128,
                    96,
                    1,
                    out,
                    work,
                ))
                .unwrap();
        }
        assert_ne!(
            std::fs::read(&plain).unwrap(),
            std::fs::read(&with_surface).unwrap(),
            "the surface changes the picture"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
