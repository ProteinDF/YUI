//! テキストファイルのエディタ（ドキュメントのタブ）の操作。
//!
//! 開く・作る・保存・閉じる（未保存の確認つき）と、ペイン構成の差し替えを扱う。
//! タブの並びそのものは `pane_view` の純粋関数が決める。

use std::path::PathBuf;

use iced::Task;

use crate::pane_view::{
    self, active_document, add_document_tab, attach_documents, close_document_tab,
};
use crate::status_bar::StatusMessage;
use crate::text_editor_pane::{
    CloseChoice, DocumentId, EditorMessage, SAVE_CANCELED, save_file, save_file_as,
};

use super::{Message, Yui};

impl Yui {
    /// ペイン構成を差し替える（ワークスペースの読み込み、File → New）。
    ///
    /// 開いているテキストファイルは保存しない（利用者ごとのもの）ので、差し替えたあとで
    /// 分割の状態に合わせてタブを並べ直す。選んでいたタブも、できるだけ選び直す。
    pub(super) fn set_pane_views(&mut self, mut views: crate::pane_view::PaneLayout) {
        let prev_active = active_document(&self.pane_views);
        attach_documents(&mut views, &self.documents.ids(), prev_active);
        self.pane_views = views;
        self.rebuild_pane_grid();
    }

    /// 読み込んだテキストファイルを開く（同じパスがすでに開いていれば、そのタブを選ぶ）。
    pub(super) fn open_text_document(
        &mut self,
        file_name: String,
        path: Option<PathBuf>,
        text: &str,
    ) {
        let (id, created) = self.documents.open(file_name.clone(), path, text);
        add_document_tab(&mut self.pane_views, id, true);
        self.status_message = Some(StatusMessage::info(if created {
            format!("Opened '{file_name}'")
        } else {
            format!("'{file_name}' is already open")
        }));
    }

    /// File → New Text File
    pub(super) fn new_text_document(&mut self) {
        let id = self.documents.new_untitled();
        add_document_tab(&mut self.pane_views, id, true);
        if let Some(doc) = self.documents.get(id) {
            self.status_message = Some(StatusMessage::info(format!("Created '{}'", doc.file_name)));
        }
    }

