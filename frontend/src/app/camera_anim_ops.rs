//! カメラのアニメーションの操作（architecture.md 8.11節）。
//!
//! 純粋な計算は `crate::camera_anim`。ここは、どの分子図を動かすか、タイマーの時刻の管理、
//! 操作したら止める条件、視点の連動、構造に合わせる移動を、アプリの状態に結び付ける。
//! アニメーションは利用者ごとのもので、`AppState` にも協調編集にも入れない（視点の結果は、普通の視点の変更と同じ扱い）。

use iced::time::Instant;

use super::Yui;
use crate::camera_anim::{self, CameraAction, CameraAnimation};
use crate::status_bar::StatusMessage;
use crate::viewport::ViewportId;

/// もう一方の分子図
fn other(id: ViewportId) -> ViewportId {
    match id {
        ViewportId::Primary => ViewportId::Secondary,
        ViewportId::Secondary => ViewportId::Primary,
    }
}

impl Yui {
    /// メニューの操作で動かす分子図。最後に視点を動かした分子図（2つ目を表示していなければ、1つ目）。
    pub(super) fn animation_target(&self) -> ViewportId {
        if self.active_viewport == ViewportId::Secondary
            && crate::pane_view::has_second_viewport(&self.pane_views)
        {
            ViewportId::Secondary
        } else {
            ViewportId::Primary
        }
    }

    /// アニメーションを始める。時計は、最初のアニメーションが始まるときに合わせる。
    pub(super) fn start_camera_animation(&mut self, id: ViewportId, anim: CameraAnimation) {
        if self.camera_anims.is_empty() {
            self.camera_anim_clock = Some(Instant::now());
        }
        self.camera_anims.insert(id, anim);
    }

    /// 分子図のアニメーションを止める。連動していれば、もう一方も止める（同じ視点を回しているため）。
    pub(super) fn stop_camera_animation(&mut self, id: ViewportId) {
        self.camera_anims.remove(&id);
        if self.camera_link {
            self.camera_anims.remove(&other(id));
        }
        if self.camera_anims.is_empty() {
            self.camera_anim_clock = None;
        }
    }

    /// すべてのアニメーションを止める（構造が入れ替わるときなど）
    pub(super) fn stop_all_camera_animations(&mut self) {
        self.camera_anims.clear();
        self.camera_anim_clock = None;
        self.animate_fit.clear();
    }

    pub(super) fn is_spinning(&self) -> bool {
        self.camera_anims
            .get(&self.animation_target())
            .is_some_and(CameraAnimation::is_spin)
    }

    pub(super) fn is_rocking(&self) -> bool {
        self.camera_anims
            .get(&self.animation_target())
            .is_some_and(CameraAnimation::is_rock)
    }

    pub(super) fn handle_camera_action(&mut self, action: CameraAction) {
        let id = self.animation_target();
        match action {
            CameraAction::ToggleSpin => {
                if self.is_spinning() {
                    self.stop_camera_animation(id);
                } else {
                    self.start_camera_animation(id, CameraAnimation::spin(self.spin_speed));
                }
            }
            CameraAction::ToggleRock => {
                if self.is_rocking() {
                    self.stop_camera_animation(id);
                } else {
                    let base = self.camera_of(id).clone();
                    self.start_camera_animation(id, CameraAnimation::rock(base));
                }
            }
            CameraAction::CycleSpinSpeed => {
                self.spin_speed = camera_anim::next_spin_speed(self.spin_speed);
                // 回っているときは、新しい速さですぐに回す
                if self.is_spinning() {
                    self.start_camera_animation(id, CameraAnimation::spin(self.spin_speed));
                }
            }
            CameraAction::ZoomToSelection => self.zoom_to_selection(),
        }
    }

    /// 選ばれている原子に合わせる。合わせる視点は分子図が（縦横比を使って）求め、結果が届いたら、滑らかに動く。
    fn zoom_to_selection(&mut self) {
        if self.selection.is_empty() {
            self.status_message = Some(StatusMessage::warning("Nothing is selected".to_string()));
            return;
        }
        self.request_camera_fit();
        self.fit_selection = true;
        self.animate_fit = self.fit_pending.clone();
        self.status_message = Some(StatusMessage::info(format!(
            "Zooming to {} selected atom(s)",
            self.selection.len()
        )));
    }

