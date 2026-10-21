//! テキストファイルのエディタ（ドキュメントごとのタブ）
//!
//! 開いているファイルを `Document` として `DocumentStore` に持つ。タブの並び（どのペインのどこか）は
//! `pane_view` が持ち、ここは内容・パス・未保存かどうかだけを持つ。ドキュメントはワークスペースの YAML にも
//! 協調編集にも入れない（利用者ごとのもの）。

use std::path::{Path, PathBuf};

use iced::widget::scrollable::{AbsoluteOffset, Direction, Scrollbar};
use iced::widget::{button, column, container, row, scrollable, sensor, text, text_editor};
use iced::{Element, Fill, Size, Task};
use serde::{Deserialize, Serialize};

use crate::fonts;

/// エディタにスクロールバーを出すか（設定の `editor.scrollbar`。起動時に 1 回だけ決める）
static SCROLLBAR: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

pub fn set_scrollbar(on: bool) {
    let _ = SCROLLBAR.set(on);
}

pub fn scrollbar_enabled() -> bool {
    SCROLLBAR.get().copied().unwrap_or(true)
}

/// エディタ本体の内側の余白（ピクセル）
const PAD: f32 = 5.0;
/// iced の行の高さの既定（文字の大きさに対する倍率）
const LINE_HEIGHT: f32 = 1.3;
/// 等幅フォントの文字の幅（文字の大きさに対する倍率）の見積もり
const CHAR_WIDTH: f32 = 0.6;

fn scroll_id(id: DocumentId) -> iced::widget::Id {
    iced::widget::Id::from(format!("yui-editor-scroll-{}", id.0))
}

/// ドキュメントの識別子（アプリの起動中だけ一意）
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DocumentId(pub u32);

/// 開いているテキストファイル 1 つ分
#[derive(Debug)]
pub struct Document {
    pub id: DocumentId,
    /// エディタの内容・カーソル・選択範囲等
    pub content: text_editor::Content,
    /// ファイルのパス（ネイティブのみ意味を持つ。wasm32 や、まだ保存していない新規ファイルでは `None`）
    pub path: Option<PathBuf>,
    /// 表示用のファイル名（新規ファイルは `Untitled-1` など）
    pub file_name: String,
    /// 未保存の変更があるか
    pub is_dirty: bool,
    /// スクロールバーを出すときの状態（表示のたびに `sensor` / `scrollable` から受け取る）
    pub scroll: ScrollState,
}

/// スクロールバーつきの表示の状態。テキストの位置は iced のエディタから読めないので、
/// 表示の大きさと内容の高さから、カーソルの位置を見積もる。
#[derive(Debug, Default, Clone)]
pub struct ScrollState {
    /// 見えている範囲の大きさ
    pub view: Size,
    /// エディタ本体（内容全体）の大きさ
    pub content: Size,
    /// 縦のスクロール量
    pub offset: f32,
    /// 編集の直後で、内容の高さが更新されたらカーソルへ追従し直す
    pub follow_pending: bool,
}

impl Document {
    /// タブに表示する名前（未保存なら `*` を付ける）
    pub fn tab_label(&self) -> String {
        if self.is_dirty {
            format!("*{}", self.file_name)
        } else {
            self.file_name.clone()
        }
    }

    /// カーソルが見える位置になるための縦のスクロール量（すでに見えていれば `None`）。
    /// 折り返しで増える行数は、文字の幅から見積もる。内容の高さが分かっていれば、見積もりを実測に合わせる。
    pub fn follow_offset(&self) -> Option<f32> {
        let size = fonts::editor_font_size();
        let line_h = size * LINE_HEIGHT;
        let width = if self.scroll.content.width > 0.0 {
            self.scroll.content.width
        } else {
            self.scroll.view.width
        };
        let cols = ((width - 2.0 * PAD) / (size * CHAR_WIDTH)).floor().max(1.0) as usize;
        let cursor = self.content.cursor().position;
        let (row, total) = estimate_rows(
            self.content.lines().map(|l| l.text.into_owned()),
            cursor.line,
            cursor.column,
            cols,
        );
        let scale = if self.scroll.content.height > 2.0 * PAD && total > 0 {
            (self.scroll.content.height - 2.0 * PAD) / (total as f32 * line_h)
        } else {
            1.0
        };
        let y = PAD + row as f32 * line_h * scale;
        follow_offset(
            y,
            line_h * scale,
            self.scroll.offset,
            self.scroll.view.height,
        )
    }
}