    /// File → Open Settings File（ネイティブ版）。設定ファイルがなければ、コメントだけの雛形を作ってから、タブで開く。
    pub(super) fn open_settings_file(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        match crate::user_settings::settings_path() {
            Some(path) => self.open_settings_at(path),
            None => {
                self.status_message = Some(StatusMessage::error(
                    "Could not find the settings folder of this OS.".to_string(),
                ));
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.status_message = Some(StatusMessage::warning(
                "The settings file is not available in the browser.".to_string(),
            ));
        }
    }

    /// 指定した場所の設定ファイルをタブで開く（なければ雛形を作る）。設定は起動時に読むので、反映には再起動が要る。
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(super) fn open_settings_at(&mut self, path: PathBuf) {
        let text = crate::user_settings::ensure_default_file(&path)
            .and_then(|()| std::fs::read_to_string(&path));
        match text {
            Ok(text) => {
                let name = crate::user_settings::SETTINGS_FILE_NAME.to_string();
                self.open_text_document(name.clone(), Some(path), &text);
                self.status_message = Some(StatusMessage::info(format!(
                    "Opened '{name}'. Settings are read at startup: restart YUI after saving."
                )));
            }
            Err(e) => {
                self.status_message = Some(StatusMessage::error(format!(
                    "Could not open the settings file '{}': {e}",
                    path.display()
                )));
            }
        }
    }

    /// File → Open Text File... のダイアログの結果
    pub(super) fn handle_text_file_opened(
        &mut self,
        result: Result<Option<(String, Option<PathBuf>, String)>, String>,
    ) {
        match result {
            Ok(Some((name, path, text))) => self.open_text_document(name, path, &text),
            Ok(None) => {} // キャンセル
            Err(e) => self.status_message = Some(StatusMessage::error(format!("Error: {e}"))),
        }
    }

    pub(super) fn handle_editor_message(&mut self, msg: EditorMessage) -> Task<Message> {
        match msg {
            EditorMessage::Edit(id, action) => {
                self.documents.perform(id, action).map(Message::Editor)
            }
            ref m @ (EditorMessage::ViewResized(..)
            | EditorMessage::ContentResized(..)
            | EditorMessage::Scrolled(..)) => self
                .documents
                .handle_scroll(m)
                .map_or_else(Task::none, |t| t.map(Message::Editor)),
            EditorMessage::Save(id) => self.save_document(id, false),
            EditorMessage::SaveAs(id) => self.save_document(id, true),
            EditorMessage::SaveCompleted(id, result) => {
                let close_after = self.close_after_save == Some(id);
                let quitting = self
                    .quit_save_queue
                    .as_ref()
                    .is_some_and(|q| q.first() == Some(&id));
                match result {
                    Ok((name, path)) => {
                        self.documents.mark_saved(id, name.clone(), path);
                        self.status_message = Some(StatusMessage::info(format!("Saved '{name}'")));
                        if close_after {
                            self.close_after_save = None;
                            self.close_document(id);
                        }
                        if quitting {
                            return self.continue_quit_save();
                        }
                    }
                    Err(e) => {
                        // 保存して閉じる・終了する途中で失敗・キャンセルしたら、閉じない・終了しない
                        if close_after {
                            self.close_after_save = None;
                        }
                        if quitting {
                            self.quit_save_queue = None;
                        }
                        if e != SAVE_CANCELED {
                            self.status_message = Some(StatusMessage::error(format!("Error: {e}")));
                        } else if quitting {
                            self.status_message = Some(StatusMessage::warning(
                                "Quit canceled: a file was not saved.".to_string(),
                            ));
                        }
                    }
                }
                Task::none()
            }
        }
    }

    /// 選んでいるテキストファイルを保存する（Ctrl+S / Cmd+S）。
    pub(super) fn save_active_document(&mut self) -> Task<Message> {
        match active_document(&self.pane_views) {
            Some(id) => self.save_document(id, false),
            None => Task::none(),
        }
    }

    fn save_document(&mut self, id: DocumentId, save_as: bool) -> Task<Message> {
        let Some(doc) = self.documents.get(id) else {
            return Task::none();
        };
        let name = Some(doc.file_name.clone());
        let text = doc.content.text();
        let done = move |r| Message::Editor(EditorMessage::SaveCompleted(id, r));
        if save_as {
            Task::perform(save_file_as(name, text), done)
        } else {
            Task::perform(save_file(doc.path.clone(), name, text), done)
        }
    }

    /// タブの閉じるボタン。未保存の変更があれば、確認のダイアログを出す。
    pub(super) fn request_close_document(&mut self, id: DocumentId) {
        match self.documents.get(id) {
            Some(doc) if doc.is_dirty => self.pending_close = Some(id),
            Some(_) => self.close_document(id),
            None => {}
        }
    }

    /// 確認のダイアログの選択
    pub(super) fn handle_close_choice(&mut self, choice: CloseChoice) -> Task<Message> {
        let Some(id) = self.pending_close.take() else {
            return Task::none();
        };
        match choice {
            CloseChoice::Cancel => Task::none(),
            CloseChoice::DontSave => {
                self.close_document(id);
                Task::none()
            }
            CloseChoice::Save => {
                self.close_after_save = Some(id);
                self.save_document(id, false)
            }
        }
    }

    fn close_document(&mut self, id: DocumentId) {
        self.documents.remove(id);
        let before = self.begin_layout_change();
        close_document_tab(&mut self.pane_views, id);
        self.end_layout_change(before);
    }

    /// 未保存のテキストファイルの名前（開いている順）
    pub(super) fn unsaved_file_names(&self) -> Vec<String> {
        self.documents
            .ids()
            .into_iter()
            .filter_map(|id| self.documents.get(id))
            .filter(|d| d.is_dirty)
            .map(|d| d.file_name.clone())
            .collect()
    }

    /// 終了の要求（File → Quit、ウィンドウを閉じる操作）。
    /// 未保存のファイルがなければ終了し、あれば確認のダイアログを出す。
    pub(super) fn request_quit(&mut self) -> Task<Message> {
        if self.unsaved_file_names().is_empty() {
            iced::exit()
        } else {
            self.quit_dialog = true;
            Task::none()
        }
    }

    /// 終了するときの確認の選択。
    /// Save All は、未保存のファイルを順に保存し、すべて保存できたときだけ終了する。
    pub(super) fn handle_quit_choice(&mut self, choice: CloseChoice) -> Task<Message> {
        self.quit_dialog = false;
        match choice {
            CloseChoice::Cancel => Task::none(),
            CloseChoice::DontSave => iced::exit(),
            CloseChoice::Save => {
                let queue: Vec<DocumentId> = self
                    .documents
                    .ids()
                    .into_iter()
                    .filter(|id| self.documents.get(*id).is_some_and(|d| d.is_dirty))
                    .collect();
                self.quit_save_queue = Some(queue);
                self.continue_quit_save()
            }
        }
    }

    /// 終了前の保存を進める。先頭の未保存のドキュメントを保存し、なければ終了する。
    fn continue_quit_save(&mut self) -> Task<Message> {
        while let Some(&first) = self.quit_save_queue.as_ref().and_then(|q| q.first()) {
            if self.documents.get(first).is_some_and(|d| d.is_dirty) {
                return self.save_document(first, false);
            }
            // すでに保存済み（または閉じられた）ものは飛ばす
            if let Some(q) = self.quit_save_queue.as_mut() {
                q.remove(0);
            }
        }
        if self.quit_save_queue.take().is_some() {
            return iced::exit();
        }
        Task::none()
    }

    /// 分割のオン・オフ（View → Split Editor Right）
    pub(super) fn toggle_editor_split(&mut self) {
        let on = !pane_view::is_editor_split(&self.pane_views);
        let before = self.begin_layout_change();
        pane_view::set_editor_split(&mut self.pane_views, on);
        if on {
            self.on_secondary_viewport_shown();
        }
        self.end_layout_change(before);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PaneKind;
    use crate::menu_bar::MenuAction;
    use crate::pane_view::{DropZone, ViewId, is_editor_split};
    use iced::widget::text_editor::{Action, Edit};
    use std::path::PathBuf;

    fn center_views(app: &Yui) -> Vec<ViewId> {
        app.pane_views[&PaneKind::CENTER].views.clone()
    }

    fn right_views(app: &Yui) -> Vec<ViewId> {
        app.pane_views
            .get(&PaneKind::CENTER_RIGHT)
            .map(|s| {
                s.views
                    .iter()
                    .copied()
                    .filter(|v| *v != ViewId::Viewport2)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn open(app: &mut Yui, name: &str, path: Option<&str>, text: &str) -> DocumentId {
        let _ = app.update(Message::TextFileOpened(Ok(Some((
            name.to_string(),
            path.map(PathBuf::from),
            text.to_string(),
        )))));
        app.documents.ids().last().copied().expect("a document")
    }

    fn dirty(app: &mut Yui, id: DocumentId) {
        let _ = app.documents.perform(id, Action::Edit(Edit::Insert('x')));
        assert!(app.documents.get(id).unwrap().is_dirty);
    }

    fn has_pane(app: &Yui, kind: PaneKind) -> bool {
        app.panes.iter().any(|(_, k)| *k == kind)
    }

    #[test]
    fn test_open_adds_tab_and_same_path_selects_existing() {
        let mut app = Yui::default();
        let a = open(&mut app, "a.txt", Some("/w/a.txt"), "A");
        let b = open(&mut app, "b.txt", Some("/w/b.txt"), "B");
        assert_eq!(
            center_views(&app),
            vec![ViewId::Viewport, ViewId::Document(a), ViewId::Document(b)]
        );
        assert_eq!(active_document(&app.pane_views), Some(b));

        // 同じパスをもう一度開くと、タブは増えず、そのタブが選ばれる
        let _ = app.update(Message::TextFileOpened(Ok(Some((
            "a.txt".to_string(),
            Some(PathBuf::from("/w/a.txt")),
            "A".to_string(),
        )))));
        assert_eq!(app.documents.ids().len(), 2);
        assert_eq!(center_views(&app).len(), 3);
        assert_eq!(active_document(&app.pane_views), Some(a));
    }

    #[test]
    fn test_new_text_file_menu_creates_untitled_tabs() {
        let mut app = Yui::default();
        let _ = app.update(Message::MenuAction(MenuAction::FileNewTextFile));
        let _ = app.update(Message::MenuAction(MenuAction::FileNewTextFile));
        let ids = app.documents.ids();
        assert_eq!(ids.len(), 2);
        assert_eq!(app.documents.get(ids[0]).unwrap().file_name, "Untitled-1");
        assert_eq!(app.documents.get(ids[1]).unwrap().file_name, "Untitled-2");
        assert_eq!(active_document(&app.pane_views), Some(ids[1]));
    }

    #[test]
    fn test_split_toggle_keeps_documents_and_panes_follow() {
        let mut app = Yui::default();
        let a = open(&mut app, "a.txt", Some("/w/a.txt"), "line 1\nline 2");
        dirty(&mut app, a);
        let before = app.documents.get(a).unwrap().content.text();

        for expect_split in [true, false] {
            let _ = app.update(Message::MenuAction(MenuAction::ViewToggleEditorSplit));
            assert_eq!(is_editor_split(&app.pane_views), expect_split);
            assert_eq!(has_pane(&app, PaneKind::CENTER_RIGHT), expect_split);
            // 内容・ファイル名・未保存の印が残る
            let doc = app.documents.get(a).unwrap();
            assert_eq!(doc.content.text(), before);
            assert_eq!(doc.file_name, "a.txt");
            assert!(doc.is_dirty);
            // タブは、分割なら右、なしなら中央に、1つだけある
            if expect_split {
                assert_eq!(right_views(&app), vec![ViewId::Document(a)]);
                assert_eq!(center_views(&app), vec![ViewId::Viewport]);
            } else {
                assert!(right_views(&app).is_empty());
                assert_eq!(
                    center_views(&app),
                    vec![ViewId::Viewport, ViewId::Document(a)]
                );
            }
        }
    }

    #[test]
    fn test_split_without_files_and_new_file_goes_right() {
        let mut app = Yui::default();
        let _ = app.update(Message::MenuAction(MenuAction::ViewToggleEditorSplit));
        assert!(is_editor_split(&app.pane_views));
        assert!(right_views(&app).is_empty());
        assert!(has_pane(&app, PaneKind::CENTER_RIGHT));
        // 分割中に開いたファイルは、右側に出る
        let a = open(&mut app, "a.txt", Some("/w/a.txt"), "A");
        assert_eq!(right_views(&app), vec![ViewId::Document(a)]);
        assert_eq!(center_views(&app), vec![ViewId::Viewport]);
    }

    #[test]
    fn test_close_clean_document_removes_tab_and_selects_neighbor() {
        let mut app = Yui::default();
        let a = open(&mut app, "a.txt", Some("/w/a.txt"), "A");
        let b = open(&mut app, "b.txt", Some("/w/b.txt"), "B");
        let _ = app.update(Message::CloseTabRequested(b));
        assert!(app.documents.get(b).is_none());
        assert_eq!(app.pending_close, None);
        assert_eq!(active_document(&app.pane_views), Some(a));
        let _ = app.update(Message::CloseTabRequested(a));
        assert_eq!(center_views(&app), vec![ViewId::Viewport]);
        assert_eq!(
            app.pane_views[&PaneKind::CENTER].active_view(),
            Some(ViewId::Viewport)
        );
    }

    #[test]
    fn test_close_dirty_document_asks_and_respects_choice() {
        let mut app = Yui::default();
        let a = open(&mut app, "a.txt", Some("/w/a.txt"), "A");
        dirty(&mut app, a);

        // 閉じようとすると確認になり、まだ閉じない
        let _ = app.update(Message::CloseTabRequested(a));
        assert_eq!(app.pending_close, Some(a));
        assert!(app.documents.get(a).is_some());

        // Cancel: 閉じない
        let _ = app.update(Message::CloseDialogChoice(CloseChoice::Cancel));
        assert_eq!(app.pending_close, None);
        assert!(app.documents.get(a).is_some());
        assert!(center_views(&app).contains(&ViewId::Document(a)));

        // Don't Save: 保存せずに閉じる
        let _ = app.update(Message::CloseTabRequested(a));
        let _ = app.update(Message::CloseDialogChoice(CloseChoice::DontSave));
        assert!(app.documents.get(a).is_none());
        assert!(!center_views(&app).contains(&ViewId::Document(a)));
    }

    #[test]
    fn test_close_with_save_waits_for_save_and_does_not_close_on_failure() {
        let mut app = Yui::default();
        let a = open(&mut app, "a.txt", Some("/w/a.txt"), "A");
        dirty(&mut app, a);

        // Save を選ぶと、保存が終わるまで閉じない
        let _ = app.update(Message::CloseTabRequested(a));
        let _ = app.update(Message::CloseDialogChoice(CloseChoice::Save));
        assert_eq!(app.close_after_save, Some(a));
        assert!(app.documents.get(a).is_some());

        // 保存のダイアログをキャンセルしたら、閉じない（未保存のまま）
        let _ = app.update(Message::Editor(EditorMessage::SaveCompleted(
            a,
            Err(SAVE_CANCELED.to_string()),
        )));
        assert_eq!(app.close_after_save, None);
        assert!(app.documents.get(a).unwrap().is_dirty);
        assert!(app.documents.get(a).is_some());

        // もう一度 Save を選び、今度は保存できたら、閉じる
        let _ = app.update(Message::CloseTabRequested(a));
        let _ = app.update(Message::CloseDialogChoice(CloseChoice::Save));
        let _ = app.update(Message::Editor(EditorMessage::SaveCompleted(
            a,
            Ok(("a.txt".to_string(), Some(PathBuf::from("/w/a.txt")))),
        )));
        assert!(app.documents.get(a).is_none());
        assert!(!center_views(&app).contains(&ViewId::Document(a)));
        assert_eq!(app.close_after_save, None);
    }

    #[test]
    fn test_save_completed_updates_document_and_status() {
        let mut app = Yui::default();
        let a = open(&mut app, "Untitled-1", None, "A");
        dirty(&mut app, a);
        let _ = app.update(Message::Editor(EditorMessage::SaveCompleted(
            a,
            Ok(("n.txt".to_string(), Some(PathBuf::from("/w/n.txt")))),
        )));
        let doc = app.documents.get(a).unwrap();
        assert!(!doc.is_dirty);
        assert_eq!(doc.file_name, "n.txt");
        assert_eq!(doc.path.as_deref(), Some(std::path::Path::new("/w/n.txt")));
        let msg = app.status_message.as_ref().unwrap();
        assert_eq!(msg.text, "Saved 'n.txt'");

        // 失敗したときはエラーを出し、未保存のまま
        dirty(&mut app, a);
        let _ = app.update(Message::Editor(EditorMessage::SaveCompleted(
            a,
            Err("disk full".to_string()),
        )));
        assert!(app.documents.get(a).unwrap().is_dirty);
        let msg = app.status_message.as_ref().unwrap();
        assert_eq!(msg.level, crate::status_bar::StatusLevel::Error);
    }

    #[test]
    fn test_open_error_goes_to_status_bar() {
        let mut app = Yui::default();
        let _ = app.update(Message::TextFileOpened(Err("not UTF-8".to_string())));
        let msg = app.status_message.as_ref().unwrap();
        assert_eq!(msg.level, crate::status_bar::StatusLevel::Error);
        assert!(app.documents.ids().is_empty());
    }

    #[test]
    fn test_workspace_load_and_new_keep_open_documents() {
        let mut app = Yui::default();
        let a = open(&mut app, "a.txt", Some("/w/a.txt"), "A");
        let b = open(&mut app, "b.txt", Some("/w/b.txt"), "B");
        let _ = app.update(Message::MenuAction(MenuAction::ViewToggleEditorSplit));
        assert_eq!(
            right_views(&app),
            vec![ViewId::Document(a), ViewId::Document(b)]
        );

        // 分割ありで保存した YAML には、ドキュメントのタブは入らない
        let yaml =
            crate::menu_bar::export_workspace_to_yaml(&app.state, &app.pane_views).expect("export");
        assert!(!yaml.contains("document"));

        // File → New は分割を既定（なし）に戻すが、開いているファイルのタブは残る
        let _ = app.update(Message::MenuAction(MenuAction::FileNew));
        assert!(!is_editor_split(&app.pane_views));
        assert_eq!(
            center_views(&app),
            vec![ViewId::Viewport, ViewId::Document(a), ViewId::Document(b)]
        );
        assert!(app.documents.get(a).is_some() && app.documents.get(b).is_some());

        // ワークスペースを読み込むと、保存されていた分割に従って、開いているファイルのタブが右側に並ぶ
        let _ = app.update(Message::WorkspaceFileOpened(Ok(Some((
            "w.yaml".to_string(),
            None,
            yaml,
        )))));
        assert!(is_editor_split(&app.pane_views));
        assert!(has_pane(&app, PaneKind::CENTER_RIGHT));
        assert_eq!(
            right_views(&app),
            vec![ViewId::Document(a), ViewId::Document(b)]
        );
        assert_eq!(center_views(&app), vec![ViewId::Viewport]);
    }

    #[test]
    fn test_active_document_follows_selected_tab() {
        let mut app = Yui::default();
        let a = open(&mut app, "a.txt", Some("/w/a.txt"), "A");
        let _b = open(&mut app, "b.txt", Some("/w/b.txt"), "B");
        let _ = app.update(Message::PaneTabSelected {
            pane: PaneKind::CENTER,
            view: ViewId::Document(a),
        });
        assert_eq!(active_document(&app.pane_views), Some(a));
        let _ = app.update(Message::PaneTabSelected {
            pane: PaneKind::CENTER,
            view: ViewId::Viewport,
        });
        assert_eq!(active_document(&app.pane_views), None);
    }

    #[test]
    fn test_dragging_a_tab_over_another_reorders_until_released() {
        let mut app = Yui::default();
        let a = open(&mut app, "a.txt", Some("/w/a.txt"), "A");
        let b = open(&mut app, "b.txt", Some("/w/b.txt"), "B");
        let (va, vb) = (ViewId::Document(a), ViewId::Document(b));
        assert_eq!(center_views(&app), vec![ViewId::Viewport, va, vb]);
        let hover = |app: &mut Yui, view| {
            let _ = app.update(Message::PaneTabHovered {
                pane: PaneKind::CENTER,
                view,
            });
        };
        // 押さずに上を通っても、並びは変わらない
        hover(&mut app, ViewId::Viewport);
        assert_eq!(center_views(&app), vec![ViewId::Viewport, va, vb]);

        let _ = app.update(Message::PaneTabSelected {
            pane: PaneKind::CENTER,
            view: vb,
        });
        hover(&mut app, va);
        assert_eq!(center_views(&app), vec![ViewId::Viewport, vb, va]);
        hover(&mut app, ViewId::Viewport);
        assert_eq!(center_views(&app), vec![vb, ViewId::Viewport, va]);
        assert_eq!(active_document(&app.pane_views), Some(b));
        let _ = app.update(Message::TabDragEnded);
        hover(&mut app, va);
        assert_eq!(center_views(&app), vec![vb, ViewId::Viewport, va]);
    }

    #[test]
    fn test_dragging_a_tab_to_another_group() {
        let mut app = Yui::default();
        app.toggle_editor_split();
        let a = open(&mut app, "a.txt", Some("/w/a.txt"), "A");
        let va = ViewId::Document(a);
        let all_right = |app: &Yui| app.pane_views[&PaneKind::CENTER_RIGHT].views.clone();
        assert_eq!(center_views(&app), vec![ViewId::Viewport]);
        assert_eq!(all_right(&app), vec![ViewId::Viewport2, va]);
        let camera = app
            .camera_of(crate::viewport::ViewportId::Secondary)
            .clone();

        // タブの帯: 左のタブの上に入ると、その位置へ移る
        let _ = app.update(Message::PaneTabSelected {
            pane: PaneKind::CENTER_RIGHT,
            view: va,
        });
        let _ = app.update(Message::PaneTabHovered {
            pane: PaneKind::CENTER,
            view: ViewId::Viewport,
        });
        assert_eq!(center_views(&app), vec![va, ViewId::Viewport]);
        assert_eq!(all_right(&app), vec![ViewId::Viewport2]);
        let _ = app.update(Message::TabDragEnded);
        assert!(app.tab_drag.is_none());

        // グループの本体: 出てから離すと、移らない
        let _ = app.update(Message::PaneTabSelected {
            pane: PaneKind::CENTER_RIGHT,
            view: ViewId::Viewport2,
        });
        let _ = app.update(Message::TabDropTargetEntered(
            PaneKind::CENTER,
            DropZone::Center,
        ));
        let _ = app.update(Message::TabDropTargetLeft(
            PaneKind::CENTER,
            DropZone::Center,
        ));
        let _ = app.update(Message::TabDragEnded);
        assert_eq!(all_right(&app), vec![ViewId::Viewport2]);
        // 中央で離すと末尾へ移り、空になった右のグループは消える。視点は変わらない
        let _ = app.update(Message::PaneTabSelected {
            pane: PaneKind::CENTER_RIGHT,
            view: ViewId::Viewport2,
        });
        let _ = app.update(Message::TabDropTargetEntered(
            PaneKind::CENTER,
            DropZone::Center,
        ));
        let _ = app.update(Message::TabDragEnded);
        assert_eq!(
            center_views(&app),
            vec![va, ViewId::Viewport, ViewId::Viewport2]
        );
        assert!(!has_pane(&app, PaneKind::CENTER_RIGHT));
        assert!(!is_editor_split(&app.pane_views));
        assert_eq!(
            app.camera_of(crate::viewport::ViewportId::Secondary),
            &camera
        );
        assert!(app.tab_drop_target.is_none());

        // ドラッグしていないときは、本体に入っても、落とす先にならない
        let _ = app.update(Message::TabDropTargetEntered(
            PaneKind::CENTER,
            DropZone::Center,
        ));
        assert!(app.tab_drop_target.is_none());
    }

    #[test]
    fn test_dropping_a_tab_on_an_edge_splits_the_group() {
        let mut app = Yui::default();
        let a = open(&mut app, "a.txt", Some("/w/a.txt"), "A");
        let va = ViewId::Document(a);
        let drop = |app: &mut Yui, from, view, to, zone| {
            let _ = app.update(Message::PaneTabSelected { pane: from, view });
            let _ = app.update(Message::TabDropTargetEntered(to, zone));
            let _ = app.update(Message::TabDragEnded);
        };
        // 下の端: 上下に分割して、新しいグループ（下）へ移す
        drop(
            &mut app,
            PaneKind::CENTER,
            va,
            PaneKind::CENTER,
            DropZone::Bottom,
        );
        let new = PaneKind::Group(crate::app::GroupId(1));
        assert_eq!(app.pane_views.groups(), vec![PaneKind::CENTER, new]);
        assert_eq!(app.pane_views[&new].views, vec![va]);
        assert!(matches!(
            app.pane_views.editor(),
            crate::pane_view::GroupNode::Split {
                axis: iced::widget::pane_grid::Axis::Horizontal,
                ..
            }
        ));
        assert_eq!(app.panes.len(), 5, "the pane grid has the new group");
        // 唯一のタブを、同じグループの端に落としても、何も変わらない
        drop(&mut app, new, va, new, DropZone::Left);
        assert_eq!(app.pane_views.groups(), vec![PaneKind::CENTER, new]);
        // 下のグループの左の端へ、3D Viewport を移すと、上のグループは空になって消える
        drop(
            &mut app,
            PaneKind::CENTER,
            ViewId::Viewport,
            new,
            DropZone::Left,
        );
        let newer = PaneKind::Group(crate::app::GroupId(2));
        assert_eq!(app.pane_views.groups(), vec![newer, new]);
        assert_eq!(app.pane_views[&newer].views, vec![ViewId::Viewport]);
        assert_eq!(app.panes.len(), 5);
        // ファイルを閉じると、空になったグループは消える
        let _ = app.update(Message::CloseTabRequested(a));
        assert_eq!(app.pane_views.groups(), vec![newer]);
        assert_eq!(app.panes.len(), 4);
    }

    #[test]
    fn test_saved_workspace_restores_the_editor_split_size() {
        use iced::widget::pane_grid::Node;
        // 中央の左右の境界を探す
        fn editor_split(app: &Yui) -> Option<(iced::widget::pane_grid::Split, f32)> {
            fn walk(app: &Yui, node: &Node) -> Option<(iced::widget::pane_grid::Split, f32)> {
                let Node::Split {
                    id, ratio, a, b, ..
                } = node
                else {
                    return None;
                };
                let kind = |n: &Node| match n {
                    Node::Pane(p) => app.panes.get(*p).copied(),
                    _ => None,
                };
                if kind(a) == Some(PaneKind::CENTER) && kind(b) == Some(PaneKind::CENTER_RIGHT) {
                    return Some((*id, *ratio));
                }
                walk(app, a).or_else(|| walk(app, b))
            }
            walk(app, app.panes.layout())
        }
        let mut app = Yui::default();
        app.toggle_editor_split();
        let (split, ratio) = editor_split(&app).expect("the editor split");
        assert_eq!(ratio, 0.5);
        // 利用者が境界をドラッグしてから、保存する
        app.panes.resize(split, 0.3);
        app.sync_editor_ratios();
        let yaml =
            crate::menu_bar::export_workspace_to_yaml(&app.state, &app.pane_views).expect("export");
        let (_, views) = crate::menu_bar::import_workspace_from_yaml(&yaml).expect("import");

        let mut other = Yui::default();
        other.set_pane_views(views);
        let (_, restored) = editor_split(&other).expect("the editor split");
        assert!((restored - 0.3).abs() < 1e-6, "{restored}");
    }

    #[test]
    fn test_quit_without_unsaved_files_has_no_dialog() {
        let mut app = Yui::default();
        let _a = open(&mut app, "a.txt", Some("/w/a.txt"), "A");
        assert!(app.unsaved_file_names().is_empty());
        let _ = app.update(Message::QuitRequested);
        assert!(!app.quit_dialog);
    }

    #[test]
    fn test_quit_with_unsaved_files_asks_and_cancel_keeps_everything() {
        let mut app = Yui::default();
        let a = open(&mut app, "a.txt", Some("/w/a.txt"), "A");
        let _b = open(&mut app, "b.txt", Some("/w/b.txt"), "B");
        let c = open(&mut app, "c.txt", Some("/w/c.txt"), "C");
        dirty(&mut app, a);
        dirty(&mut app, c);
        assert_eq!(app.unsaved_file_names(), vec!["a.txt", "c.txt"]);

        let _ = app.update(Message::QuitRequested);
        assert!(app.quit_dialog);

        let _ = app.update(Message::QuitDialogChoice(CloseChoice::Cancel));
        assert!(!app.quit_dialog);
        assert_eq!(app.quit_save_queue, None);
        assert!(app.documents.get(a).unwrap().is_dirty);
        assert!(app.documents.get(c).unwrap().is_dirty);
        assert_eq!(app.documents.ids().len(), 3);
    }

    #[test]
    fn test_quit_save_all_saves_each_unsaved_file_in_order() {
        let mut app = Yui::default();
        let a = open(&mut app, "a.txt", Some("/w/a.txt"), "A");
        let _b = open(&mut app, "b.txt", Some("/w/b.txt"), "B");
        let c = open(&mut app, "c.txt", Some("/w/c.txt"), "C");
        dirty(&mut app, a);
        dirty(&mut app, c);

        let _ = app.update(Message::QuitRequested);
        let _ = app.update(Message::QuitDialogChoice(CloseChoice::Save));
        // 未保存のファイルだけが、開いている順に並ぶ。先頭が保存中
        assert_eq!(app.quit_save_queue, Some(vec![a, c]));
        assert!(!app.quit_dialog);

        // 1 つ目が保存できたら、次の保存に進む（まだ終了しない）
        let _ = app.update(Message::Editor(EditorMessage::SaveCompleted(
            a,
            Ok(("a.txt".to_string(), Some(PathBuf::from("/w/a.txt")))),
        )));
        assert_eq!(app.quit_save_queue, Some(vec![c]));
        assert!(!app.documents.get(a).unwrap().is_dirty);

        // すべて保存できたら、キューが空になって終了へ進む
        let _ = app.update(Message::Editor(EditorMessage::SaveCompleted(
            c,
            Ok(("c.txt".to_string(), Some(PathBuf::from("/w/c.txt")))),
        )));
        assert_eq!(app.quit_save_queue, None);
        assert!(app.unsaved_file_names().is_empty());
    }

    #[test]
    fn test_quit_save_all_aborts_when_a_save_fails_or_is_canceled() {
        for failure in [SAVE_CANCELED.to_string(), "disk full".to_string()] {
            let mut app = Yui::default();
            let a = open(&mut app, "a.txt", Some("/w/a.txt"), "A");
            let b = open(&mut app, "Untitled", None, "B");
            dirty(&mut app, a);
            dirty(&mut app, b);

            let _ = app.update(Message::QuitRequested);
            let _ = app.update(Message::QuitDialogChoice(CloseChoice::Save));
            let _ = app.update(Message::Editor(EditorMessage::SaveCompleted(
                a,
                Ok(("a.txt".to_string(), Some(PathBuf::from("/w/a.txt")))),
            )));
            assert_eq!(app.quit_save_queue, Some(vec![b]));

            // 2 つ目の保存がキャンセル・失敗したら、終了を取りやめる（未保存のまま残る）
            let _ = app.update(Message::Editor(EditorMessage::SaveCompleted(
                b,
                Err(failure.clone()),
            )));
            assert_eq!(app.quit_save_queue, None, "{failure}");
            assert!(app.documents.get(b).unwrap().is_dirty);
            assert!(app.status_message.is_some());
        }
    }

    #[test]
    fn test_quit_dont_save_closes_dialog() {
        let mut app = Yui::default();
        let a = open(&mut app, "a.txt", Some("/w/a.txt"), "A");
        dirty(&mut app, a);
        let _ = app.update(Message::QuitRequested);
        assert!(app.quit_dialog);
        let _ = app.update(Message::QuitDialogChoice(CloseChoice::DontSave));
        assert!(!app.quit_dialog);
        assert_eq!(app.quit_save_queue, None);
    }

    #[test]
    fn test_quit_dialog_detail_truncates_long_lists() {
        use crate::text_editor_pane::quit_dialog_detail;
        let few = vec!["a.txt".to_string(), "b.txt".to_string()];
        let text = quit_dialog_detail(&few);
        assert!(text.contains("• a.txt") && text.contains("• b.txt"));
        assert!(!text.contains("more"));

        let many: Vec<String> = (1..=11).map(|i| format!("f{i}.txt")).collect();
        let text = quit_dialog_detail(&many);
        assert!(text.contains("• f8.txt"));
        assert!(!text.contains("f9.txt"));
        assert!(text.contains("…and 3 more"));
    }

    #[test]
    fn test_open_settings_at_creates_template_and_opens_one_tab() {
        let dir = std::env::temp_dir().join(format!("yui-open-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("yui").join("settings.yaml");

        let mut app = Yui::default();
        app.open_settings_at(path.clone());
        // 雛形が作られ、タブで開く
        assert!(path.exists());
        let ids = app.documents.ids();
        assert_eq!(ids.len(), 1);
        let doc = app.documents.get(ids[0]).unwrap();
        assert_eq!(doc.file_name, "settings.yaml");
        assert_eq!(doc.path.as_deref(), Some(path.as_path()));
        assert_eq!(
            doc.content.text().trim_end(),
            crate::user_settings::DEFAULT_SETTINGS_TEMPLATE.trim_end()
        );
        let msg = app.status_message.as_ref().unwrap();
        assert!(msg.text.contains("restart"), "{}", msg.text);
        assert_eq!(active_document(&app.pane_views), Some(ids[0]));

        // もう一度開いても、タブは増えない
        app.open_settings_at(path);
        assert_eq!(app.documents.ids().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_save_shortcut_detection() {
        use crate::text_editor_pane::is_save_shortcut;
        use iced::keyboard::{Key, Modifiers};
        let s = Key::Character("s".into());
        assert!(is_save_shortcut(&s, Modifiers::COMMAND));
        assert!(is_save_shortcut(
            &Key::Character("S".into()),
            Modifiers::COMMAND
        ));
        assert!(!is_save_shortcut(&s, Modifiers::empty()));
        assert!(!is_save_shortcut(&s, Modifiers::COMMAND | Modifiers::SHIFT));
        assert!(!is_save_shortcut(
            &Key::Character("a".into()),
            Modifiers::COMMAND
        ));
    }
}
