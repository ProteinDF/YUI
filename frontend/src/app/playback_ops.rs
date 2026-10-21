//! 再生の操作（architecture.md 8.10節）。
//!
//! 再生の基盤（`crate::playback`）は、フレームが何であるかを知らない。ここで、フレームの番号を、表示するモデル
//! （`AppState.active_model`）に結び付ける。後で軌跡ファイルの再生を足すときは、
//! `frame_count` / `current_frame` / `show_frame` の 3 つが、フレームの出どころを切り替える場所になる。

use iced::Task;
use iced::time::Instant;

use super::{Message, Yui};
use crate::playback::{self, PlaybackAction};
use crate::status_bar::StatusMessage;

impl Yui {
    /// フレームの総数。軌跡を開いていれば軌跡のフレーム、なければ構造のモデル。
    pub(super) fn frame_count(&self) -> usize {
        match &self.trajectory {
            Some(t) => t.traj.frame_count(),
            None => self.model_atom_ranges.len(),
        }
    }

    /// 再生できるか（フレームが 2 つ以上あるときだけ、操作のバーを出す）
    pub(super) fn has_playback(&self) -> bool {
        self.frame_count() >= 2
    }

    /// 今のフレームの番号。全モデル表示のときは `None`
    pub(super) fn current_frame(&self) -> Option<usize> {
        if let Some(t) = &self.trajectory {
            return Some(t.current);
        }
        let active = self.state.active_model.as_deref()?;
        self.model_atom_ranges
            .iter()
            .position(|r| r.model_key == active)
    }

    /// 指定のフレームを表示する。表示するモデルが変わるので、選択は外す（原子の ID はモデルごとに違うため）。
    /// 変わったか（すでに表示中のフレームなら `false`）を返す。
    pub(super) fn show_frame(&mut self, frame: usize) -> bool {
        if self.trajectory.is_some() {
            return self.show_trajectory_frame(frame);
        }
        let Some(range) = self.model_atom_ranges.get(frame) else {
            return false;
        };
        if self.state.active_model.as_deref() == Some(range.model_key.as_str()) {
            return false;
        }
        self.state.active_model = Some(range.model_key.clone());
        self.selection.clear();
        true
    }

    /// 軌跡のフレームを表示する。表示するモデルは変わらず、原子の ID も同じなので、選択は外さない。
    /// 読めなかったときは、再生を止めて、理由を示す（`false`）。
    fn show_trajectory_frame(&mut self, frame: usize) -> bool {
        let total = self.frame_count();
        let frame = frame.min(total.saturating_sub(1));
        if self.current_frame() == Some(frame) {
            return false;
        }
        match self.apply_trajectory_frame(frame) {
            Ok(()) => true,
            Err(e) => {
                self.reset_playback();
                self.status_message = Some(StatusMessage::error(format!(
                    "Failed to read frame {}: {e}",
                    frame + 1
                )));
                false
            }
        }
    }

    /// 再生を止める。分子表面は、止めた今のモデルで作り直す。
    fn pause_playback(&mut self) -> Task<Message> {
        self.playback.pause();
        self.playback.scrubbing = false;
        self.trigger_surface_rebuild()
    }

    /// 構造が入れ替わったときなど、フレームの出どころが変わったときに、再生を止めて捨てる。
    /// （分子表面は、入れ替えの側が作り直す）
    pub(super) fn reset_playback(&mut self) {
        self.playback.pause();
        self.playback.scrubbing = false;
    }

    /// フレームを動かしたあとの分子表面。止まっているときは、すぐ作り直す。再生中は、追従させる。
    fn after_manual_frame_change(&mut self, changed: bool) -> Task<Message> {
        if !changed {
            Task::none()
        } else if self.playback.playing {
            self.follow_surface()
        } else {
            self.trigger_surface_rebuild()
        }
    }

