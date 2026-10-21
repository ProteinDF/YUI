//! 2つの分子図の視点の持ち方と、連動・構造への合わせ込みの管理。
//!
//! - 1つ目の視点は `AppState.camera`（ワークスペースに保存され、協調編集の差分でも変わる）。
//! - 2つ目の視点は `Yui.secondary_camera`（その画面だけのもの。保存も協調編集もしない）。
//!
//! 分子図のウィジェットは、視点をアプリから受け取って描き、操作で視点が変わると `ViewportCameraChanged` を送る。

use yui_core::state::CameraState;

use super::Yui;
use crate::camera_anim::CameraAnimation;
use crate::menu_bar::default_camera_state;
use crate::pane_view::has_second_viewport;
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
    /// 分子図の視点（2つ目をまだ表示していなければ、1つ目と同じ）
    pub(super) fn camera_of(&self, id: ViewportId) -> &CameraState {
        match id {
            ViewportId::Primary => &self.state.camera,
            ViewportId::Secondary => self.secondary_camera.as_ref().unwrap_or(&self.state.camera),
        }
    }

    pub(super) fn store_camera(&mut self, id: ViewportId, camera: CameraState) {
        match id {
            ViewportId::Primary => self.state.camera = camera,
            ViewportId::Secondary => self.secondary_camera = Some(camera),
        }
    }

    /// 分子図から、視点が変わったと知らせが届いたとき。
    ///
    /// - `fitted`（構造に合わせた結果）なら、その分子図の「合わせ待ち」を終える。連動では伝えない（縦横比が違うため、各自で合わせる）。
    /// - 操作による変更で、連動がオンなら、もう一方の視点も同じにする。
    pub(super) fn handle_camera_changed(
        &mut self,
        id: ViewportId,
        camera: CameraState,
        fitted: bool,
    ) {
        if fitted {
            self.fit_pending.remove(&id);
            if self.fit_pending.is_empty() {
                self.fit_selection = false;
            }
            // Reset Camera / Zoom to Selection: 合わせた視点へ、今の視点から滑らかに動く。
            // （分子図は合わせた視点を持っているが、アプリの視点は今のままなので、アプリの視点が使われる）
            let from = self.camera_of(id).clone();
            if self.animate_fit.remove(&id) && from != camera {
                self.start_camera_animation(id, CameraAnimation::move_between(from, camera));
                return;
            }
        } else {
            // 利用者が視点を動かした（ドラッグ、ホイール）: アニメーションは止め、この分子図を、メニューの対象にする
            self.stop_camera_animation(id);
            self.active_viewport = id;
            if self.camera_link {
                self.store_camera(other(id), camera.clone());
            }
        }
        self.store_camera(id, camera);
    }

    /// View → Link Viewport Cameras。オンにした時点で、2つ目の視点を1つ目に揃える。
    pub(super) fn toggle_camera_link(&mut self) {
        self.camera_link = !self.camera_link;
        if self.camera_link {
            self.secondary_camera = Some(self.state.camera.clone());
        }
        self.status_message = Some(StatusMessage::info(if self.camera_link {
            "Viewport cameras are linked.".to_string()
        } else {
            "Viewport cameras are unlinked.".to_string()
        }));
    }

    /// 2つ目の分子図を表示するとき（分割をオンにしたとき）、まだ視点がなければ1つ目をコピーする。
    pub(super) fn on_secondary_viewport_shown(&mut self) {
        if self.secondary_camera.is_none() {
            self.secondary_camera = Some(self.state.camera.clone());
            // コピーした視点を、すぐに構造へ合わせ直さない
            self.fit_pending.remove(&ViewportId::Secondary);
        }
    }

    /// ワークスペースや backend の状態を読み込んだあとの視点。
    ///
    /// 保存されていた視点が、既定の視点のまま（古い版は視点を保存しなかった）なら、構造に合わせる。
    /// 利用者が動かした視点が保存されていれば、それをそのまま使う（合わせない）。
    pub(super) fn apply_loaded_camera(&mut self) {
        // 構造が入れ替わったので、古い原子の選択は捨てる
        self.selection.clear();
        self.stop_all_camera_animations();
        let camera = &self.state.camera;
        if *camera == CameraState::default() || *camera == default_camera_state() {
            self.request_camera_fit();
        } else {
            self.fit_pending.clear();
            self.camera_fit_token = self.camera_fit_token.wrapping_add(1);
            self.secondary_camera = has_second_viewport(&self.pane_views).then(|| camera.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Message;
    use crate::menu_bar::MenuAction;
    use crate::viewport::effective_camera;

    fn cam(x: f32) -> CameraState {
        CameraState {
            eye: [x, 0.0, 50.0],
            target: [x, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            fov: 45.0,
        }
    }

    fn split(app: &mut Yui) {
        let _ = app.update(Message::MenuAction(MenuAction::ViewToggleEditorSplit));
    }

    fn changed(app: &mut Yui, id: ViewportId, camera: CameraState) {
        let _ = app.update(Message::ViewportCameraChanged {
            id,
            camera,
            fitted: false,
        });
    }

    #[test]
    fn test_primary_camera_change_updates_app_state() {
        let mut app = Yui::default();
        changed(&mut app, ViewportId::Primary, cam(3.0));
        assert_eq!(app.state.camera, cam(3.0));
        assert_eq!(app.camera_of(ViewportId::Primary), &cam(3.0));
    }

    #[test]
    fn test_link_off_keeps_cameras_independent() {
        let mut app = Yui::default();
        split(&mut app);
        let secondary_before = app.camera_of(ViewportId::Secondary).clone();
        changed(&mut app, ViewportId::Primary, cam(3.0));
        assert_eq!(app.camera_of(ViewportId::Secondary), &secondary_before);
        changed(&mut app, ViewportId::Secondary, cam(7.0));
        assert_eq!(app.camera_of(ViewportId::Secondary), &cam(7.0));
        assert_eq!(app.state.camera, cam(3.0));
    }

    #[test]
    fn test_link_on_propagates_both_ways_and_aligns_when_enabled() {
        let mut app = Yui::default();
        split(&mut app);
        changed(&mut app, ViewportId::Primary, cam(3.0));
        changed(&mut app, ViewportId::Secondary, cam(7.0));

        // オンにした時点で、2つ目を1つ目に揃える
        let _ = app.update(Message::MenuAction(MenuAction::ViewToggleCameraLink));
        assert!(app.camera_link);
        assert_eq!(app.camera_of(ViewportId::Secondary), &cam(3.0));

        changed(&mut app, ViewportId::Primary, cam(4.0));
        assert_eq!(app.camera_of(ViewportId::Secondary), &cam(4.0));
        changed(&mut app, ViewportId::Secondary, cam(9.0));
        assert_eq!(app.state.camera, cam(9.0));

        // オフに戻すと、また独立する
        let _ = app.update(Message::MenuAction(MenuAction::ViewToggleCameraLink));
        changed(&mut app, ViewportId::Primary, cam(1.0));
        assert_eq!(app.camera_of(ViewportId::Secondary), &cam(9.0));
    }

    #[test]
    fn test_secondary_camera_starts_as_copy_of_primary() {
        let mut app = Yui::default();
        changed(&mut app, ViewportId::Primary, cam(3.0));
        split(&mut app);
        assert_eq!(app.secondary_camera, Some(cam(3.0)));

        // 分割をオフ・オンしても、2つ目の視点は保たれる（最初に表示するときだけコピーする）
        changed(&mut app, ViewportId::Secondary, cam(7.0));
        split(&mut app);
        changed(&mut app, ViewportId::Primary, cam(5.0));
        split(&mut app);
        assert_eq!(app.secondary_camera, Some(cam(7.0)));
    }

    #[test]
    fn test_reset_camera_requests_fit_for_both_viewports() {
        let mut app = Yui::default();
        split(&mut app);
        app.fit_pending.clear();
        let token = app.camera_fit_token;
        let _ = app.update(Message::MenuAction(MenuAction::ViewResetCamera));
        assert_ne!(app.camera_fit_token, token);
        assert!(app.fit_pending.contains(&ViewportId::Primary));
        assert!(app.fit_pending.contains(&ViewportId::Secondary));

        // 合わせた結果が届くと、その分子図の待ちが終わる。連動はオンでも伝えない。
        app.camera_link = true;
        let _ = app.update(Message::ViewportCameraChanged {
            id: ViewportId::Primary,
            camera: cam(2.0),
            fitted: true,
        });
        assert!(!app.fit_pending.contains(&ViewportId::Primary));
        assert!(app.fit_pending.contains(&ViewportId::Secondary));
        assert_ne!(app.secondary_camera, Some(cam(2.0)));
    }

    #[test]
    fn test_reset_camera_without_split_only_fits_primary() {
        let mut app = Yui {
            secondary_camera: Some(cam(7.0)),
            ..Yui::default()
        };
        let _ = app.update(Message::MenuAction(MenuAction::ViewResetCamera));
        assert!(app.fit_pending.contains(&ViewportId::Primary));
        assert!(!app.fit_pending.contains(&ViewportId::Secondary));
        assert_eq!(app.secondary_camera, None);
    }

    #[test]
    fn test_saved_camera_is_kept_on_load_but_default_camera_is_fitted() {
        let mut app = Yui::default();

        app.state.camera = cam(3.0);
        app.fit_pending.clear();
        app.apply_loaded_camera();
        assert!(app.fit_pending.is_empty(), "動かした視点は、合わせない");
        assert_eq!(app.state.camera, cam(3.0));

        app.state.camera = CameraState::default();
        app.apply_loaded_camera();
        assert!(app.fit_pending.contains(&ViewportId::Primary));

        app.fit_pending.clear();
        app.state.camera = default_camera_state();
        app.apply_loaded_camera();
        assert!(app.fit_pending.contains(&ViewportId::Primary));
    }

    #[test]
    fn test_workspace_yaml_keeps_primary_camera_only() {
        use crate::pane_view::{export_workspace_to_yaml_str, import_workspace_from_yaml_str};
        let mut app = Yui::default();
        split(&mut app);
        changed(&mut app, ViewportId::Primary, cam(3.0));
        changed(&mut app, ViewportId::Secondary, cam(7.0));

        let yaml = export_workspace_to_yaml_str(&app.state, &app.pane_views).unwrap();
        assert!(!yaml.contains("x: 7"), "2つ目の視点は保存しない");
        let (state, _) = import_workspace_from_yaml_str(&yaml).unwrap();
        assert_eq!(state.camera, cam(3.0));
        // 2つ目の視点は、AppState のどこにも入らない
        let json = serde_json::to_string(&app.state).unwrap();
        assert!(!json.contains("7.0,0.0,50.0"));
    }

    #[test]
    fn test_external_camera_change_is_applied_to_primary_view() {
        // 協調編集の差分で AppState.camera が変わる
        let mut app = Yui::default();
        let delta = yui_core::state::AppStateDelta {
            camera: Some(cam(8.0)),
            ..Default::default()
        };
        let _ = app.update(Message::CollabDeltaReceived(delta));
        assert_eq!(app.camera_of(ViewportId::Primary), &cam(8.0));

        // ウィジェットは、最後にやりとりした視点（synced）と違う外の視点を採り入れる
        let local = cam(1.0);
        assert_eq!(
            effective_camera(&local, Some(&cam(2.0)), &cam(8.0)),
            &cam(8.0)
        );
    }

    #[test]
    fn test_effective_camera_prefers_local_while_in_sync() {
        let local = cam(5.0);
        let external = cam(5.0);
        // 同期している間は、操作中の手元の視点
        let ahead = cam(6.0);
        assert_eq!(effective_camera(&ahead, Some(&external), &external), &ahead);
        // まだやりとりしていなければ、アプリ側
        assert_eq!(effective_camera(&local, None, &cam(9.0)), &cam(9.0));
    }
}
