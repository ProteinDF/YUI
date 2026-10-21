//! 軌跡ファイルの再生の操作（architecture.md 4.2節）。
//!
//! 軌跡は、構造（トポロジー）と同じ原子の並びの座標の列。開くと、再生の基盤（`playback`）のフレームの出どころが、
//! 構造のモデルから軌跡に切り替わる（`playback_ops` の `frame_count` / `current_frame` / `show_frame`）。

use std::sync::Arc;

use iced::Task;
use yui_core::xyz_trajectory::XyzTrajectory;

use super::{Message, Yui};
use crate::status_bar::StatusMessage;
use crate::trajectory::LoadedTrajectory;

impl Yui {
    /// 軌跡が開かれたとき（File メニュー、Project タブ）。構造と原子数が合わなければ、エラーにする。
    pub(super) fn handle_trajectory_opened(
        &mut self,
        result: Result<Option<Arc<XyzTrajectory>>, String>,
    ) -> Task<Message> {
        let traj = match result {
            Ok(Some(traj)) => traj,
            Ok(None) => return Task::none(),
            Err(e) => {
                self.status_message = Some(StatusMessage::error(format!(
                    "Failed to open the trajectory: {e}"
                )));
                return Task::none();
            }
        };
        if self.state.atom_group.is_none() {
            self.status_message = Some(StatusMessage::warning(
                "Open a structure first: a trajectory is opened for the structure that is shown"
                    .to_string(),
            ));
            return Task::none();
        }
        let atoms = self.state.molecule.atoms.len();
        if traj.num_atoms() != atoms {
            self.status_message = Some(StatusMessage::error(format!(
                "The trajectory '{}' has {} atoms, but the structure has {atoms}",
                traj.name(),
                traj.num_atoms()
            )));
            return Task::none();
        }

        // 前の軌跡があれば、構造の座標を元に戻してから、新しい軌跡を開く
        self.restore_original_positions();
        self.reset_playback();
        let original: Vec<[f32; 3]> = self
            .state
            .molecule
            .atoms
            .iter()
            .map(|a| a.position)
            .collect();
        let name = traj.name().to_string();
        let frames = traj.frame_count();
        self.trajectory = Some(LoadedTrajectory {
            traj,
            current: 0,
            step: 0,
            original,
        });
        if let Err(e) = self.apply_trajectory_frame(0) {
            self.trajectory = None;
            self.restore_original_positions();
            self.status_message = Some(StatusMessage::error(format!(
                "Failed to read the first frame of '{name}': {e}"
            )));
            return Task::none();
        }
        self.status_message = Some(StatusMessage::info(format!(
            "Loaded trajectory '{name}': {frames} frames, {atoms} atoms"
        )));
        self.trigger_surface_rebuild()
    }

    /// File → Close Trajectory: 再生を止めて、構造を軌跡を開く前の座標に戻す。
    pub(super) fn close_trajectory(&mut self) -> Task<Message> {
        if self.trajectory.is_none() {
            return Task::none();
        }
        self.reset_playback();
        self.restore_original_positions();
        self.trajectory = None;
        self.status_message = Some(StatusMessage::info("Closed the trajectory".to_string()));
        self.trigger_surface_rebuild()
    }

    /// 軌跡のフレームを、構造の座標に反映する。
    ///
    /// 更新するもの: 構造の木（`atom_group`。リボンと相互作用の端点が、これを使う）、平坦化キャッシュ（`molecule`。
    /// 球・スティック・ピッキング・分子表面が使う）、描画用の相互作用（端点の座標だけを解決し直す。検出はしない）。
    /// 原子の ID、結合、二次構造、選択は変わらない。
    pub(super) fn apply_trajectory_frame(&mut self, frame: usize) -> Result<(), String> {
        let (traj, _) = match &self.trajectory {
            Some(t) => (t.traj.clone(), t.current),
            None => return Err("No trajectory is open".to_string()),
        };
        let data = traj.read_frame(frame)?;
        self.set_atom_positions(&data.positions)?;
        if let Some(t) = &mut self.trajectory {
            t.current = frame;
            t.step = data.step;
        }
        Ok(())
    }