/// 1 行が、`cols` 桁の幅で折り返されたときの行数（空の行も 1 行）
fn visual_rows(line: &str, cols: usize) -> usize {
    use unicode_width::UnicodeWidthStr;
    line.width().div_ceil(cols).max(1)
}

/// 全体の行数と、カーソルのある表示行（0 始まり）を見積もる。
/// `cursor_line` / `cursor_col` は文字単位の位置。
fn estimate_rows(
    lines: impl Iterator<Item = String>,
    cursor_line: usize,
    cursor_col: usize,
    cols: usize,
) -> (usize, usize) {
    use unicode_width::UnicodeWidthStr;
    let mut total = 0;
    let mut cursor_row = 0;
    for (i, line) in lines.enumerate() {
        if i == cursor_line {
            let head: String = line.chars().take(cursor_col).collect();
            cursor_row = total + head.width() / cols;
        }
        total += visual_rows(&line, cols);
    }
    (cursor_row, total)
}

/// 高さ `line_h` の行が、位置 `y` にあるとき、見えている範囲（`offset` から `view_h`）に入るためのスクロール量
fn follow_offset(y: f32, line_h: f32, offset: f32, view_h: f32) -> Option<f32> {
    if view_h <= line_h {
        return None;
    }
    if y < offset {
        Some(y - PAD)
    } else if y + line_h > offset + view_h {
        Some(y + line_h + PAD - view_h)
    } else {
        None
    }
}

/// カーソルを動かす（内容を変える）操作か。クリックやドラッグは、見えている場所を指すので追従しない。
fn moves_cursor(action: &text_editor::Action) -> bool {
    use text_editor::Action as A;
    matches!(
        action,
        A::Move(_) | A::Select(_) | A::SelectAll | A::Edit(_)
    )
}

/// 開いているドキュメントの一覧
#[derive(Debug, Default)]
pub struct DocumentStore {
    docs: Vec<Document>,
    next_id: u32,
    untitled_count: u32,
}

impl DocumentStore {
    fn alloc_id(&mut self) -> DocumentId {
        self.next_id += 1;
        DocumentId(self.next_id)
    }

    /// 内容を読み込んだファイルを開く。同じパスのファイルがすでに開いていれば、新しく作らず、そのドキュメントを返す
    /// （編集中の内容を失わないため、読み込み直さない）。返り値の `bool` は、新しく作ったかどうか。
    pub fn open(
        &mut self,
        file_name: String,
        path: Option<PathBuf>,
        text: &str,
    ) -> (DocumentId, bool) {
        if let Some(p) = path.as_deref()
            && let Some(existing) = self.find_by_path(p)
        {
            return (existing, false);
        }
        let id = self.alloc_id();
        self.docs.push(Document {
            id,
            content: text_editor::Content::with_text(text),
            path,
            file_name,
            is_dirty: false,
            scroll: ScrollState::default(),
        });
        (id, true)
    }

    /// 新しい空のファイルを作る（名前は `Untitled-1`, `Untitled-2`, ...）
    pub fn new_untitled(&mut self) -> DocumentId {
        self.untitled_count += 1;
        let name = format!("Untitled-{}", self.untitled_count);
        let id = self.alloc_id();
        self.docs.push(Document {
            id,
            content: text_editor::Content::new(),
            path: None,
            file_name: name,
            is_dirty: false,
            scroll: ScrollState::default(),
        });
        id
    }

    pub fn find_by_path(&self, path: &Path) -> Option<DocumentId> {
        self.docs
            .iter()
            .find(|d| d.path.as_deref() == Some(path))
            .map(|d| d.id)
    }

    pub fn get(&self, id: DocumentId) -> Option<&Document> {
        self.docs.iter().find(|d| d.id == id)
    }