    /// 操作のバー、メニューのコマンド、キーボードの再生の操作
    pub(super) fn handle_playback_action(&mut self, action: PlaybackAction) -> Task<Message> {
        match action {
            PlaybackAction::SetFps(fps) => {
                self.playback.set_fps(fps);
                return Task::none();
            }
            PlaybackAction::SetLoopMode(mode) => {
                self.playback.loop_mode = mode;
                return Task::none();
            }
            PlaybackAction::ScrubEnd => {
                if self.playback.scrubbing {
                    self.playback.scrubbing = false;
                    return self.trigger_surface_rebuild();
                }
                return Task::none();
            }
            _ => {}
        }

        let total = self.frame_count();
        if total < 2 {
            // フレームが 1 つしかない構造では、操作のバーは出ないので、キーボードやコマンドだけがここへ来る
            if action == PlaybackAction::PlayPause {
                self.status_message = Some(StatusMessage::warning(
                    "Nothing to play: the structure has fewer than two models".to_string(),
                ));
            }
            return Task::none();
        }
        let current = self.current_frame();

        match action {
            PlaybackAction::PlayPause => {
                if self.playback.playing {
                    return self.pause_playback();
                }
                let from = playback::start_frame(current, total, self.playback.loop_mode);
                self.show_frame(from);
                self.playback.start(Instant::now());
                Task::none()
            }
            PlaybackAction::First => {
                let changed = self.show_frame(0);
                self.after_manual_frame_change(changed)
            }
            PlaybackAction::Last => {
                let changed = self.show_frame(total - 1);
                self.after_manual_frame_change(changed)
            }
            PlaybackAction::Prev => {
                let target = match current {
                    Some(f) => f.saturating_sub(1),
                    None => total - 1,
                };
                let changed = self.show_frame(target);
                self.after_manual_frame_change(changed)
            }
            PlaybackAction::Next => {
                let target = match current {
                    Some(f) => (f + 1).min(total - 1),
                    None => 0,
                };
                let changed = self.show_frame(target);
                self.after_manual_frame_change(changed)
            }
            PlaybackAction::SetFrame(frame) => {
                // つまみを動かしている間も、分子表面は追従させる（離したときに、最後のモデルで作り直す）
                if !self.playback.playing {
                    self.playback.scrubbing = true;
                }
                let changed = self.show_frame(frame.min(total - 1));
                if changed {
                    self.follow_surface()
                } else {
                    Task::none()
                }
            }
            PlaybackAction::ScrubEnd
            | PlaybackAction::SetFps(_)
            | PlaybackAction::SetLoopMode(_) => Task::none(),
        }
    }

