use iced::Task;
use iced::widget::pane_grid;

use crate::context_menu::{ContextMenuAction, ContextMenuTarget};
use crate::file_open_dispatch::{self, FileOpenHandler};
use crate::menu_bar::{self, MenuAction};
use crate::named_selections::{NamedSelectionAction, generate_unique_selection_name};
use crate::pane_view::default_pane_views;
use crate::project_tree_pane;
use crate::representation_inspector::{
    self, RepContextMenuAction, RepContextTarget, RepresentationAction,
};
use crate::state_sync;
use crate::status_bar::StatusMessage;
use crate::structure_tree::{StructureTreeAction, TreeContextMenuAction};
use crate::workspace_file_io::{self, WorkspaceSaveMode};
use yui_core::representation::{ColorScheme, Representation, RepresentationKind};
use yui_core::selector::Selector;

use super::{Message, PaneKind, Yui};

impl Yui {
    fn spawn_file_fetch_task(&self, path: String, handler: FileOpenHandler) -> Task<Message> {
        match self.project_tree.mode {
            project_tree_pane::TreeMode::Backend => {
                let base_url = self
                    .project_tree
                    .base_url
                    .as_deref()
                    .unwrap_or("http://127.0.0.1:8080");
                let url = state_sync::derive_http_fs_read_url(base_url, &path);
                let path_clone = path.clone();
                Task::perform(
                    file_open_dispatch::fetch_file_bytes_backend(url),
                    move |result| Message::FileContentFetched {
                        path: path_clone,
                        handler,
                        result,
                    },
                )
            }
            #[cfg(not(target_arch = "wasm32"))]
            project_tree_pane::TreeMode::LocalNative => {
                if let Some(ref root) = self.project_tree.local_root {
                    let full_path = root.join(&path);
                    let path_clone = path.clone();
                    Task::perform(
                        file_open_dispatch::fetch_file_bytes_local(full_path),
                        move |result| Message::FileContentFetched {
                            path: path_clone,
                            handler,
                            result,
                        },
                    )
                } else {
                    let path_clone = path.clone();
                    Task::perform(
                        async { Err("No local folder opened".to_string()) },
                        move |result| Message::FileContentFetched {
                            path: path_clone,
                            handler,
                            result,
                        },
                    )
                }
            }
            _ => {
                let path_clone = path.clone();
                Task::perform(
                    async { Err("Filesystem is unavailable".to_string()) },
                    move |result| Message::FileContentFetched {
                        path: path_clone,
                        handler,
                        result,
                    },
                )
            }
        }
    }
    pub fn update(&mut self, message: Message) -> Task<Message> {
        let mut task = Task::none();
        match message {
            Message::PaneResized(pane_grid::ResizeEvent { split, ratio }) => {
                self.panes.resize(split, ratio);
            }
            Message::PaneDragged(pane_grid::DragEvent::Dropped { pane, target }) => {
                self.panes.drop(pane, target);
            }
            Message::PaneDragged(_) => {}
            Message::RunCommand(invocation) => {
                // メニューの項目を選んだら、メニューを閉じる（`MenuAction` 以外のコマンドも）
                self.active_menu = None;
                match self.commands.resolve(&invocation) {
                    Ok(message) => return self.update(message),
                    Err(e) => {
                        // 未登録のコマンドや引数の誤りでは、何もせず、警告を出す（アプリを止めない）
                        eprintln!("Warning: {e}");
                        self.status_message =
                            Some(StatusMessage::warning(format!("Command failed: {e}")));
                    }
                }
            }
            Message::PluginCommand {
                plugin,
                id,
                payload,
            } => {
                #[cfg(not(target_arch = "wasm32"))]
                return crate::plugin_menu::run(plugin, id, payload);
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = (plugin, id, payload);
                }
            }
            Message::PluginCommandDone { plugin, result } => {
                if let Err(reason) = result {
                    // 常駐のプラグインは、失敗したら外す（項目が消え、押せなくなる）。アプリは続ける。
                    // 都度呼び出しのプラグインは、その呼び出しが失敗しただけなので、外さない。
                    #[cfg(not(target_arch = "wasm32"))]
                    let oneshot = crate::plugins::is_oneshot(&plugin);
                    #[cfg(target_arch = "wasm32")]
                    let oneshot = false;
                    if oneshot {
                        self.status_message = Some(StatusMessage::warning(format!(
                            "Plugin {plugin}: the command failed: {reason}"
                        )));
                    } else {
                        #[cfg(not(target_arch = "wasm32"))]
                        crate::plugin_menu::mark_failed(&plugin, &reason);
                        self.status_message = Some(StatusMessage::warning(format!(
                            "Plugin {plugin} failed and was disabled: {reason}"
                        )));
                    }
                }
            }
            Message::PluginsDialogOpen => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    self.plugin_rows = crate::plugin_list::rows(
                        crate::plugins::plugins_dir().as_deref(),
                        &crate::plugins::configured(),
                    );
                    self.plugin_notice = None;
                    self.show_plugins = true;
                }
            }
            Message::PluginsDialogClose => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    self.show_plugins = false;
                }
            }
            Message::PluginToggled(name) => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    // 設定に書く（次回の起動から効く）。書けなければ、理由を一覧の下に出す。
                    let names = crate::plugin_list::toggled_names(&self.plugin_rows, &name);
                    self.plugin_notice =
                        Some(match crate::user_settings::save_plugins_enabled(&names) {
                            Ok(()) => {
                                crate::plugins::set_configured(&names);
                                "Saved. Restart YUI to apply the change.".to_string()
                            }
                            Err(e) => format!("Could not save: {e}"),
                        });
                    self.plugin_rows = crate::plugin_list::rows(
                        crate::plugins::plugins_dir().as_deref(),
                        &crate::plugins::configured(),
                    );
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = name;
                }
            }
            Message::ViewportCameraChanged { id, camera, fitted } => {
                self.handle_camera_changed(id, camera, fitted);
            }
            Message::InitialStateFetched(Ok(remote_state)) => {
                self.rest_error = None;
                state_sync::merge_initial_state(&mut self.state, remote_state);
                self.update_resolved_interactions();
                self.update_model_atom_ranges();
                self.apply_loaded_camera();
                task = self.trigger_surface_rebuild();
            }
            Message::InitialStateFetched(Err(err)) => {
                eprintln!("Warning: Failed to fetch initial state via REST: {err}");
                self.rest_error = Some(err);
            }
            Message::CollabConnected => {
                self.collab_connected = true;
            }
            Message::CollabDisconnected => {
                self.collab_connected = false;
            }
            Message::CollabDeltaReceived(delta) => {
                self.collab_connected = true;
                let sig_before = crate::surface_render::extract_surface_geometry_signature(
                    &self.state.representations,
                );
                yui_core::collab::apply_app_state_delta(&mut self.state, &delta);
                let sig_after = crate::surface_render::extract_surface_geometry_signature(
                    &self.state.representations,
                );
                if sig_before != sig_after {
                    task = self.trigger_surface_rebuild();
                } else {
                    self.update_surface_colors();
                }
            }
            Message::PaneTabSelected { pane, view } => {
                if let Some(state) = self.pane_views.get_mut(&pane)
                    && state.select_view(view)
                {
                    self.tab_drag = Some((pane, view));
                    self.tab_drag_start = self.cursor_position;
                }
            }
            Message::PaneTabHovered { pane, view } => {
                // 同じペインなら並べ替え、ほかのエディタのグループなら、そのタブの位置へ移す
                // （ドックのタブは、ほかのペインへは移さない）
                if let Some((drag_pane, dragged)) = self.tab_drag {
                    if drag_pane == pane {
                        if let Some(state) = self.pane_views.get_mut(&pane) {
                            state.move_view_to(dragged, view);
                        }
                    } else if self.pane_views.move_view_between(
                        dragged,
                        drag_pane,
                        pane,
                        Some(view),
                    ) {
                        self.tab_drag = Some((pane, dragged));
                    }
                }
            }
            Message::TabDropTargetEntered(pane, zone) => {
                if self.tab_drag.is_some() {
                    self.tab_drop_target = Some((pane, zone));
                }
            }
            Message::TabDropTargetLeft(pane, zone) => {
                if self.tab_drop_target == Some((pane, zone)) {
                    self.tab_drop_target = None;
                }
            }
            Message::TabDragEnded => self.end_tab_drag(),
            Message::TabBarWheel { pane, dx } => {
                task = iced::widget::operation::scroll_by(
                    crate::pane_view::tab_bar_scroll_id(pane),
                    iced::widget::scrollable::AbsoluteOffset { x: dx, y: 0.0 },
                );
            }
            Message::LogViewerMsg(msg) => {
                self.log_viewer.handle_message(msg);
            }
            Message::MousePressed => self.terminal_pane.mouse_pressed_somewhere(),
            Message::TerminalKey {
                key,
                modifiers,
                text,
            } => {
                if self.terminal_pane.is_composing() {
                    // 日本語入力で変換中: キーは IME に任せる（確定は `TerminalPaneMessage::Ime` で届く）
                } else if let Some(action) = crate::terminal_pane::clipboard_key(&key, modifiers) {
                    task = self.terminal_clipboard(action);
                } else if let Some(dir) = crate::terminal_pane::scroll_key(&key, modifiers) {
                    let page = i32::from(self.terminal_pane.rows.saturating_sub(1).max(1));
                    self.terminal_pane.scroll_back(dir * page);
                } else {
                    let app_cursor = self.terminal_pane.parser.screen().application_cursor();
                    if let Some(bytes) = crate::terminal_pane::key_bytes(
                        &key,
                        modifiers,
                        text.as_deref(),
                        app_cursor,
                    ) {
                        self.terminal_pane.enqueue_input(bytes);
                    }
                }
            }
            Message::TerminalPaneMsg(msg) => {
                use crate::terminal_pane::{ClipboardKey, TerminalPaneMessage as T};
                match msg {
                    T::CopyPressed => task = self.terminal_clipboard(ClipboardKey::Copy),
                    T::PastePressed => task = self.terminal_clipboard(ClipboardKey::Paste),
                    msg => self.terminal_pane.update(msg),
                }
            }
            Message::TerminalPasteText(text) => {
                if let Some(text) = text {
                    self.terminal_pane.paste(&text);
                }
            }
            Message::Editor(msg) => {
                task = self.handle_editor_message(msg);
            }
            Message::TextFileOpened(result) => {
                self.handle_text_file_opened(result);
            }
            Message::CloseTabRequested(id) => {
                self.request_close_document(id);
            }
            Message::CloseDialogChoice(choice) => {
                task = self.handle_close_choice(choice);
            }
            Message::SaveActiveDocument => {
                task = self.save_active_document();
            }
            Message::QuitRequested => {
                task = self.request_quit();
            }
            Message::QuitDialogChoice(choice) => {
                task = self.handle_quit_choice(choice);
            }
            Message::ProjectTreePaneMsg(msg) => {
                task = self.handle_project_tree_pane_msg(msg);
            }
            Message::OpenFileWith { path, handler } => {
                self.file_context_menu = None;
                task = self.spawn_file_fetch_task(path, handler);
            }
            Message::FileContextMenuClosed => {
                self.file_context_menu = None;
            }
            Message::OpenHandlerCancelled => {
                self.pending_open_choice = None;
            }
            Message::OpenHandlerChosen(handler) => {
                if let Some((path, _)) = self.pending_open_choice.take() {
                    task = self.spawn_file_fetch_task(path, handler);
                }
            }
            Message::FileContentFetched {
                path,
                handler,
                result,
            } => {
                task = self.handle_file_content_fetched(path, handler, result);
            }
            Message::MenuAction(action) => {
                task = self.handle_menu_action(action);
            }
            Message::ViewportRightClicked { screen_pos, target } => {
                self.handle_viewport_right_clicked(screen_pos, target);
            }
            Message::SurfaceGenerated {
                generation,
                results,
            } => {
                let latest = self.surface_generation_tracker.is_latest(generation);
                self.apply_surface_build_results(generation, results);
                // 再生中に先へ進んでいたら、最新のモデルでもう一度作る
                if latest && self.surface_follow_pending {
                    task = self.trigger_surface_rebuild();
                }
            }
            Message::ContextMenuAction(action) => {
                let sig_before = crate::surface_render::extract_surface_geometry_signature(
                    &self.state.representations,
                );
                self.handle_context_menu_action(action);
                let sig_after = crate::surface_render::extract_surface_geometry_signature(
                    &self.state.representations,
                );
                if sig_before != sig_after {
                    task = self.trigger_surface_rebuild();
                } else {
                    self.update_surface_colors();
                }
            }
            Message::CursorMoved(pos) => {
                self.cursor_position = pos;
            }
            Message::ViewportClicked(click) => {
                self.handle_viewport_clicked(click);
            }
            Message::PlotClicked(kind, click) => {
                self.handle_plot_clicked(kind, &click);
            }
            Message::ClearSelection => {
                self.clear_selection();
            }
            Message::PytoolsPlotOpened(result) => {
                self.handle_pytools_plot_opened(result);
            }
            Message::TrajectoryOpened(result) => {
                task = self.handle_trajectory_opened(result);
            }
            Message::MovieDialog(action) => {
                task = self.handle_movie_dialog(action);
            }
            #[cfg(not(target_arch = "wasm32"))]
            Message::MovieFolderChosen(result) => {
                task = self.handle_movie_folder_chosen(result);
            }
            Message::MovieRendererReady(result) => {
                task = self.handle_movie_renderer_ready(result);
            }
            Message::MovieFrameDone(result) => {
                task = self.handle_movie_frame_done(result);
            }
            #[cfg(not(target_arch = "wasm32"))]
            Message::MovieEncoded(result) => {
                task = self.handle_movie_encoded(result);
            }
            #[cfg(target_arch = "wasm32")]
            Message::MovieZipSaved(result) => {
                task = self.handle_movie_zip_saved(result);
            }
            Message::MovieCancel => {
                task = self.handle_movie_cancel();
            }
            Message::Share(action) => {
                task = self.handle_share_action(action, false);
            }
            Message::SvgDialog(action) => {
                task = self.handle_svg_dialog(action);
            }
            Message::PngDialog(action) => {
                task = self.handle_png_dialog(action);
            }
            Message::CubeOpened(result) => {
                task = self.handle_cube_opened(result);
            }
            Message::CubeAction(action) => {
                task = self.handle_cube_action(action);
            }
            Message::CubeJobDone(result) => {
                task = self.handle_cube_job_done(result);
            }
            Message::CameraAction(action) => {
                self.handle_camera_action(action);
            }
            Message::CameraAnimTick => {
                self.handle_camera_anim_tick(iced::time::Instant::now());
            }
            Message::PlaybackControl(action) => {
                task = self.handle_playback_action(action);
            }
            Message::PlaybackTick => {
                task = self.handle_playback_tick(iced::time::Instant::now());
            }
            Message::SaveSelectionAsNamed => {
                self.save_selection_as_named();
            }
            Message::WindowResized(size) => {
                self.window_size = size;
            }
            Message::StructureTreeAction(action) => {
                let sig_before = crate::surface_render::extract_surface_geometry_signature(
                    &self.state.representations,
                );
                let model_before = self.state.active_model.clone();
                // 再生中にモデルを手で選んだら、再生を止める（選んだモデルを表示する）
                let was_playing = self.playback.playing;
                if was_playing && matches!(action, StructureTreeAction::SelectModel(_)) {
                    self.reset_playback();
                }
                let stopped_playback = was_playing && !self.playback.playing;
                self.handle_structure_tree_action(action);
                let sig_after = crate::surface_render::extract_surface_geometry_signature(
                    &self.state.representations,
                );
                let model_after = self.state.active_model.clone();
                if model_before != model_after {
                    // 表示するモデルが変わると、選んでいた原子（モデルごとに ID が違う）は意味を失う
                    self.selection.clear();
                }
                if sig_before != sig_after || model_before != model_after || stopped_playback {
                    task = self.trigger_surface_rebuild();
                } else {
                    self.update_surface_colors();
                }
            }
            Message::TreeContextMenuAction(action) => {
                let sig_before = crate::surface_render::extract_surface_geometry_signature(
                    &self.state.representations,
                );
                self.handle_tree_context_menu_action(action);
                let sig_after = crate::surface_render::extract_surface_geometry_signature(
                    &self.state.representations,
                );
                if sig_before != sig_after {
                    task = self.trigger_surface_rebuild();
                } else {
                    self.update_surface_colors();
                }
            }
            Message::RepresentationAction(action) => {
                let sig_before = crate::surface_render::extract_surface_geometry_signature(
                    &self.state.representations,
                );
                self.handle_representation_action(action);
                let sig_after = crate::surface_render::extract_surface_geometry_signature(
                    &self.state.representations,
                );
                if sig_before != sig_after {
                    task = self.trigger_surface_rebuild();
                } else {
                    self.update_surface_colors();
                }
            }
            Message::RepContextMenuAction(action) => {
                let sig_before = crate::surface_render::extract_surface_geometry_signature(
                    &self.state.representations,
                );
                self.handle_rep_context_menu_action(action);
                let sig_after = crate::surface_render::extract_surface_geometry_signature(
                    &self.state.representations,
                );
                if sig_before != sig_after {
                    task = self.trigger_surface_rebuild();
                } else {
                    self.update_surface_colors();
                }
            }
            Message::NamedSelectionAction(action) => {
                let sig_before = crate::surface_render::extract_surface_geometry_signature(
                    &self.state.representations,
                );
                self.handle_named_selection_action(action);
                let sig_after = crate::surface_render::extract_surface_geometry_signature(
                    &self.state.representations,
                );
                if sig_before != sig_after {
                    task = self.trigger_surface_rebuild();
                } else {
                    self.update_surface_colors();
                }
            }
            Message::WorkspaceFileOpened(Ok(Some((file_name, path, yaml_content)))) => {
                match workspace_file_io::apply_loaded_workspace_yaml(&yaml_content) {
                    Ok(loaded) => {
                        self.state = loaded.state;
                        self.set_pane_views(loaded.pane_views);
                        self.current_workspace_file_name = Some(file_name.clone());
                        self.current_workspace_path = path;
                        self.update_resolved_interactions();
                        self.update_model_atom_ranges();
                        self.apply_loaded_camera();
                        self.status_message = Some(StatusMessage::info(format!(
                            "Loaded workspace from '{file_name}'"
                        )));
                        task = Task::batch([
                            self.trigger_surface_rebuild(),
                            self.sync_cube_with_workspace(),
                        ]);
                    }
                    Err(e) => {
                        self.status_message = Some(StatusMessage::error(format!(
                            "Failed to parse workspace: {e}"
                        )));
                    }
                }
            }
            Message::WorkspaceFileOpened(Ok(None)) => {
                // ユーザーによるキャンセル: 何もしない（状態を変えない）
            }
            Message::WorkspaceFileOpened(Err(e)) => {
                self.status_message = Some(StatusMessage::error(format!("Error: {e}")));
            }
            Message::WorkspaceFileSaved(Ok(Some((file_name, path)))) => {
                self.current_workspace_file_name = Some(file_name.clone());
                self.current_workspace_path = path;
                self.status_message = Some(StatusMessage::info(format!(
                    "Saved workspace to '{file_name}'"
                )));
            }
            Message::WorkspaceFileSaved(Ok(None)) => {
                // ユーザーによるキャンセル: 何もしない
            }
            Message::WorkspaceFileSaved(Err(e)) => {
                self.status_message = Some(StatusMessage::error(format!("Error: {e}")));
            }
            Message::StructureExported(Ok(Some((file_name, path)))) => {
                let msg = match path {
                    Some(p) => format!("Exported structure to '{}'", p.display()),
                    None => format!("Exported structure to '{file_name}'"),
                };
                self.status_message = Some(StatusMessage::info(msg));
            }
            Message::StructureExported(Ok(None)) => {
                // ユーザーによるキャンセル: 何もしない
            }
            Message::StructureExported(Err(e)) => {
                self.status_message = Some(StatusMessage::error(format!("Error: {e}")));
            }
            Message::ImageExported(Ok(Some((file_name, path)))) => {
                let msg = match path {
                    Some(p) => format!("Exported image to '{}'", p.display()),
                    None => format!("Exported image to '{file_name}'"),
                };
                self.status_message = Some(StatusMessage::info(msg));
            }
            Message::ImageExported(Ok(None)) => {
                // ユーザーによるキャンセル。「描画中」などの表示を残さない
                self.status_message = Some(StatusMessage::info("Export cancelled.".to_string()));
            }
            Message::ImageExported(Err(e)) => {
                self.status_message = Some(StatusMessage::error(format!("Error: {e}")));
            }
        }
        task
    }

    fn handle_project_tree_pane_msg(
        &mut self,
        msg: project_tree_pane::ProjectTreePaneMessage,
    ) -> Task<Message> {
        let mut task = Task::none();
        if let project_tree_pane::ProjectTreePaneMessage::FileSelected(ref path) = msg {
            self.status_message = Some(StatusMessage::info(format!("Selected: {path}")));
            let candidates = file_open_dispatch::candidate_handlers_for_extension(path);
            if candidates.len() > 1 && file_open_dispatch::asks_on_click(path) {
                self.pending_open_choice = Some((path.clone(), candidates));
            } else if let Some(&default) = candidates.first() {
                self.pending_open_choice = None;
                task = self.spawn_file_fetch_task(path.clone(), default);
            }
        }
        // 右クリック: マウスのそばに、開き方の候補をすべて並べたメニューを出す
        if let project_tree_pane::ProjectTreePaneMessage::FileOpenWithRequested(ref path) = msg {
            self.active_menu = None;
            self.context_menu = None;
            self.tree_context_menu = None;
            self.rep_context_menu = None;
            self.pending_open_choice = None;
            self.file_context_menu = Some((
                self.cursor_position,
                file_open_dispatch::OpenWithTarget {
                    path: path.clone(),
                    handlers: file_open_dispatch::candidate_handlers_for_extension(path),
                },
            ));
        }
        let tree_task = self
            .project_tree
            .update(msg)
            .map(Message::ProjectTreePaneMsg);
        task = Task::batch([task, tree_task]);
        task
    }

    fn handle_file_content_fetched(
        &mut self,
        path: String,
        handler: FileOpenHandler,
        result: Result<Vec<u8>, String>,
    ) -> Task<Message> {
        match result {
            Err(e) => {
                self.status_message = Some(StatusMessage::error(format!(
                    "Failed to fetch '{path}': {e}"
                )));
            }
            Ok(bytes) => {
                let action = file_open_dispatch::determine_dispatch_action(&path, handler, &bytes);
                match action {
                    file_open_dispatch::DispatchContentAction::TextEditor { filename, content } => {
                        match content {
                            Ok(content_text) => {
                                #[cfg(not(target_arch = "wasm32"))]
                                let native_path =
                                    self.project_tree.local_root.as_ref().map(|r| r.join(&path));
                                #[cfg(target_arch = "wasm32")]
                                let native_path = None;

                                self.open_text_document(filename, native_path, &content_text);
                            }
                            Err(e) => {
                                self.status_message =
                                    Some(StatusMessage::error(format!("Error: {e}")));
                            }
                        }
                    }
                    file_open_dispatch::DispatchContentAction::StructureViewer {
                        filename,
                        bytes,
                    } => {
                        match yui_core::structure_loader::load_structure_into_state(
                            &mut self.state,
                            &bytes,
                            &filename,
                        ) {
                            Ok(()) => {
                                self.update_resolved_interactions();
                                self.update_model_atom_ranges();
                                self.selection.clear();
                                self.request_camera_fit();
                                let atom_count = self.state.molecule.atoms.len();
                                let bond_count = self.state.molecule.bonds.len();
                                self.status_message = Some(StatusMessage::info(format!(
                                    "Loaded structure '{filename}': {atom_count} atoms, {bond_count} bonds"
                                )));
                                return self.trigger_surface_rebuild();
                            }
                            Err(e) => {
                                self.status_message = Some(StatusMessage::error(format!(
                                    "Failed to load structure '{filename}': {e}"
                                )));
                            }
                        }
                    }
                    file_open_dispatch::DispatchContentAction::Trajectory { filename, bytes } => {
                        return Task::perform(
                            crate::trajectory::open_from_bytes(filename, bytes),
                            Message::TrajectoryOpened,
                        );
                    }
                    file_open_dispatch::DispatchContentAction::WorkspaceStateImport {
                        filename,
                        content,
                    } => match content {
                        Ok(yaml_str) => match menu_bar::import_workspace_from_yaml(&yaml_str) {
                            Ok((imported_state, imported_views)) => {
                                self.state = imported_state;
                                self.set_pane_views(imported_views);
                                self.update_resolved_interactions();
                                self.update_model_atom_ranges();
                                self.apply_loaded_camera();
                                self.status_message = Some(StatusMessage::info(format!(
                                    "Imported workspace state from '{filename}'"
                                )));
                                return Task::batch([
                                    self.trigger_surface_rebuild(),
                                    self.sync_cube_with_workspace(),
                                ]);
                            }
                            Err(e) => {
                                self.status_message = Some(StatusMessage::error(format!(
                                    "Failed to parse workspace state: {e}"
                                )));
                            }
                        },
                        Err(e) => {
                            self.status_message = Some(StatusMessage::error(e));
                        }
                    },
                }
            }
        }
        Task::none()
    }

    fn handle_menu_action(&mut self, action: MenuAction) -> Task<Message> {
        match action {
            MenuAction::ToggleCategory(cat) => {
                self.active_menu = if self.active_menu == Some(cat) {
                    None
                } else {
                    Some(cat)
                };
            }
            MenuAction::CloseMenu => {
                self.active_menu = None;
            }
            MenuAction::FileNew => {
                self.active_menu = None;
                self.state = menu_bar::create_empty_state();
                self.selection.clear();
                self.set_pane_views(default_pane_views());
                self.update_resolved_interactions();
                self.update_model_atom_ranges();
                self.request_camera_fit();
                self.status_message = Some(StatusMessage::info(
                    "Created new empty AppState.".to_string(),
                ));
                return Task::batch([
                    self.trigger_surface_rebuild(),
                    self.sync_cube_with_workspace(),
                ]);
            }
            MenuAction::FileOpen => {
                self.active_menu = None;
                return Task::perform(
                    workspace_file_io::open_workspace_file_dialog(),
                    Message::WorkspaceFileOpened,
                );
            }
            MenuAction::FileSave => {
                self.active_menu = None;
                let save_mode = workspace_file_io::determine_workspace_save_mode(
                    false,
                    self.current_workspace_path.as_deref(),
                );
                self.sync_editor_ratios();
                match menu_bar::export_workspace_to_yaml(&self.state, &self.pane_views) {
                    Ok(yaml_content) => match save_mode {
                        WorkspaceSaveMode::Overwrite => {
                            let path = self.current_workspace_path.clone();
                            let name = self.current_workspace_file_name.clone();
                            return Task::perform(
                                workspace_file_io::save_workspace_file(path, name, yaml_content),
                                Message::WorkspaceFileSaved,
                            );
                        }
                        WorkspaceSaveMode::SaveAs => {
                            let name = self.current_workspace_file_name.clone();
                            return Task::perform(
                                workspace_file_io::save_workspace_file_as(name, yaml_content),
                                Message::WorkspaceFileSaved,
                            );
                        }
                    },
                    Err(e) => {
                        self.status_message =
                            Some(StatusMessage::error(format!("Export failed: {e}")));
                    }
                }
            }
            MenuAction::FileSaveAs => {
                self.active_menu = None;
                self.sync_editor_ratios();
                match menu_bar::export_workspace_to_yaml(&self.state, &self.pane_views) {
                    Ok(yaml_content) => {
                        let name = self.current_workspace_file_name.clone();
                        return Task::perform(
                            workspace_file_io::save_workspace_file_as(name, yaml_content),
                            Message::WorkspaceFileSaved,
                        );
                    }
                    Err(e) => {
                        self.status_message =
                            Some(StatusMessage::error(format!("Export failed: {e}")));
                    }
                }
            }
            MenuAction::FileExportStructure => {
                self.active_menu = None;
                let Some(atom_group) = self.state.atom_group.clone() else {
                    self.status_message = Some(StatusMessage::warning(
                        "No structure loaded to export.".to_string(),
                    ));
                    return Task::none();
                };
                let suggested_name = self.current_workspace_file_name.as_ref().map(|name| {
                    let stem = std::path::Path::new(name)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("structure");
                    format!("{stem}.pdb")
                });
                return Task::perform(
                    workspace_file_io::export_structure_file(suggested_name, atom_group),
                    Message::StructureExported,
                );
            }

            MenuAction::FileExportImageSvg => {
                self.active_menu = None;
                if !super::has_structure(&self.state) {
                    self.status_message = Some(StatusMessage::warning(
                        "No structure loaded to export.".to_string(),
                    ));
                    return Task::none();
                }
                // 形と構図を選ぶダイアログを開く（「Export...」で、保存先の選択へ進む）
                self.svg_dialog = true;
            }

            MenuAction::FileExportImagePng => {
                self.active_menu = None;
                if !super::has_structure(&self.state) {
                    self.status_message = Some(StatusMessage::warning(
                        "No structure loaded to export.".to_string(),
                    ));
                    return Task::none();
                }
                // 設定のダイアログを開く（「Export...」で、保存先の選択へ進む）
                self.png_dialog = true;
            }

            MenuAction::FileExportAnimation => {
                self.active_menu = None;
                self.open_movie_dialog();
            }

            MenuAction::FileQuit => {
                self.active_menu = None;
                #[cfg(not(target_arch = "wasm32"))]
                return self.request_quit();
                #[cfg(target_arch = "wasm32")]
                {
                    self.status_message = Some(StatusMessage::warning(
                        "Quit is not available in browser environment.".to_string(),
                    ));
                }
            }
            MenuAction::ViewResetCamera => {
                self.active_menu = None;
                self.request_smooth_camera_fit();
                let text = if super::has_structure(&self.state) {
                    "Camera reset to fit the structure."
                } else {
                    "Camera position reset to default."
                };
                self.status_message = Some(StatusMessage::info(text.to_string()));
            }
            MenuAction::ViewSetBackgroundColor(color) => {
                self.active_menu = None;
                self.state.scene.background_color = color;
            }
            MenuAction::ViewTogglePane(pane) => {
                self.active_menu = None;
                match pane {
                    PaneKind::Left => self.show_left = !self.show_left,
                    PaneKind::Right => self.show_right = !self.show_right,
                    PaneKind::Bottom => self.show_bottom = !self.show_bottom,
                    PaneKind::Group(_) => {}
                }
                self.rebuild_pane_grid();
            }
            MenuAction::ViewOpenPlot(kind) => {
                self.active_menu = None;
                // 下のパネルが隠れていると、開いたタブが見えない
                if !self.show_bottom {
                    self.show_bottom = true;
                    self.rebuild_pane_grid();
                }
                crate::pane_view::open_plot_tab(&mut self.pane_views, kind);
            }
            MenuAction::ViewToggleEditorSplit => {
                self.active_menu = None;
                self.toggle_editor_split();
            }
            MenuAction::ViewSetTheme(kind) => {
                self.active_menu = None;
                self.theme = kind;
                // 選んだテーマを利用者ごとの設定に残す。保存できなくても、このまま使い続ける
                self.status_message = Some(match crate::user_settings::save_theme(kind) {
                    Ok(()) => StatusMessage::info(format!("Theme: {}", kind.label())),
                    Err(e) => StatusMessage::warning(format!(
                        "Theme: {} (could not save the setting: {e})",
                        kind.label()
                    )),
                });
            }
            MenuAction::ViewSetSelectionHighlight(style) => {
                self.active_menu = None;
                self.highlight = style;
                // 選んだ方法を利用者ごとの設定に残す。保存できなくても、このまま使い続ける
                self.status_message = Some(match crate::user_settings::save_highlight(style) {
                    Ok(()) => {
                        StatusMessage::info(format!("Selection highlight: {}", style.label()))
                    }
                    Err(e) => StatusMessage::warning(format!(
                        "Selection highlight: {} (could not save the setting: {e})",
                        style.label()
                    )),
                });
            }
            MenuAction::ViewToggleCameraLink => {
                self.active_menu = None;
                self.toggle_camera_link();
            }
            MenuAction::FileNewTextFile => {
                self.active_menu = None;
                self.new_text_document();
            }
            MenuAction::FileShareSession => {
                self.active_menu = None;
                return self.handle_share_action(crate::share_dialog::ShareAction::Refresh, true);
            }
            MenuAction::FileOpenSettings => {
                self.active_menu = None;
                self.open_settings_file();
            }
            MenuAction::FileOpenTrajectory => {
                self.active_menu = None;
                return Task::perform(
                    crate::trajectory::open_trajectory_dialog(),
                    Message::TrajectoryOpened,
                );
            }
            MenuAction::FileOpenPlot => {
                self.active_menu = None;
                return Task::perform(
                    super::pytools_ops::open_plot_dialog(),
                    Message::PytoolsPlotOpened,
                );
            }
            MenuAction::FileOpenCube => {
                self.active_menu = None;
                return Task::perform(crate::cube_view::open_cube_dialog(), Message::CubeOpened);
            }
            MenuAction::FileCloseCube => {
                self.active_menu = None;
                self.close_cube();
            }
            MenuAction::FileCloseTrajectory => {
                self.active_menu = None;
                return self.close_trajectory();
            }
            MenuAction::FileOpenTextFile => {
                self.active_menu = None;
                return Task::perform(
                    crate::text_editor_pane::open_file_dialog(),
                    Message::TextFileOpened,
                );
            }
            MenuAction::HelpAbout => {
                self.active_menu = None;
                self.show_about = true;
            }
            MenuAction::CloseAbout => {
                self.show_about = false;
            }
            MenuAction::HelpControls => {
                self.active_menu = None;
                self.show_controls = true;
            }
            MenuAction::CloseControls => {
                self.show_controls = false;
            }
        }
        Task::none()
    }

    fn handle_viewport_right_clicked(
        &mut self,
        screen_pos: [f32; 2],
        target: Option<ContextMenuTarget>,
    ) {
        self.active_menu = None;
        self.tree_context_menu = None;
        self.rep_context_menu = None;
        self.file_context_menu = None;
        self.context_menu = target.map(|t| (screen_pos, t));
    }

    fn handle_context_menu_action(&mut self, action: ContextMenuAction) {
        match action {
            ContextMenuAction::SelectAtom(id) => {
                self.context_menu = None;
                self.select_only(&[id as u32]);
            }
            ContextMenuAction::SelectResidue {
                chain_id,
                residue_id,
            } => {
                self.context_menu = None;
                let ids = crate::selection::residue_atom_ids(
                    &self.state.molecule.atoms,
                    &chain_id,
                    residue_id,
                );
                self.select_only(&ids);
            }
            ContextMenuAction::HideAtom(id) => {
                self.context_menu = None;
                let rep_id = format!("hide_atom_{id}");
                let rep = Representation::new(
                    &rep_id,
                    Selector::AtomIds(vec![id as u32]),
                    RepresentationKind::BallAndStick,
                    ColorScheme::Element,
                )
                .with_hides_atoms(true);
                if let Some(pos) = self
                    .state
                    .representations
                    .iter()
                    .position(|r| r.id == rep_id)
                {
                    self.state.representations[pos] = rep;
                } else {
                    self.state.representations.push(rep);
                }
                self.status_message = Some(StatusMessage::info(format!("Hidden atom #{id}")));
            }
            ContextMenuAction::ShowAs { atom_id, kind } => {
                self.context_menu = None;
                let rep_id = format!("rep_atom_kind_{atom_id}");
                let rep = Representation::new(
                    &rep_id,
                    Selector::AtomIds(vec![atom_id as u32]),
                    kind,
                    ColorScheme::Element,
                );
                if let Some(pos) = self
                    .state
                    .representations
                    .iter()
                    .position(|r| r.id == rep_id)
                {
                    self.state.representations[pos] = rep;
                } else {
                    self.state.representations.push(rep);
                }
                self.status_message = Some(StatusMessage::info(format!(
                    "Changed representation of atom #{atom_id} to {kind:?}"
                )));
            }
            ContextMenuAction::ColorBy { atom_id, scheme } => {
                self.context_menu = None;
                let rep_id = format!("rep_atom_color_{atom_id}");
                let rep = Representation::new(
                    &rep_id,
                    Selector::AtomIds(vec![atom_id as u32]),
                    RepresentationKind::BallAndStick,
                    scheme,
                );
                if let Some(pos) = self
                    .state
                    .representations
                    .iter()
                    .position(|r| r.id == rep_id)
                {
                    self.state.representations[pos] = rep;
                } else {
                    self.state.representations.push(rep);
                }
                self.status_message = Some(StatusMessage::info(format!(
                    "Changed color scheme of atom #{atom_id}"
                )));
            }
            ContextMenuAction::SaveAtomAsNamedSelection(atom_id) => {
                self.context_menu = None;
                let selector = Selector::AtomIds(vec![atom_id as u32]);
                let name = generate_unique_selection_name(&self.state.named_selections);
                self.state.named_selections.insert(name.clone(), selector);
                self.status_message = Some(StatusMessage::info(format!(
                    "Saved atom #{atom_id} as named selection '{name}'"
                )));
            }
            ContextMenuAction::SaveResidueAsNamedSelection {
                chain_id,
                residue_id,
            } => {
                self.context_menu = None;
                let selector = Selector::ResidueRange {
                    chain: if chain_id.is_empty() {
                        None
                    } else {
                        Some(chain_id.clone())
                    },
                    start: residue_id,
                    end: residue_id,
                };
                let name = generate_unique_selection_name(&self.state.named_selections);
                self.state.named_selections.insert(name.clone(), selector);
                self.status_message = Some(StatusMessage::info(format!(
                    "Saved residue {residue_id} in chain {chain_id} as named selection '{name}'"
                )));
            }
            ContextMenuAction::Close => {
                self.context_menu = None;
            }
        }
    }

    fn handle_structure_tree_action(&mut self, action: StructureTreeAction) {
        match action {
            StructureTreeAction::ToggleNode(key) => {
                self.structure_tree.toggle_expanded(&key);
            }
            StructureTreeAction::SelectNode {
                key,
                label,
                selector,
            } => {
                self.structure_tree.select(Some(key.clone()));
                self.context_menu = None;
                self.tree_context_menu = None;
                self.rep_context_menu = None;
                self.select_by_selector(&selector);
                self.status_message = Some(StatusMessage::info(format!(
                    "Selected {label} from Structure Tree ({} atom(s))",
                    self.selection.len()
                )));
            }
            StructureTreeAction::OpenContextMenu { target } => {
                self.context_menu = None;
                self.file_context_menu = None;
                self.rep_context_menu = None;
                let pos = [
                    self.cursor_position[0].max(200.0),
                    self.cursor_position[1].max(80.0),
                ];
                self.tree_context_menu = Some((pos, target));
            }
            StructureTreeAction::SelectModel(model) => {
                let label = model.as_deref().unwrap_or("All");
                self.status_message = Some(StatusMessage::info(format!(
                    "Switched display to model '{label}'"
                )));
                self.state.active_model = model;
            }
        }
    }

    fn handle_tree_context_menu_action(&mut self, action: TreeContextMenuAction) {
        if let Some((_pos, target)) = self.tree_context_menu.clone() {
            match action {
                TreeContextMenuAction::Select => {
                    self.tree_context_menu = None;
                    self.structure_tree.select(Some(target.key.clone()));
                    self.select_by_selector(&target.selector);
                    self.status_message = Some(StatusMessage::info(format!(
                        "Selected {} from Structure Tree ({} atom(s))",
                        target.label,
                        self.selection.len()
                    )));
                }
                TreeContextMenuAction::Hide => {
                    self.tree_context_menu = None;
                    let rep_id = format!("tree_hide_{}", target.key);
                    let rep = Representation::new(
                        &rep_id,
                        target.selector,
                        RepresentationKind::BallAndStick,
                        ColorScheme::Element,
                    )
                    .with_hides_atoms(true);
                    if let Some(pos) = self
                        .state
                        .representations
                        .iter()
                        .position(|r| r.id == rep_id)
                    {
                        self.state.representations[pos] = rep;
                    } else {
                        self.state.representations.push(rep);
                    }
                    self.status_message =
                        Some(StatusMessage::info(format!("Hidden {}", target.label)));
                }
                TreeContextMenuAction::ShowAs(kind) => {
                    self.tree_context_menu = None;
                    let rep_id = format!("tree_rep_{}", target.key);
                    let rep =
                        Representation::new(&rep_id, target.selector, kind, ColorScheme::Element);
                    if let Some(pos) = self
                        .state
                        .representations
                        .iter()
                        .position(|r| r.id == rep_id)
                    {
                        self.state.representations[pos] = rep;
                    } else {
                        self.state.representations.push(rep);
                    }
                    self.status_message = Some(StatusMessage::info(format!(
                        "Changed representation of {} to {kind:?}",
                        target.label
                    )));
                }
                TreeContextMenuAction::ColorBy(scheme) => {
                    self.tree_context_menu = None;
                    let rep_id = format!("tree_color_{}", target.key);
                    let rep = Representation::new(
                        &rep_id,
                        target.selector,
                        RepresentationKind::BallAndStick,
                        scheme,
                    );
                    if let Some(pos) = self
                        .state
                        .representations
                        .iter()
                        .position(|r| r.id == rep_id)
                    {
                        self.state.representations[pos] = rep;
                    } else {
                        self.state.representations.push(rep);
                    }
                    self.status_message = Some(StatusMessage::info(format!(
                        "Changed color scheme of {}",
                        target.label
                    )));
                }
                TreeContextMenuAction::SaveAsNamedSelection => {
                    self.tree_context_menu = None;
                    let name = generate_unique_selection_name(&self.state.named_selections);
                    self.state
                        .named_selections
                        .insert(name.clone(), target.selector);
                    self.status_message = Some(StatusMessage::info(format!(
                        "Saved {} as named selection '{name}'",
                        target.label
                    )));
                }
                TreeContextMenuAction::Close => {
                    self.tree_context_menu = None;
                }
            }
        } else {
            self.tree_context_menu = None;
        }
    }

    fn handle_representation_action(&mut self, action: RepresentationAction) {
        match action {
            RepresentationAction::AddRepresentation => {
                self.context_menu = None;
                self.tree_context_menu = None;
                self.rep_context_menu = None;
                let mut next_n = self.state.representations.len() + 1;
                let mut new_id = format!("rep_{next_n}");
                while self.state.representations.iter().any(|r| r.id == new_id) {
                    next_n += 1;
                    new_id = format!("rep_{next_n}");
                }
                let rep = Representation::new(
                    new_id,
                    Selector::All,
                    RepresentationKind::BallAndStick,
                    ColorScheme::Element,
                );
                self.state.representations.push(rep);
                self.status_message =
                    Some(StatusMessage::info("Added new representation".to_string()));
            }
            RepresentationAction::ToggleVisibility(index) => {
                self.context_menu = None;
                self.tree_context_menu = None;
                self.rep_context_menu = None;
                if let Some(rep) = self.state.representations.get_mut(index) {
                    rep.visible = !rep.visible;
                    self.status_message = Some(StatusMessage::info(format!(
                        "Toggled visibility of {} to {}",
                        rep.id, rep.visible
                    )));
                }
            }
            RepresentationAction::ChangeKind { index, kind } => {
                self.context_menu = None;
                self.tree_context_menu = None;
                self.rep_context_menu = None;
                if let Some(rep) = self.state.representations.get_mut(index) {
                    rep.kind = kind;
                    self.status_message = Some(StatusMessage::info(format!(
                        "Changed representation kind of {} to {kind:?}",
                        rep.id
                    )));
                }
            }
            RepresentationAction::ToggleInteractionKind { index, kind } => {
                self.context_menu = None;
                self.tree_context_menu = None;
                self.rep_context_menu = None;
                if let Some(rep) = self.state.representations.get_mut(index) {
                    rep.interaction_kinds = representation_inspector::toggle_interaction_kind(
                        &rep.interaction_kinds,
                        kind,
                    );
                    self.status_message = Some(StatusMessage::info(format!(
                        "Updated interaction filters for {}",
                        rep.id
                    )));
                }
            }
            RepresentationAction::ChangeColor { index, color } => {
                self.context_menu = None;
                self.tree_context_menu = None;
                self.rep_context_menu = None;
                if let Some(rep) = self.state.representations.get_mut(index) {
                    rep.color = color;
                    self.status_message = Some(StatusMessage::info(format!(
                        "Changed color scheme of {}",
                        rep.id
                    )));
                }
            }
            RepresentationAction::ChangeOpacity { index, opacity } => {
                // スライダーを動かすたびに届くので、状態メッセージは出さない
                representation_inspector::set_opacity(
                    &mut self.state.representations,
                    index,
                    opacity,
                );
            }
            RepresentationAction::Delete(index) => {
                self.context_menu = None;
                self.tree_context_menu = None;
                self.rep_context_menu = None;
                if index < self.state.representations.len() {
                    let removed = self.state.representations.remove(index);
                    self.status_message = Some(StatusMessage::info(format!(
                        "Deleted representation {}",
                        removed.id
                    )));
                }
            }
            RepresentationAction::OpenContextMenu {
                index,
                screen_pos: _,
            } => {
                self.context_menu = None;
                self.tree_context_menu = None;
                self.file_context_menu = None;
                if let Some(rep) = self.state.representations.get(index) {
                    let target = RepContextTarget {
                        index,
                        id: rep.id.clone(),
                        kind: rep.kind,
                        visible: rep.visible,
                    };
                    let pos = [
                        self.cursor_position[0].max(100.0),
                        self.cursor_position[1].max(80.0),
                    ];
                    self.rep_context_menu = Some((pos, target));
                }
            }
        }
    }

    fn handle_rep_context_menu_action(&mut self, action: RepContextMenuAction) {
        match action {
            RepContextMenuAction::ToggleVisibility(index) => {
                self.rep_context_menu = None;
                if let Some(rep) = self.state.representations.get_mut(index) {
                    rep.visible = !rep.visible;
                    self.status_message = Some(StatusMessage::info(format!(
                        "Toggled visibility of {} to {}",
                        rep.id, rep.visible
                    )));
                }
            }
            RepContextMenuAction::ShowAs { index, kind } => {
                self.rep_context_menu = None;
                if let Some(rep) = self.state.representations.get_mut(index) {
                    rep.kind = kind;
                    self.status_message = Some(StatusMessage::info(format!(
                        "Changed representation kind of {} to {kind:?}",
                        rep.id
                    )));
                }
            }
            RepContextMenuAction::ColorBy { index, scheme } => {
                self.rep_context_menu = None;
                if let Some(rep) = self.state.representations.get_mut(index) {
                    rep.color = scheme;
                    self.status_message = Some(StatusMessage::info(format!(
                        "Changed color scheme of {}",
                        rep.id
                    )));
                }
            }
            RepContextMenuAction::Delete(index) => {
                self.rep_context_menu = None;
                if index < self.state.representations.len() {
                    let removed = self.state.representations.remove(index);
                    self.status_message = Some(StatusMessage::info(format!(
                        "Deleted representation {}",
                        removed.id
                    )));
                }
            }
            RepContextMenuAction::Close => {
                self.rep_context_menu = None;
            }
        }
    }

    fn handle_named_selection_action(&mut self, action: NamedSelectionAction) {
        match action {
            NamedSelectionAction::RenameDraft { key, draft } => {
                self.renaming_selection = Some((key, draft));
            }
            NamedSelectionAction::RenameCommit { old_name } => {
                let draft_val = self
                    .renaming_selection
                    .as_ref()
                    .filter(|(k, _)| k == &old_name)
                    .map(|(_, d)| d.clone());
                self.renaming_selection = None;

                if let Some(new_name) = draft_val {
                    let trimmed = new_name.trim();
                    if trimmed.is_empty() {
                        self.status_message = Some(StatusMessage::warning(
                            "Selection name cannot be empty".to_string(),
                        ));
                    } else if trimmed == old_name {
                        // 名前変更なし
                    } else if self.state.named_selections.contains_key(trimmed) {
                        self.status_message = Some(StatusMessage::warning(format!(
                            "Selection '{trimmed}' already exists"
                        )));
                    } else if let Some(selector) = self.state.named_selections.remove(&old_name) {
                        self.state
                            .named_selections
                            .insert(trimmed.to_string(), selector);

                        // 指摘3: 既存のハイライトRepresentationがあればIDを追従更新
                        let old_rep_id = format!("named_sel_{old_name}");
                        let new_rep_id = format!("named_sel_{trimmed}");
                        if let Some(rep) = self
                            .state
                            .representations
                            .iter_mut()
                            .find(|r| r.id == old_rep_id)
                        {
                            rep.id = new_rep_id;
                        }
                        self.status_message = Some(StatusMessage::info(format!(
                            "Renamed '{old_name}' to '{trimmed}'"
                        )));
                    }
                }
            }
            NamedSelectionAction::Delete(name) => {
                if self
                    .renaming_selection
                    .as_ref()
                    .map(|(k, _)| k == &name)
                    .unwrap_or(false)
                {
                    self.renaming_selection = None;
                }
                if self.state.named_selections.remove(&name).is_some() {
                    // 指摘3: 削除時にハイライトRepresentationのゴーストを解消
                    let rep_id = format!("named_sel_{name}");
                    self.state.representations.retain(|r| r.id != rep_id);
                    self.status_message = Some(StatusMessage::info(format!(
                        "Deleted named selection '{name}'"
                    )));
                }
            }
            NamedSelectionAction::Apply(name) => {
                if let Some(selector) = self.state.named_selections.get(&name) {
                    let rep_id = format!("named_sel_{name}");
                    let rep = Representation::new(
                        &rep_id,
                        selector.clone(),
                        RepresentationKind::BallAndStick,
                        ColorScheme::Uniform([1.0, 0.85, 0.1, 1.0]),
                    );
                    if let Some(pos) = self
                        .state
                        .representations
                        .iter()
                        .position(|r| r.id == rep_id)
                    {
                        self.state.representations[pos] = rep;
                    } else {
                        self.state.representations.push(rep);
                    }
                    self.status_message = Some(StatusMessage::info(format!(
                        "Applied named selection '{name}' as representation"
                    )));
                }
            }
        }
    }
}