    pub fn get_mut(&mut self, id: DocumentId) -> Option<&mut Document> {
        self.docs.iter_mut().find(|d| d.id == id)
    }

    pub fn remove(&mut self, id: DocumentId) -> Option<Document> {
        let pos = self.docs.iter().position(|d| d.id == id)?;
        Some(self.docs.remove(pos))
    }

    pub fn ids(&self) -> Vec<DocumentId> {
        self.docs.iter().map(|d| d.id).collect()
    }

    /// エディタの操作を、ドキュメントへ反映する。内容を変える操作のときだけ、未保存にする
    /// （カーソルの移動や選択では、未保存にしない）。
    ///
    /// スクロールバーを出すときは、カーソルを見える範囲に入れるための `scroll_to` を返す
    /// （iced のエディタは、ほかの部品の中では、自分でスクロールしないため）。
    pub fn perform(&mut self, id: DocumentId, action: text_editor::Action) -> Task<EditorMessage> {
        let Some(doc) = self.get_mut(id) else {
            return Task::none();
        };
        let is_edit = action.is_edit();
        let follow = scrollbar_enabled() && moves_cursor(&action);
        doc.content.perform(action);
        if is_edit {
            doc.is_dirty = true;
            doc.scroll.follow_pending = true;
        }
        match doc.follow_offset() {
            Some(y) if follow => scroll_task(id, y),
            _ => Task::none(),
        }
    }

    /// 表示の大きさ・内容の高さ・スクロール量を受け取る。編集の直後は、高さが変わったので追従し直す。
    fn apply_scroll(&mut self, id: DocumentId, change: ScrollChange) -> Task<EditorMessage> {
        let Some(doc) = self.get_mut(id) else {
            return Task::none();
        };
        match change {
            ScrollChange::View(s) => doc.scroll.view = s,
            ScrollChange::Content(s) => doc.scroll.content = s,
            ScrollChange::Offset(y) => {
                doc.scroll.offset = y;
                return Task::none();
            }
        }
        if doc.scroll.follow_pending {
            doc.scroll.follow_pending = false;
            if let Some(y) = doc.follow_offset() {
                return scroll_task(id, y);
            }
        }
        Task::none()
    }

    /// 表示にかかわるメッセージ（大きさ・スクロール量）を反映する
    pub fn handle_scroll(&mut self, msg: &EditorMessage) -> Option<Task<EditorMessage>> {
        match *msg {
            EditorMessage::ViewResized(id, s) => Some(self.apply_scroll(id, ScrollChange::View(s))),
            EditorMessage::ContentResized(id, s) => {
                Some(self.apply_scroll(id, ScrollChange::Content(s)))
            }
            EditorMessage::Scrolled(id, y) => Some(self.apply_scroll(id, ScrollChange::Offset(y))),
            _ => None,
        }
    }

    /// 保存が終わったことを反映する（名前とパスは「名前を付けて保存」で変わることがある）。
    pub fn mark_saved(&mut self, id: DocumentId, file_name: String, path: Option<PathBuf>) {
        if let Some(doc) = self.get_mut(id) {
            doc.file_name = file_name;
            doc.path = path;
            doc.is_dirty = false;
        }
    }
}

enum ScrollChange {
    View(Size),
    Content(Size),
    Offset(f32),
}

fn scroll_task(id: DocumentId, y: f32) -> Task<EditorMessage> {
    iced::widget::operation::scroll_to(
        scroll_id(id),
        AbsoluteOffset {
            x: None,
            y: Some(y.max(0.0)),
        },
    )
}

/// エディタのメッセージ
#[derive(Debug, Clone)]
pub enum EditorMessage {
    /// スクロールバーつきの表示：見えている範囲の大きさ
    ViewResized(DocumentId, Size),
    /// スクロールバーつきの表示：エディタ本体（内容全体）の大きさ
    ContentResized(DocumentId, Size),
    /// スクロールバーつきの表示：縦のスクロール量
    Scrolled(DocumentId, f32),
    /// エディタの編集・カーソル操作アクション
    Edit(DocumentId, text_editor::Action),
    /// 「Save」ボタン押下
    Save(DocumentId),
    /// 「Save As」ボタン押下
    SaveAs(DocumentId),
    /// ファイル保存完了（`Err("Save canceled")` はダイアログのキャンセル）
    SaveCompleted(DocumentId, Result<(String, Option<PathBuf>), String>),
}