    /// Reset Camera: 構造に合わせて、滑らかに動く
    pub(super) fn request_smooth_camera_fit(&mut self) {
        self.request_camera_fit();
        self.animate_fit = self.fit_pending.clone();
    }

    /// タイマーの 1 回分。経過時間だけ、それぞれのアニメーションを進める。
    pub(super) fn handle_camera_anim_tick(&mut self, now: Instant) {
        if self.camera_anims.is_empty() {
            self.camera_anim_clock = None;
            return;
        }
        let dt = match self.camera_anim_clock {
            Some(last) => now.saturating_duration_since(last).as_secs_f32(),
            None => 0.0,
        };
        self.camera_anim_clock = Some(now);

        let ids: Vec<ViewportId> = self.camera_anims.keys().copied().collect();
        for id in ids {
            let current = self.camera_of(id).clone();
            let Some(anim) = self.camera_anims.get_mut(&id) else {
                continue;
            };
            let (camera, finished) = anim.advance(&current, dt);
            let continuous = anim.is_continuous();
            if finished {
                self.camera_anims.remove(&id);
            }
            self.set_animated_camera(id, camera, continuous);
        }
        if self.camera_anims.is_empty() {
            self.camera_anim_clock = None;
        }
    }

    /// アニメーションが決めた視点を反映する。連動がオンなら、回転と首振りは、もう一方にも同じ視点を入れる
    /// （構造に合わせる移動は、縦横比が違うので、各自で行う）。
    fn set_animated_camera(
        &mut self,
        id: ViewportId,
        camera: yui_core::state::CameraState,
        continuous: bool,
    ) {
        if self.camera_link && continuous {
            self.store_camera(other(id), camera.clone());
        }
        self.store_camera(id, camera);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Message;
    use crate::menu_bar::MenuAction;
    use std::time::Duration;
    use yui_core::state::CameraState;

    fn cam(x: f32) -> CameraState {
        CameraState {
            eye: [x, 0.0, 50.0],
            target: [x, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            fov: 45.0,
        }
    }

    fn azimuth(c: &CameraState) -> f32 {
        let d = crate::math::vec3_sub(c.eye, c.target);
        d[2].atan2(d[0]).to_degrees()
    }

    fn act(app: &mut Yui, action: CameraAction) {
        let _ = app.update(Message::CameraAction(action));
    }

    fn split(app: &mut Yui) {
        let _ = app.update(Message::MenuAction(MenuAction::ViewToggleEditorSplit));
    }

    /// 時計を決めて、`secs` 秒ぶんだけ進める
    fn run(app: &mut Yui, t0: Instant, secs: f32) {
        app.camera_anim_clock = Some(t0);
        let steps = (secs / 0.05).round() as usize;
        for i in 1..=steps {
            app.handle_camera_anim_tick(t0 + Duration::from_secs_f32(0.05 * i as f32));
        }
    }

    fn mouse_moved(app: &mut Yui, id: ViewportId, camera: CameraState) {
        let _ = app.update(Message::ViewportCameraChanged {
            id,
            camera,
            fitted: false,
        });
    }

    #[test]
    fn test_spin_turns_the_primary_camera_by_speed_times_time() {
        let mut app = Yui::default();
        app.state.camera = cam(0.0);
        let before = azimuth(&app.state.camera);
        act(&mut app, CameraAction::ToggleSpin);
        assert!(app.is_spinning());
        run(&mut app, Instant::now(), 2.0);
        let turned = (azimuth(&app.state.camera) - before).abs();
        // 既定 20 度/秒 × 2 秒
        assert!((turned - 40.0).abs() < 0.5, "turned {turned}");
    }

    #[test]
    fn test_toggling_spin_and_rock_switches_between_them() {
        let mut app = Yui::default();
        act(&mut app, CameraAction::ToggleSpin);
        assert!(app.is_spinning() && !app.is_rocking());
        act(&mut app, CameraAction::ToggleRock);
        assert!(app.is_rocking() && !app.is_spinning());
        act(&mut app, CameraAction::ToggleRock);
        assert!(!app.is_rocking());
        assert!(app.camera_anims.is_empty());
        assert!(app.camera_anim_clock.is_none(), "no clock, so no timer");
    }

    #[test]
    fn test_rock_stays_within_its_range() {
        let mut app = Yui::default();
        app.state.camera = cam(0.0);
        let base = azimuth(&app.state.camera);
        act(&mut app, CameraAction::ToggleRock);
        let t0 = Instant::now();
        app.camera_anim_clock = Some(t0);
        for i in 1..=200 {
            app.handle_camera_anim_tick(t0 + Duration::from_secs_f32(0.05 * i as f32));
            let off = azimuth(&app.state.camera) - base;
            assert!(off.abs() <= camera_anim::ROCK_AMPLITUDE_DEG + 0.1, "{off}");
        }
    }

    #[test]
    fn test_spin_speed_cycles_and_applies_to_a_running_spin() {
        let mut app = Yui::default();
        let first = app.spin_speed;
        act(&mut app, CameraAction::CycleSpinSpeed);
        assert_ne!(app.spin_speed, first);
        act(&mut app, CameraAction::ToggleSpin);
        let before = app.spin_speed;
        act(&mut app, CameraAction::CycleSpinSpeed);
        assert_ne!(app.spin_speed, before);
        assert!(app.is_spinning(), "still spinning, at the new speed");
        assert!(matches!(
            app.camera_anims.get(&ViewportId::Primary).unwrap().kind,
            camera_anim::AnimKind::Spin { deg_per_sec } if deg_per_sec == app.spin_speed
        ));
    }

    #[test]
    fn test_mouse_operation_stops_the_animation() {
        for start in [CameraAction::ToggleSpin, CameraAction::ToggleRock] {
            let mut app = Yui::default();
            act(&mut app, start);
            assert!(!app.camera_anims.is_empty());
            mouse_moved(&mut app, ViewportId::Primary, cam(3.0));
            assert!(app.camera_anims.is_empty(), "{start:?}");
            assert!(app.camera_anim_clock.is_none());
            assert_eq!(app.state.camera, cam(3.0));
        }
    }

    #[test]
    fn test_mouse_operation_stops_the_move_to_a_fitted_camera() {
        let mut app = Yui::default();
        app.state.camera = cam(0.0);
        app.start_camera_animation(
            ViewportId::Primary,
            CameraAnimation::move_between(cam(0.0), cam(40.0)),
        );
        mouse_moved(&mut app, ViewportId::Primary, cam(5.0));
        assert!(app.camera_anims.is_empty());
    }

    #[test]
    fn test_animation_acts_on_the_viewport_that_was_moved_last() {
        let mut app = Yui::default();
        split(&mut app);
        assert_eq!(app.animation_target(), ViewportId::Primary);
        mouse_moved(&mut app, ViewportId::Secondary, cam(7.0));
        assert_eq!(app.animation_target(), ViewportId::Secondary);
        act(&mut app, CameraAction::ToggleSpin);
        assert!(app.camera_anims.contains_key(&ViewportId::Secondary));
        assert!(!app.camera_anims.contains_key(&ViewportId::Primary));

        // 分割をやめたら、1つ目が対象
        let mut app = Yui::default();
        split(&mut app);
        mouse_moved(&mut app, ViewportId::Secondary, cam(7.0));
        split(&mut app);
        assert_eq!(app.animation_target(), ViewportId::Primary);
    }

    #[test]
    fn test_link_mirrors_spin_to_the_other_viewport_and_unlinked_does_not() {
        let mut app = Yui::default();
        split(&mut app);
        app.state.camera = cam(0.0);
        app.secondary_camera = Some(cam(9.0));
        act(&mut app, CameraAction::ToggleSpin);
        run(&mut app, Instant::now(), 1.0);
        assert_eq!(app.secondary_camera, Some(cam(9.0)), "unlinked: untouched");

        let mut app = Yui::default();
        split(&mut app);
        app.camera_link = true;
        app.secondary_camera = Some(app.state.camera.clone());
        act(&mut app, CameraAction::ToggleSpin);
        run(&mut app, Instant::now(), 1.0);
        assert_eq!(app.secondary_camera.as_ref(), Some(&app.state.camera));
        assert_ne!(app.state.camera, CameraState::default());

        // 連動しているとき、どちらを動かしても、両方のアニメーションが止まる
        mouse_moved(&mut app, ViewportId::Secondary, cam(1.0));
        assert!(app.camera_anims.is_empty());
    }

    #[test]
    fn test_reset_camera_moves_smoothly_to_the_fitted_camera() {
        let mut app = Yui::default();
        app.state.camera = cam(0.0);
        let _ = app.update(Message::MenuAction(MenuAction::ViewResetCamera));
        assert!(app.animate_fit.contains(&ViewportId::Primary));

        // 分子図が、合わせた視点を知らせてきた
        let fitted = cam(30.0);
        let _ = app.update(Message::ViewportCameraChanged {
            id: ViewportId::Primary,
            camera: fitted.clone(),
            fitted: true,
        });
        // すぐには切り替えず、今の視点から動き始める
        assert_eq!(app.state.camera, cam(0.0));
        assert!(app.camera_anims.contains_key(&ViewportId::Primary));
        assert!(!app.fit_pending.contains(&ViewportId::Primary));

        run(&mut app, Instant::now(), 0.2);
        let mid = app.state.camera.target[0];
        assert!(mid > 0.0 && mid < 30.0, "in between: {mid}");

        run(&mut app, Instant::now(), 0.6);
        assert_eq!(app.state.camera, fitted);
        assert!(app.camera_anims.is_empty());
        assert!(app.camera_anim_clock.is_none());
    }

    #[test]
    fn test_fit_for_a_new_structure_is_not_animated() {
        let mut app = Yui::default();
        app.state.camera = cam(0.0);
        app.request_camera_fit(); // 構造を開いたとき
        assert!(app.animate_fit.is_empty());
        let _ = app.update(Message::ViewportCameraChanged {
            id: ViewportId::Primary,
            camera: cam(30.0),
            fitted: true,
        });
        assert_eq!(app.state.camera, cam(30.0));
        assert!(app.camera_anims.is_empty());
    }

    #[test]
    fn test_opening_a_structure_stops_the_animation() {
        let mut app = Yui::default();
        act(&mut app, CameraAction::ToggleSpin);
        app.request_camera_fit();
        assert!(app.camera_anims.is_empty());
        assert!(app.camera_anim_clock.is_none());
    }

    #[test]
    fn test_zoom_to_selection_needs_a_selection() {
        let mut app = Yui::default();
        let token = app.camera_fit_token;
        act(&mut app, CameraAction::ZoomToSelection);
        assert_eq!(app.camera_fit_token, token, "nothing to fit");
        assert!(app.status_message.is_some());
        assert!(!app.fit_selection);
    }

    #[test]
    fn test_zoom_to_selection_fits_the_selection_then_moves_smoothly() {
        let mut app = Yui::default();
        app.selection.insert(1);
        app.state.camera = cam(0.0);
        let token = app.camera_fit_token;
        act(&mut app, CameraAction::ZoomToSelection);
        assert_ne!(app.camera_fit_token, token);
        assert!(app.fit_selection);
        assert!(app.animate_fit.contains(&ViewportId::Primary));

        let _ = app.update(Message::ViewportCameraChanged {
            id: ViewportId::Primary,
            camera: cam(12.0),
            fitted: true,
        });
        assert!(!app.fit_selection, "the request is done");
        assert!(app.camera_anims.contains_key(&ViewportId::Primary));
        assert_eq!(app.state.camera, cam(0.0));
    }

    #[test]
    fn test_animation_is_not_part_of_the_workspace() {
        use crate::pane_view::export_workspace_to_yaml_str;
        let mut app = Yui::default();
        let before = export_workspace_to_yaml_str(&app.state, &app.pane_views).unwrap();
        act(&mut app, CameraAction::ToggleSpin);
        act(&mut app, CameraAction::CycleSpinSpeed);
        // 視点が動くまで、保存されるものは変わらない（アニメーションの状態は、YAML に入らない）
        let after = export_workspace_to_yaml_str(&app.state, &app.pane_views).unwrap();
        assert_eq!(before, after);
    }
}