    /// タイマーの 1 回分。時刻と速さから進める量を求め、ループの方法に従って表示するモデルを進める。
    pub(super) fn handle_playback_tick(&mut self, now: Instant) -> Task<Message> {
        if !self.playback.playing {
            return Task::none();
        }
        let total = self.frame_count();
        if total < 2 {
            return self.pause_playback();
        }
        let steps = self.playback.steps_due(now);
        if steps == 0 {
            return Task::none();
        }
        let current = self.current_frame().unwrap_or(0);
        let next = playback::advance(
            current,
            total,
            self.playback.direction,
            self.playback.loop_mode,
            steps,
        );
        self.playback.direction = next.direction;
        let changed = self.show_frame(next.frame);
        if next.finished {
            // 「1回」で最後に着いた
            return self.pause_playback();
        }
        if changed {
            self.follow_surface()
        } else {
            Task::none()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_open_dispatch::FileOpenHandler;
    use crate::menu_bar::MenuAction;
    use crate::playback::{LoopMode, PlaybackAction as P};
    use crate::structure_tree::StructureTreeAction;
    use std::time::Duration;

    /// 1HLS.cif（20 モデル）を開いたアプリ。分子表面の Representation も足してある。
    fn app_with_models() -> Yui {
        let mut app = Yui::default();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/tests/data/1HLS.cif");
        let bytes = std::fs::read(path).expect("failed to read 1HLS.cif");
        let _ = app.update(Message::FileContentFetched {
            path: "1HLS.cif".to_string(),
            handler: FileOpenHandler::StructureViewer,
            result: Ok(bytes),
        });
        app.state
            .representations
            .push(yui_core::representation::Representation::new(
                "surface_test",
                yui_core::selector::Selector::All,
                yui_core::representation::RepresentationKind::Surface,
                yui_core::representation::ColorScheme::Element,
            ));
        app.surface_generating = false;
        app
    }

    /// 全モデル表示にする（構造を開いた直後は、先頭のモデルが表示されている）
    fn show_all(app: &mut Yui) {
        app.state.active_model = None;
    }

    fn single_model_app() -> Yui {
        let mut app = Yui::default();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/tests/data/1hls.pdb");
        let bytes = std::fs::read(path).expect("failed to read 1hls.pdb");
        let _ = app.update(Message::FileContentFetched {
            path: "1hls.pdb".to_string(),
            handler: FileOpenHandler::StructureViewer,
            result: Ok(bytes),
        });
        app
    }

    fn act(app: &mut Yui, action: P) {
        let _ = app.update(Message::PlaybackControl(action));
    }

    /// 時計を決めて再生を始める（時刻をテストで進めるため）
    fn play_at(app: &mut Yui, t0: Instant, fps: u32) {
        act(app, P::SetFps(fps));
        act(app, P::PlayPause);
        app.playback.start(t0);
    }

    #[test]
    fn test_playing_from_all_models_starts_at_the_first_model() {
        let mut app = app_with_models();
        assert!(app.has_playback());
        show_all(&mut app);
        act(&mut app, P::PlayPause);
        assert!(app.playback.playing);
        assert_eq!(app.current_frame(), Some(0));
        assert_eq!(app.state.active_model.as_deref(), Some("model_1"));
    }

    #[test]
    fn test_step_first_last_and_slider_change_the_displayed_model() {
        let mut app = app_with_models();
        let total = app.frame_count();
        assert_eq!(total, 20);

        show_all(&mut app);
        act(&mut app, P::Next); // 全モデル表示から: 先頭
        assert_eq!(app.current_frame(), Some(0));
        act(&mut app, P::Next);
        assert_eq!(app.current_frame(), Some(1));
        act(&mut app, P::Prev);
        act(&mut app, P::Prev); // 先頭より前へは行かない
        assert_eq!(app.current_frame(), Some(0));
        act(&mut app, P::Last);
        assert_eq!(app.current_frame(), Some(total - 1));
        act(&mut app, P::Next); // 末尾より先へは行かない
        assert_eq!(app.current_frame(), Some(total - 1));
        act(&mut app, P::First);
        assert_eq!(app.current_frame(), Some(0));
        act(&mut app, P::SetFrame(7));
        assert_eq!(app.current_frame(), Some(7));
        act(&mut app, P::SetFrame(999)); // 範囲を超えたら末尾
        assert_eq!(app.current_frame(), Some(total - 1));
    }

    #[test]
    fn test_prev_from_all_models_goes_to_the_last_model() {
        let mut app = app_with_models();
        show_all(&mut app);
        act(&mut app, P::Prev);
        assert_eq!(app.current_frame(), Some(app.frame_count() - 1));
    }

    #[test]
    fn test_surface_follows_while_playing_and_is_rebuilt_on_pause() {
        let mut app = app_with_models();
        let t0 = Instant::now();
        play_at(&mut app, t0, 10);
        assert_eq!(app.playback.hides_surface(), cfg!(target_arch = "wasm32"));

        // 進んだら、今のモデルで分子表面の生成を始める（ブラウザ版は描かないので、始めない）
        let _ = app.handle_playback_tick(t0 + Duration::from_secs(1));
        assert_eq!(app.current_frame(), Some(10));
        assert_eq!(app.surface_generating, cfg!(not(target_arch = "wasm32")));

        // 一時停止で、今のモデルの分子表面を作り直す
        act(&mut app, P::PlayPause);
        assert!(!app.playback.playing);
        assert!(!app.playback.hides_surface());
        assert!(app.surface_generating, "rebuild on pause");
        assert!(!app.surface_follow_pending);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn test_frames_during_a_build_are_skipped_and_the_latest_is_built_next() {
        let mut app = app_with_models();
        let t0 = Instant::now();
        play_at(&mut app, t0, 10);
        let _ = app.handle_playback_tick(t0 + Duration::from_millis(100));
        assert!(app.surface_generating);
        let generation = app.surface_generation_tracker.current_generation();
        assert!(!app.surface_follow_pending);

        // 生成の途中で先へ進んでも、世代は進めず（結果を捨てず）、印だけ付ける
        let _ = app.handle_playback_tick(t0 + Duration::from_millis(200));
        let _ = app.handle_playback_tick(t0 + Duration::from_millis(300));
        assert_eq!(
            app.surface_generation_tracker.current_generation(),
            generation
        );
        assert!(app.surface_follow_pending);

        // 結果が届くと、反映して、最新のモデルでもう一度作る
        let _ = app.update(Message::SurfaceGenerated {
            generation,
            results: Vec::new(),
        });
        assert!(!app.surface_follow_pending);
        assert!(app.surface_generating);
        assert_eq!(
            app.surface_generation_tracker.current_generation(),
            generation + 1
        );
    }

    #[test]
    fn test_stepping_while_paused_rebuilds_the_surface() {
        let mut app = app_with_models();
        act(&mut app, P::Next);
        assert!(app.surface_generating, "paused: rebuild for the new model");
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn test_scrubbing_follows_and_rebuilds_on_release() {
        let mut app = app_with_models();
        act(&mut app, P::SetFrame(3));
        assert!(app.surface_generating, "following while dragging");
        act(&mut app, P::SetFrame(4));
        assert!(
            app.surface_follow_pending,
            "second frame waits for the build"
        );
        act(&mut app, P::ScrubEnd);
        assert!(!app.playback.hides_surface());
        assert!(app.surface_generating, "rebuild when released");
        assert!(!app.surface_follow_pending);
        assert_eq!(app.current_frame(), Some(4));
    }

    #[test]
    fn test_playback_loops_by_mode() {
        let t0 = Instant::now();
        // 繰り返し: 末尾の次は先頭
        let mut app = app_with_models();
        act(&mut app, P::SetLoopMode(LoopMode::Repeat));
        play_at(&mut app, t0, 10);
        act(&mut app, P::Last);
        let _ = app.handle_playback_tick(t0 + Duration::from_millis(100));
        assert_eq!(app.current_frame(), Some(0));
        assert!(app.playback.playing);

        // 往復: 末尾で向きが反転する
        let mut app = app_with_models();
        act(&mut app, P::SetLoopMode(LoopMode::PingPong));
        play_at(&mut app, t0, 10);
        act(&mut app, P::Last);
        let _ = app.handle_playback_tick(t0 + Duration::from_millis(100));
        assert_eq!(app.current_frame(), Some(app.frame_count() - 2));
        assert_eq!(app.playback.direction, crate::playback::Direction::Backward);

        // 1回: 末尾で止まり、分子表面を作り直す
        let mut app = app_with_models();
        act(&mut app, P::SetLoopMode(LoopMode::Once));
        play_at(&mut app, t0, 10);
        act(&mut app, P::Last);
        app.surface_generating = false;
        let _ = app.handle_playback_tick(t0 + Duration::from_millis(100));
        assert!(!app.playback.playing);
        assert_eq!(app.current_frame(), Some(app.frame_count() - 1));
        assert!(app.surface_generating, "rebuild when playback ends");
    }

    #[test]
    fn test_frame_change_clears_the_selection() {
        let mut app = app_with_models();
        app.selection.insert(1);
        act(&mut app, P::Next);
        assert!(app.selection.is_empty(), "stepping");

        app.selection.insert(1);
        let t0 = Instant::now();
        play_at(&mut app, t0, 10);
        let _ = app.handle_playback_tick(t0 + Duration::from_millis(100));
        assert!(app.selection.is_empty(), "playing");

        app.selection.insert(1);
        let _ = app.update(Message::StructureTreeAction(
            StructureTreeAction::SelectModel(Some("model_5".to_string())),
        ));
        assert!(app.selection.is_empty(), "tree model switch");
    }

    #[test]
    fn test_manual_model_choice_stops_playback_and_shows_that_model() {
        let mut app = app_with_models();
        play_at(&mut app, Instant::now(), 10);
        app.surface_generating = false;
        let _ = app.update(Message::StructureTreeAction(
            StructureTreeAction::SelectModel(Some("model_9".to_string())),
        ));
        assert!(!app.playback.playing);
        assert_eq!(app.state.active_model.as_deref(), Some("model_9"));
        assert!(app.surface_generating, "the surface is rebuilt for it");
    }

    #[test]
    fn test_no_status_message_per_frame_while_playing() {
        let mut app = app_with_models();
        let t0 = Instant::now();
        play_at(&mut app, t0, 10);
        app.status_message = None;
        let _ = app.handle_playback_tick(t0 + Duration::from_millis(300));
        assert!(app.status_message.is_none());
    }

    #[test]
    fn test_opening_or_closing_a_structure_stops_playback() {
        let mut app = app_with_models();
        play_at(&mut app, Instant::now(), 10);
        let _ = app.update(Message::MenuAction(MenuAction::FileNew));
        assert!(!app.playback.playing);
        assert_eq!(app.frame_count(), 0);

        let mut app = app_with_models();
        play_at(&mut app, Instant::now(), 10);
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/tests/data/1hls.pdb");
        let bytes = std::fs::read(path).unwrap();
        let _ = app.update(Message::FileContentFetched {
            path: "1hls.pdb".to_string(),
            handler: FileOpenHandler::StructureViewer,
            result: Ok(bytes),
        });
        assert!(
            !app.playback.playing,
            "a single-model structure replaced it"
        );
        assert!(!app.has_playback());
    }

    #[test]
    fn test_single_model_structure_has_no_playback() {
        let mut app = single_model_app();
        assert!(!app.has_playback());
        assert!(app.view_playback_bar().is_none());
        act(&mut app, P::PlayPause);
        assert!(!app.playback.playing);
        assert!(app.status_message.is_some(), "explains why nothing plays");
        act(&mut app, P::Next);
        assert_eq!(app.state.active_model, None);
    }

    #[test]
    fn test_bar_is_shown_for_a_multi_model_structure() {
        let app = app_with_models();
        assert!(app.view_playback_bar().is_some());
    }

    #[test]
    fn test_timer_runs_only_while_playing() {
        let mut app = app_with_models();
        assert!(app.playback.tick_interval().is_none());
        act(&mut app, P::PlayPause);
        assert!(app.playback.tick_interval().is_some());
        act(&mut app, P::PlayPause);
        assert!(app.playback.tick_interval().is_none());
    }

    #[test]
    fn test_a_tick_while_paused_does_nothing() {
        let mut app = app_with_models();
        act(&mut app, P::SetFrame(3));
        act(&mut app, P::ScrubEnd);
        let _ = app.handle_playback_tick(Instant::now() + Duration::from_secs(10));
        assert_eq!(app.current_frame(), Some(3));
    }

    #[test]
    fn test_playback_is_not_part_of_the_workspace_or_collaboration() {
        // 再生の状態は `Yui.playback` にだけある。ワークスペース（YAML）の `trajectory` は変わらない。
        let mut app = app_with_models();
        let before = serde_yaml::to_string(&app.state.trajectory).unwrap();
        act(&mut app, P::PlayPause);
        act(&mut app, P::SetFps(20));
        act(&mut app, P::SetLoopMode(LoopMode::PingPong));
        assert_eq!(
            serde_yaml::to_string(&app.state.trajectory).unwrap(),
            before
        );
    }
}