/// 保存のダイアログがキャンセルされたときの `Err` の文
pub const SAVE_CANCELED: &str = "Save canceled";

/// ファイル選択ダイアログを開き、内容を読み込む非同期関数。
/// ユーザーによるキャンセルは`Ok(None)`、UTF-8として解釈できない場合は`Err`を返す。
pub async fn open_file_dialog() -> Result<Option<(String, Option<PathBuf>, String)>, String> {
    let Some(handle) = rfd::AsyncFileDialog::new().pick_file().await else {
        return Ok(None);
    };
    let file_name = handle.file_name();

    #[cfg(not(target_arch = "wasm32"))]
    let path = Some(handle.path().to_path_buf());
    #[cfg(target_arch = "wasm32")]
    let path = None;

    let bytes = handle.read().await;
    let content = String::from_utf8(bytes)
        .map_err(|_| format!("'{file_name}' is not a valid UTF-8 text file"))?;

    Ok(Some((file_name, path, content)))
}

/// ファイル保存ダイアログ（または既存パスへの直接保存）を行う非同期関数
#[allow(unused_variables)]
pub async fn save_file(
    current_path: Option<PathBuf>,
    suggested_name: Option<String>,
    text_content: String,
) -> Result<(String, Option<PathBuf>), String> {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(path) = current_path {
        match tokio::fs::write(&path, text_content.as_bytes()).await {
            Ok(()) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "untitled".to_string());
                return Ok((name, Some(path)));
            }
            Err(e) => return Err(format!("Failed to save file: {e}")),
        }
    }

    save_file_as(suggested_name, text_content).await
}

/// 「名前を付けて保存」ダイアログを開き、保存する非同期関数
pub async fn save_file_as(
    suggested_name: Option<String>,
    text_content: String,
) -> Result<(String, Option<PathBuf>), String> {
    let mut dialog = rfd::AsyncFileDialog::new();
    if let Some(ref name) = suggested_name {
        dialog = dialog.set_file_name(name);
    }

    let handle = match dialog.save_file().await {
        Some(h) => h,
        None => return Err(SAVE_CANCELED.to_string()),
    };

    let file_name = handle.file_name();

    #[cfg(not(target_arch = "wasm32"))]
    let path = Some(handle.path().to_path_buf());
    #[cfg(target_arch = "wasm32")]
    let path = None;

    if let Err(e) = handle.write(text_content.as_bytes()).await {
        return Err(format!("Failed to write file: {e}"));
    }

    Ok((file_name, path))
}

/// 保存のショートカット（Ctrl+S。macOS は Cmd+S）か
pub fn is_save_shortcut(key: &iced::keyboard::Key, modifiers: iced::keyboard::Modifiers) -> bool {
    modifiers.command()
        && !modifiers.shift()
        && !modifiers.alt()
        && matches!(key.as_ref(), iced::keyboard::Key::Character(c) if c.eq_ignore_ascii_case("s"))
}