    /// 原子の座標を、構造の木と平坦化キャッシュの両方で差し替える
    fn set_atom_positions(&mut self, positions: &[[f32; 3]]) -> Result<(), String> {
        let ag = self
            .state
            .atom_group
            .as_mut()
            .ok_or_else(|| "No structure is loaded".to_string())?;
        yui_core::atom_group::set_flat_positions(ag.as_inner_mut(), positions)?;
        for (atom, p) in self.state.molecule.atoms.iter_mut().zip(positions) {
            atom.position = *p;
        }
        self.update_resolved_interactions();
        Ok(())
    }

    fn restore_original_positions(&mut self) {
        if let Some(t) = &self.trajectory {
            let original = t.original.clone();
            let _ = self.set_atom_positions(&original);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Message;
    use crate::file_open_dispatch::FileOpenHandler;
    use crate::menu_bar::MenuAction;
    use crate::playback::PlaybackAction as P;
    use iced::time::Instant;
    use std::time::Duration;
    use yui_core::atom_group::iter_flat_atoms;

    /// `core/tests/data/multiframe.xyz`（C, O, N の 3 原子、5 フレーム）と原子の並びが合う PDB
    const THREE_ATOM_PDB: &str = "\
ATOM      1  C   ALA A   1       0.000   0.000   0.000  1.00  0.00           C
ATOM      2  O   ALA A   1       1.500   0.000   0.000  1.00  0.00           O
ATOM      3  N   ALA A   1       0.000   1.200   0.000  1.00  0.00           N
END
";

    fn app_with_structure() -> Yui {
        let mut app = Yui::default();
        let _ = app.update(Message::FileContentFetched {
            path: "three.pdb".to_string(),
            handler: FileOpenHandler::StructureViewer,
            result: Ok(THREE_ATOM_PDB.as_bytes().to_vec()),
        });
        assert_eq!(
            app.state.molecule.atoms.len(),
            3,
            "the test structure loads"
        );
        app
    }

    fn multiframe() -> Arc<XyzTrajectory> {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../core/tests/data/multiframe.xyz"
        );
        Arc::new(XyzTrajectory::from_bytes("multiframe.xyz", std::fs::read(path).unwrap()).unwrap())
    }

    fn open(app: &mut Yui, traj: Arc<XyzTrajectory>) {
        let _ = app.update(Message::TrajectoryOpened(Ok(Some(traj))));
    }

    fn mol_x(app: &Yui) -> Vec<f32> {
        app.state
            .molecule
            .atoms
            .iter()
            .map(|a| a.position[0])
            .collect()
    }

    fn tree_x(app: &Yui) -> Vec<f32> {
        let ag = app.state.atom_group.as_ref().unwrap();
        iter_flat_atoms(ag.as_inner())
            .map(|fa| fa.atom.xyz.x as f32)
            .collect()
    }

    fn act(app: &mut Yui, action: P) {
        let _ = app.update(Message::PlaybackControl(action));
    }

    #[test]
    fn test_opening_a_trajectory_makes_its_frames_the_playback_frames() {
        let mut app = app_with_structure();
        assert!(
            !app.has_playback(),
            "a single-model structure has no frames"
        );
        open(&mut app, multiframe());
        assert!(app.trajectory.is_some());
        assert_eq!(app.frame_count(), 5);
        assert_eq!(app.current_frame(), Some(0));
        assert!(app.has_playback());
        assert!(app.view_playback_bar().is_some());
    }

    #[test]
    fn test_stepping_frames_moves_the_atoms_in_the_molecule_and_the_tree() {
        let mut app = app_with_structure();
        open(&mut app, multiframe());
        assert_eq!(mol_x(&app), vec![0.0, 1.5, 0.0]);

        act(&mut app, P::Next);
        assert_eq!(app.current_frame(), Some(1));
        assert_eq!(mol_x(&app), vec![0.5, 2.0, 0.5]);
        assert_eq!(
            tree_x(&app),
            vec![0.5, 2.0, 0.5],
            "the structure tree follows too"
        );

        act(&mut app, P::Last);
        assert_eq!(mol_x(&app), vec![2.0, 3.5, 2.0]);
        act(&mut app, P::SetFrame(2));
        act(&mut app, P::ScrubEnd);
        assert_eq!(mol_x(&app), vec![1.0, 2.5, 1.0]);
        assert_eq!(app.trajectory.as_ref().unwrap().step, 20);
        act(&mut app, P::First);
        assert_eq!(mol_x(&app), vec![0.0, 1.5, 0.0]);
    }

    #[test]
    fn test_ids_bonds_and_selection_survive_a_frame_change() {
        let mut app = app_with_structure();
        let ids: Vec<u32> = app.state.molecule.atoms.iter().map(|a| a.id).collect();
        let bonds = app.state.molecule.bonds.clone();
        app.selection.insert(2);
        open(&mut app, multiframe());
        act(&mut app, P::Next);
        act(&mut app, P::Last);
        assert_eq!(
            app.state
                .molecule
                .atoms
                .iter()
                .map(|a| a.id)
                .collect::<Vec<_>>(),
            ids
        );
        assert_eq!(app.state.molecule.bonds, bonds);
        assert!(
            app.selection.contains(&2),
            "atom IDs are the same in every frame"
        );
    }

    #[test]
    fn test_playback_advances_the_trajectory_by_time() {
        let mut app = app_with_structure();
        open(&mut app, multiframe());
        act(&mut app, P::SetFps(10));
        act(&mut app, P::PlayPause);
        assert!(app.playback.playing);
        let t0 = Instant::now();
        app.playback.start(t0);
        let _ = app.handle_playback_tick(t0 + Duration::from_millis(300));
        assert_eq!(app.current_frame(), Some(3));
        assert_eq!(mol_x(&app), vec![1.5, 3.0, 1.5]);
    }

    #[test]
    fn test_loop_modes_apply_to_trajectory_frames() {
        let mut app = app_with_structure();
        open(&mut app, multiframe());
        act(&mut app, P::SetLoopMode(crate::playback::LoopMode::Once));
        act(&mut app, P::SetFps(10));
        act(&mut app, P::Last);
        act(&mut app, P::PlayPause); // 末尾から、「1回」なので先頭からやり直す
        assert_eq!(app.current_frame(), Some(0));
        let t0 = Instant::now();
        app.playback.start(t0);
        let _ = app.handle_playback_tick(t0 + Duration::from_secs(5));
        assert_eq!(app.current_frame(), Some(4));
        assert!(!app.playback.playing, "Once stops at the last frame");
    }

    #[test]
    fn test_atom_count_mismatch_is_an_error_and_changes_nothing() {
        let mut app = app_with_structure();
        let two = "2\nc\nC 0 0 0\nO 1 0 0\n";
        let traj = Arc::new(XyzTrajectory::from_bytes("two.xyz", two.as_bytes().to_vec()).unwrap());
        open(&mut app, traj);
        assert!(app.trajectory.is_none());
        let msg = app.status_message.as_ref().unwrap();
        assert_eq!(msg.level, crate::status_bar::StatusLevel::Error);
        assert!(
            msg.text.contains("2 atoms") && msg.text.contains("3"),
            "{}",
            msg.text
        );
        assert_eq!(mol_x(&app), vec![0.0, 1.5, 0.0]);
    }

    #[test]
    fn test_a_trajectory_needs_a_structure() {
        let mut app = Yui::default();
        open(&mut app, multiframe());
        assert!(app.trajectory.is_none());
        assert_eq!(
            app.status_message.as_ref().unwrap().level,
            crate::status_bar::StatusLevel::Warning
        );
    }

    #[test]
    fn test_a_bad_file_reports_the_error() {
        let mut app = app_with_structure();
        let _ = app.update(Message::TrajectoryOpened(Err("bad".to_string())));
        assert!(app.status_message.as_ref().unwrap().text.contains("bad"));
        let _ = app.update(Message::TrajectoryOpened(Ok(None)));
        assert!(app.trajectory.is_none());
    }

    #[test]
    fn test_closing_restores_the_original_coordinates() {
        let mut app = app_with_structure();
        open(&mut app, multiframe());
        act(&mut app, P::Last);
        assert_ne!(mol_x(&app), vec![0.0, 1.5, 0.0]);
        let _ = app.update(Message::MenuAction(MenuAction::FileCloseTrajectory));
        assert!(app.trajectory.is_none());
        assert_eq!(mol_x(&app), vec![0.0, 1.5, 0.0]);
        assert_eq!(tree_x(&app), vec![0.0, 1.5, 0.0]);
        assert!(!app.has_playback());
    }

    #[test]
    fn test_opening_a_second_trajectory_restores_then_replaces() {
        let mut app = app_with_structure();
        open(&mut app, multiframe());
        act(&mut app, P::Last);
        open(&mut app, multiframe());
        assert_eq!(app.current_frame(), Some(0));
        // 元の座標を覚え直しているので、閉じると、最初の構造の座標に戻る
        let _ = app.update(Message::MenuAction(MenuAction::FileCloseTrajectory));
        assert_eq!(mol_x(&app), vec![0.0, 1.5, 0.0]);
    }

    #[test]
    fn test_replacing_the_structure_drops_the_trajectory_and_stops_playback() {
        let mut app = app_with_structure();
        open(&mut app, multiframe());
        act(&mut app, P::PlayPause);
        let _ = app.update(Message::MenuAction(MenuAction::FileNew));
        assert!(app.trajectory.is_none());
        assert!(!app.playback.playing);
    }

    #[test]
    fn test_surface_follows_while_playing_a_trajectory_and_is_rebuilt_on_pause() {
        let mut app = app_with_structure();
        app.state
            .representations
            .push(yui_core::representation::Representation::new(
                "surface_test",
                yui_core::selector::Selector::All,
                yui_core::representation::RepresentationKind::Surface,
                yui_core::representation::ColorScheme::Element,
            ));
        open(&mut app, multiframe());
        app.surface_generating = false;
        act(&mut app, P::SetFps(10));
        act(&mut app, P::PlayPause);
        let t0 = Instant::now();
        app.playback.start(t0);
        let _ = app.handle_playback_tick(t0 + Duration::from_millis(200));
        // ネイティブ版は追従して生成を始める。ブラウザ版は再生中は作らない
        assert_eq!(app.surface_generating, cfg!(not(target_arch = "wasm32")));
        app.surface_generating = false;
        act(&mut app, P::PlayPause);
        assert!(
            app.surface_generating,
            "rebuild on pause, with the shown frame"
        );
    }

    #[test]
    fn test_a_frame_that_cannot_be_read_stops_playback_with_an_error() {
        let dir = std::env::temp_dir().join("yui_traj_ops_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("gone.xyz");
        std::fs::copy(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../core/tests/data/multiframe.xyz"
            ),
            &path,
        )
        .unwrap();
        let traj = Arc::new(XyzTrajectory::from_file(&path).unwrap());
        let mut app = app_with_structure();
        open(&mut app, traj);
        act(&mut app, P::PlayPause);
        assert!(app.playback.playing);
        std::fs::remove_file(&path).unwrap();
        let t0 = Instant::now();
        app.playback.start(t0);
        let _ = app.handle_playback_tick(t0 + Duration::from_millis(400));
        assert!(!app.playback.playing, "stopped");
        assert_eq!(
            app.status_message.as_ref().unwrap().level,
            crate::status_bar::StatusLevel::Error
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_trajectory_is_not_part_of_the_workspace() {
        use crate::pane_view::export_workspace_to_yaml_str;
        let mut app = app_with_structure();
        let before = serde_yaml::to_string(&app.state.trajectory).unwrap();
        open(&mut app, multiframe());
        act(&mut app, P::PlayPause);
        // AppState.trajectory には触らない（再生は利用者ごと）
        assert_eq!(
            serde_yaml::to_string(&app.state.trajectory).unwrap(),
            before
        );
        let _ = export_workspace_to_yaml_str(&app.state, &app.pane_views).unwrap();
    }
}