/// ドキュメント 1 つ分のエディタ（ツールバーと本体）
pub fn view_document(doc: &Document) -> Element<'_, EditorMessage> {
    let id = doc.id;
    let save_btn = button(text("Save").size(12))
        .padding([3, 8])
        .style(crate::ui_style::accent_button)
        .on_press(EditorMessage::Save(id));
    let save_as_btn = button(text("Save As").size(12))
        .padding([3, 8])
        .style(crate::ui_style::secondary_button)
        .on_press(EditorMessage::SaveAs(id));
    let file_label = text(doc.tab_label()).size(12);

    let toolbar = row![save_btn, save_as_btn, file_label]
        .spacing(8)
        .align_y(iced::Alignment::Center);

    let editor = text_editor(&doc.content)
        .on_action(move |action| EditorMessage::Edit(id, action))
        // フォーカスがあるときは、エディタが先にキーを受ける。macOS の Cmd+S が文字入力として扱われないよう、
        // 保存のショートカットはここで先に処理する。
        .key_binding(move |key_press| {
            if is_save_shortcut(&key_press.key, key_press.modifiers) {
                Some(text_editor::Binding::Custom(EditorMessage::Save(id)))
            } else {
                text_editor::Binding::from_key_press(key_press)
            }
        })
        .font(fonts::mono_font(false))
        .size(fonts::editor_font_size());

    let body: Element<'_, EditorMessage> = if scrollbar_enabled() {
        // iced のエディタにはスクロールバーがないので、高さを内容に合わせて、`scrollable` で包む。
        // 余白のクリックでも入力できるよう、最低の高さは見えている範囲に合わせる。
        let editor = editor
            .padding(PAD)
            .height(iced::Length::Shrink)
            .min_height(doc.scroll.view.height);
        let measured = sensor(editor)
            .on_show(move |s| EditorMessage::ContentResized(id, s))
            .on_resize(move |s| EditorMessage::ContentResized(id, s));
        let scroll = scrollable(measured)
            .id(scroll_id(id))
            .direction(Direction::Vertical(
                Scrollbar::new().width(10).scroller_width(8).spacing(2),
            ))
            .on_scroll(move |v| EditorMessage::Scrolled(id, v.absolute_offset().y))
            .width(Fill)
            .height(Fill);
        sensor(scroll)
            .on_show(move |s| EditorMessage::ViewResized(id, s))
            .on_resize(move |s| EditorMessage::ViewResized(id, s))
            .into()
    } else {
        editor.padding(PAD).height(Fill).into()
    };

    column![
        container(toolbar).padding(4).width(Fill),
        container(body).padding(4).width(Fill).height(Fill),
    ]
    .spacing(2)
    .width(Fill)
    .height(Fill)
    .into()
}

/// 未保存のファイルを閉じるときの確認の選択
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseChoice {
    /// 保存してから閉じる
    Save,
    /// 保存せずに閉じる
    DontSave,
    /// 閉じない
    Cancel,
}

/// 確認のカード（メッセージと、Save / Don't Save / Cancel の 3 つのボタン）。
/// `save_label` は保存のボタンの名前（1 つのファイルなら "Save"、複数なら "Save All"）。
fn view_unsaved_card<'a, Message: Clone + 'a>(
    heading: String,
    detail: String,
    save_label: &'static str,
    on_choice: impl Fn(CloseChoice) -> Message + 'a,
) -> Element<'a, Message> {
    let card = container(
        column![
            text(heading).size(14),
            text(detail).size(12),
            row![
                button(text(save_label).size(12))
                    .padding([4, 12])
                    .style(crate::ui_style::accent_button)
                    .on_press(on_choice(CloseChoice::Save)),
                button(text("Don't Save").size(12))
                    .padding([4, 12])
                    .style(crate::ui_style::secondary_button)
                    .on_press(on_choice(CloseChoice::DontSave)),
                button(text("Cancel").size(12))
                    .padding([4, 12])
                    .style(crate::ui_style::secondary_button)
                    .on_press(on_choice(CloseChoice::Cancel)),
            ]
            .spacing(8),
        ]
        .spacing(10),
    )
    .padding(16)
    .style(container::rounded_box);

    container(card).center(Fill).into()
}

/// 未保存のファイルを閉じるときの確認のカード
pub fn view_close_dialog<'a, Message: Clone + 'a>(
    file_name: &str,
    on_choice: impl Fn(CloseChoice) -> Message + 'a,
) -> Element<'a, Message> {
    view_unsaved_card(
        format!("Do you want to save the changes to '{file_name}'?"),
        "Your changes will be lost if you don't save them.".to_string(),
        "Save",
        on_choice,
    )
}

/// 一覧に名前を並べる最大数（それより多いときは「ほか N 件」にする）
const QUIT_DIALOG_MAX_NAMES: usize = 8;

/// 終了するときの確認の文（未保存のファイルの名前を並べる。多いときは省略する）
pub fn quit_dialog_detail(names: &[String]) -> String {
    let mut lines: Vec<String> = names
        .iter()
        .take(QUIT_DIALOG_MAX_NAMES)
        .map(|n| format!("  • {n}"))
        .collect();
    if names.len() > QUIT_DIALOG_MAX_NAMES {
        lines.push(format!(
            "  …and {} more",
            names.len() - QUIT_DIALOG_MAX_NAMES
        ));
    }
    lines.push("Your changes will be lost if you don't save them.".to_string());
    lines.join("\n")
}

/// 終了するときの、未保存のファイルの確認のカード
pub fn view_quit_dialog<'a, Message: Clone + 'a>(
    names: &[String],
    on_choice: impl Fn(CloseChoice) -> Message + 'a,
) -> Element<'a, Message> {
    let heading = if names.len() == 1 {
        "1 file has unsaved changes. Save before quitting?".to_string()
    } else {
        format!(
            "{} files have unsaved changes. Save before quitting?",
            names.len()
        )
    };
    let save_label = if names.len() == 1 { "Save" } else { "Save All" };
    view_unsaved_card(heading, quit_dialog_detail(names), save_label, on_choice)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_visual_rows_wraps_by_display_width() {
        assert_eq!(visual_rows("", 10), 1);
        assert_eq!(visual_rows("abcdefghij", 10), 1);
        assert_eq!(visual_rows("abcdefghijk", 10), 2);
        // 全角は 2 桁
        assert_eq!(visual_rows("あいうえお", 10), 1);
        assert_eq!(visual_rows("あいうえおあ", 10), 2);
    }

    #[test]
    fn test_estimate_rows_counts_wrapped_lines_before_the_cursor() {
        let lines = || ["abcdefghijk", "", "xyz"].iter().map(|s| s.to_string());
        // 1 行目は 2 行に折り返す。カーソルが 3 行目の先頭なら、その前に 3 行ある
        assert_eq!(estimate_rows(lines(), 2, 0, 10), (3, 4));
        // 1 行目の 11 文字目（折り返した 2 行目）
        assert_eq!(estimate_rows(lines(), 0, 10, 10), (1, 4));
        // 行がない位置は先頭
        assert_eq!(estimate_rows(lines(), 9, 0, 10), (0, 4));
    }

    #[test]
    fn test_follow_offset_scrolls_only_when_the_cursor_is_out_of_view() {
        // 見える範囲は 100〜300
        assert_eq!(follow_offset(150.0, 18.0, 100.0, 200.0), None);
        // 上に出た
        assert_eq!(follow_offset(50.0, 18.0, 100.0, 200.0), Some(45.0));
        // 下に出た（行の下端が見えるように）
        assert_eq!(follow_offset(400.0, 18.0, 100.0, 200.0), Some(223.0));
        // 見える範囲がまだ分からない（0）ときは動かさない
        assert_eq!(follow_offset(400.0, 18.0, 0.0, 0.0), None);
    }

    #[test]
    fn test_cursor_moves_follow_but_mouse_actions_do_not() {
        use text_editor::{Action, Motion};
        assert!(moves_cursor(&Action::Move(Motion::Down)));
        assert!(moves_cursor(&Action::Edit(text_editor::Edit::Enter)));
        assert!(!moves_cursor(&Action::Scroll { lines: 3 }));
        assert!(!moves_cursor(&Action::Click(iced::Point::ORIGIN)));
    }

    #[test]
    fn test_document_follows_the_cursor_down_a_long_file() {
        let mut store = DocumentStore::default();
        let text = (0..200).map(|i| format!("line {i}\n")).collect::<String>();
        let (id, _) = store.open("a.txt".into(), None, &text);
        let doc = store.get_mut(id).unwrap();
        doc.scroll.view = Size::new(600.0, 300.0);
        let line_h = fonts::editor_font_size() * LINE_HEIGHT;
        doc.scroll.content = Size::new(600.0, 2.0 * PAD + 201.0 * line_h);
        // 先頭では動かさない
        assert_eq!(doc.follow_offset(), None);
        // 末尾まで動かすと、スクロール量は内容の下端に届く
        doc.content
            .perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
        let y = doc.follow_offset().expect("the end is out of view");
        let max = doc.scroll.content.height - doc.scroll.view.height;
        assert!(y >= max - 1.0 && y <= max + PAD + line_h, "{y} vs {max}");
    }

    fn store_with(names: &[(&str, Option<&str>)]) -> (DocumentStore, Vec<DocumentId>) {
        let mut store = DocumentStore::default();
        let ids = names
            .iter()
            .map(|(n, p)| store.open(n.to_string(), p.map(PathBuf::from), "text").0)
            .collect();
        (store, ids)
    }

    #[test]
    fn test_open_creates_documents_and_dedupes_by_path() {
        let mut store = DocumentStore::default();
        let (a, created_a) = store.open("a.txt".into(), Some(PathBuf::from("/x/a.txt")), "A");
        let (b, created_b) = store.open("b.txt".into(), Some(PathBuf::from("/x/b.txt")), "B");
        assert!(created_a && created_b);
        assert_ne!(a, b);

        // 同じパスなら、新しく作らず、編集中の内容も読み込み直さない
        let _ = store.perform(a, text_editor::Action::Edit(text_editor::Edit::Insert('!')));
        let (again, created) = store.open("a.txt".into(), Some(PathBuf::from("/x/a.txt")), "NEW");
        assert_eq!(again, a);
        assert!(!created);
        assert!(store.get(a).unwrap().content.text().contains('!'));
        assert_eq!(store.ids().len(), 2);

        // パスがない（wasm32）ファイルは、同名でも別のドキュメント
        let (c, _) = store.open("c.txt".into(), None, "C");
        let (d, created_d) = store.open("c.txt".into(), None, "C");
        assert_ne!(c, d);
        assert!(created_d);
    }

    #[test]
    fn test_new_untitled_names_and_unique_ids() {
        let mut store = DocumentStore::default();
        let a = store.new_untitled();
        let b = store.new_untitled();
        assert_ne!(a, b);
        assert_eq!(store.get(a).unwrap().file_name, "Untitled-1");
        assert_eq!(store.get(b).unwrap().file_name, "Untitled-2");
        // 閉じても番号は再利用しない
        store.remove(a);
        let c = store.new_untitled();
        assert_eq!(store.get(c).unwrap().file_name, "Untitled-3");
        assert!(c != a);
    }

    #[test]
    fn test_perform_marks_dirty_only_for_edits() {
        let (mut store, ids) = store_with(&[("a.txt", None)]);
        let id = ids[0];
        let _ = store.perform(
            id,
            text_editor::Action::Move(text_editor::Motion::DocumentEnd),
        );
        assert!(
            !store.get(id).unwrap().is_dirty,
            "cursor moves are not edits"
        );
        let _ = store.perform(
            id,
            text_editor::Action::Edit(text_editor::Edit::Insert('x')),
        );
        let doc = store.get(id).unwrap();
        assert!(doc.is_dirty);
        assert_eq!(doc.tab_label(), "*a.txt");
    }

    #[test]
    fn test_mark_saved_updates_name_path_and_clears_dirty() {
        let (mut store, ids) = store_with(&[("Untitled-1", None)]);
        let id = ids[0];
        let _ = store.perform(
            id,
            text_editor::Action::Edit(text_editor::Edit::Insert('x')),
        );
        store.mark_saved(id, "n.txt".into(), Some(PathBuf::from("/d/n.txt")));
        let doc = store.get(id).unwrap();
        assert!(!doc.is_dirty);
        assert_eq!(doc.tab_label(), "n.txt");
        assert_eq!(store.find_by_path(Path::new("/d/n.txt")), Some(id));
    }

    #[test]
    fn test_remove_returns_document() {
        let (mut store, ids) = store_with(&[("a.txt", None), ("b.txt", None)]);
        assert!(store.remove(ids[0]).is_some());
        assert!(store.get(ids[0]).is_none());
        assert!(store.remove(ids[0]).is_none());
        assert_eq!(store.ids(), vec![ids[1]]);
    }
}
