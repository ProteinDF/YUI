//! ターミナルペイン UI および VTE 状態管理モジュール
//!
//! architecture.md 13.2節・13.4節に基づき、backend の PTY セッションと通信し、
//! vt100 クレートで ANSI/VT エスケープシーケンスをパースしてセルグリッドを描画します。

use crate::terminal_grid::{GridCell, TerminalGrid};
use crate::ui_style::palette;
use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use iced::widget::{button, column, container, mouse_area, row, sensor, text, text_input};
use iced::{Color, Element, Fill, Size};

/// 接続（WebSocket）へ送るもの
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalOut {
    /// キー入力のバイト列
    Input(Vec<u8>),
    /// 端末の大きさ（行・桁）の変更
    Resize { rows: u16, cols: u16 },
}

impl TerminalOut {
    /// 大きさの変更を送る JSON（backend の `ClientTerminalMessage::Resize`）
    pub fn resize_json(rows: u16, cols: u16) -> String {
        format!(r#"{{"type":"resize","rows":{rows},"cols":{cols}}}"#)
    }
}

/// 1 文字の幅（ピクセル）。等幅フォントは、文字の大きさの約 0.6 倍
fn cell_width() -> f32 {
    crate::fonts::terminal_font_size() * 0.6
}

/// 1 行の高さ（ピクセル）。行の高さの既定（1.3 倍）に、行の間隔 1 を足した値
fn cell_height() -> f32 {
    crate::fonts::terminal_font_size() * 1.3 + 1.0
}
/// 画面の内側の余白（`view` の `container.padding`）
const SCREEN_PADDING: f32 = 6.0;
/// ホイール 1 目盛りで動かす行数
const WHEEL_LINES: f32 = 3.0;
/// 画面の内側の余白（左右・上下の合計）。スクロールバーの分も含める
const SCREEN_MARGIN: Size = Size::new(12.0 + 10.0, 12.0);

/// 画面の大きさ（ピクセル）に入る、端末の行と桁
pub fn grid_for(size: Size) -> (u16, u16) {
    let cols = ((size.width - SCREEN_MARGIN.width) / cell_width()).floor();
    let rows = ((size.height - SCREEN_MARGIN.height) / cell_height()).floor();
    (
        rows.clamp(2.0, 200.0) as u16,
        cols.clamp(20.0, 500.0) as u16,
    )
}

/// キー入力を、端末へ送るバイト列にする。`app_cursor` は、アプリケーションカーソルキーのモード
/// （`vim` や `less` が入れる。矢印キーが `ESC O A` になる）。送らないキー（`None`）は、
/// Cmd を使うもの、Ctrl+Shift の組（コピー・貼り付けの予約）、修飾キーだけのもの。
pub fn key_bytes(
    key: &Key,
    modifiers: Modifiers,
    text: Option<&str>,
    app_cursor: bool,
) -> Option<Vec<u8>> {
    let (ctrl, alt, shift) = (modifiers.control(), modifiers.alt(), modifiers.shift());
    if modifiers.logo() {
        return None;
    }
    let with_alt = |mut v: Vec<u8>| {
        if alt {
            v.insert(0, 0x1b);
        }
        v
    };
    match key {
        Key::Named(named) => {
            let cursor = |c: char| {
                if app_cursor {
                    format!("\x1bO{c}")
                } else {
                    format!("\x1b[{c}")
                }
            };
            let seq: String = match named {
                Named::Enter => return Some(with_alt(b"\r".to_vec())),
                Named::Backspace => return Some(with_alt(vec![0x7f])),
                Named::Tab if shift => "\x1b[Z".to_string(),
                Named::Tab => return Some(with_alt(b"\t".to_vec())),
                Named::Escape => "\x1b".to_string(),
                Named::Space if ctrl => return Some(vec![0]),
                Named::Space => return Some(with_alt(b" ".to_vec())),
                Named::ArrowUp => cursor('A'),
                Named::ArrowDown => cursor('B'),
                Named::ArrowRight => cursor('C'),
                Named::ArrowLeft => cursor('D'),
                Named::Home => cursor('H'),
                Named::End => cursor('F'),
                Named::PageUp => "\x1b[5~".to_string(),
                Named::PageDown => "\x1b[6~".to_string(),
                Named::Delete => "\x1b[3~".to_string(),
                Named::Insert => "\x1b[2~".to_string(),
                Named::F1 => "\x1bOP".to_string(),
                Named::F2 => "\x1bOQ".to_string(),
                Named::F3 => "\x1bOR".to_string(),
                Named::F4 => "\x1bOS".to_string(),
                Named::F5 => "\x1b[15~".to_string(),
                Named::F6 => "\x1b[17~".to_string(),
                Named::F7 => "\x1b[18~".to_string(),
                Named::F8 => "\x1b[19~".to_string(),
                Named::F9 => "\x1b[20~".to_string(),
                Named::F10 => "\x1b[21~".to_string(),
                Named::F11 => "\x1b[23~".to_string(),
                Named::F12 => "\x1b[24~".to_string(),
                _ => return None,
            };
            Some(seq.into_bytes())
        }
        Key::Character(c) => {
            if ctrl {
                // Ctrl+Shift は、コピー・貼り付けのために空けておく
                if shift {
                    return None;
                }
                let ch = c.chars().next()?;
                let code = match ch.to_ascii_lowercase() {
                    l @ 'a'..='z' => l as u8 - b'a' + 1,
                    '[' => 0x1b,
                    '\\' => 0x1c,
                    ']' => 0x1d,
                    '/' => 0x1f,
                    _ => return None,
                };
                return Some(with_alt(vec![code]));
            }
            // シフトなどを反映した文字（`text`）を優先する
            let s = text
                .filter(|t| !t.is_empty() && !t.chars().any(char::is_control))
                .unwrap_or(c.as_str());
            Some(with_alt(s.as_bytes().to_vec()))
        }
        Key::Unidentified => None,
    }
}

/// ターミナルペインの UI メッセージ
#[derive(Debug, Clone)]
pub enum TerminalPaneMessage {
    /// 接続ボタン押下
    ConnectPressed,
    /// 同じマシンのシェルへの接続ボタン押下（デスクトップ版だけ）
    #[cfg(not(target_arch = "wasm32"))]
    ConnectLocalPressed,
    /// 切断ボタン押下
    DisconnectPressed,
    /// コマンド入力欄の変更
    CommandInputChanged(String),
    /// 入力欄（Enter で送る行）の表示を切り替える
    ToggleInputLine,
    /// 日本語入力（IME）の出来事（変換中の文字、確定した文字）
    Ime(crate::terminal_widget::ImeEvent),
    /// コマンド送信（Enter または送信ボタン）
    CommandSubmitted,
    /// 制御キー送信（Ctrl+C, Tab 等）
    SendControlKey(Vec<u8>),
    /// PTY 出力バイト列の受信
    PtyOutputReceived(Vec<u8>),
    /// 入力送信用チャネルの準備完了
    ChannelReady(tokio::sync::mpsc::UnboundedSender<TerminalOut>),
    /// 画面をクリックした（キー入力をこの画面へ送る。選択を始める）
    Clicked,
    /// 画面の上でマウスが動いた（画面の左上からの位置。ドラッグ中は選択を広げる）
    Moved(iced::Point),
    /// 画面でマウスのボタンを離した（選択の終わり）
    Released,
    /// マウスホイールで、履歴（スクロールバック）を動かす
    Scrolled(iced::mouse::ScrollDelta),
    /// 選択した文字をコピーする（ボタン）
    CopyPressed,
    /// クリップボードの文字を貼り付ける（ボタン）
    PastePressed,
    /// 画面の大きさ（ピクセル）が決まった・変わった
    Resized(Size),
    /// WebSocket 接続成功
    Connected,
    /// WebSocket 切断
    Disconnected,
    /// エラー通知
    Error(String),
}

/// ターミナルペインの状態
/// `ESC[行;列f`（HVP）を `ESC[行;列H`（CUP）に読み替える状態機械。
/// 読み込みの区切りをまたぐ制御列にも対応する（途中まで届いた `ESC[` の続きは、次の読み込みで続ける）。
#[derive(Debug, Default)]
pub struct HvpRewriter {
    /// 0: 通常, 1: ESC を受けた, 2: CSI の中（パラメータ、中間文字）
    state: u8,
}

impl HvpRewriter {
    pub fn rewrite(&mut self, input: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(input.len());
        for &b in input {
            match self.state {
                0 => {
                    if b == 0x1b {
                        self.state = 1;
                    }
                    out.push(b);
                }
                1 => {
                    self.state = if b == b'[' { 2 } else { 0 };
                    out.push(b);
                }
                _ => {
                    if (0x40..=0x7e).contains(&b) {
                        // 最後の文字（終端）
                        self.state = 0;
                        out.push(if b == b'f' { b'H' } else { b });
                    } else if b == 0x1b {
                        // 途中で新しい制御列が始まった
                        self.state = 1;
                        out.push(b);
                    } else {
                        out.push(b);
                    }
                }
            }
        }
        out
    }
}

pub struct TerminalPaneState {
    pub parser: vt100::Parser,
    /// `ESC[行;列f`（HVP）を `ESC[行;列H`（CUP）に読み替える。vt100 は `f` を扱わないため（btop などが使う）
    hvp: HvpRewriter,
    pub connected: bool,
    pub connect_requested: bool,
    pub status_text: String,
    pub command_input: String,
    pub rows: u16,
    pub cols: u16,
    /// PTYへ送信する未送信バイトキュー
    pub pending_inputs: Vec<Vec<u8>>,
    /// 接続中の入力送信用チャネル
    pub input_sender: Option<tokio::sync::mpsc::UnboundedSender<TerminalOut>>,
    /// 接続したときの行・桁（接続の URL に入る。接続中に変えると、つなぎ直しになるので、固定する）
    pub connect_rows: u16,
    pub connect_cols: u16,
    /// 画面にキー入力を送っているか（画面をクリックすると true、ほかをクリックすると false）
    pub focused: bool,
    /// 画面の下の入力欄を表示するか（利用者ごと。保存しない）
    pub show_input_line: bool,
    /// 接続先が、同じマシンのシェル（standalone。`true`）か、backend か（`false`）
    pub local: bool,
    /// 変換中の文字（IME の Preedit）。空なら変換していない。画面の、カーソルの位置に下線つきで出す。
    pub preedit: String,
    /// 画面のクリックを、このあとの「どこかをクリックした」の通知から区別するための印
    click_seen: bool,
    /// 選択の始点と終点（行・桁。始点が先とは限らない）
    pub selection: Option<(CellPos, CellPos)>,
    /// マウスのボタンを押したまま、選択を広げている最中か
    dragging: bool,
    /// 画面の上でのマウスの最後の位置（クリックの位置を知るため）
    pointer: Option<iced::Point>,
}

impl Default for TerminalPaneState {
    fn default() -> Self {
        let rows = 24;
        let cols = 80;
        let parser = vt100::Parser::new(rows, cols, 1000);
        let hvp = HvpRewriter::default();
        Self {
            parser,
            hvp,
            connected: false,
            connect_requested: false,
            status_text: String::from("Disconnected (click Connect to start Host shell)"),
            command_input: String::new(),
            rows,
            cols,
            pending_inputs: Vec::new(),
            input_sender: None,
            connect_rows: rows,
            connect_cols: cols,
            focused: false,
            show_input_line: true,
            local: false,
            preedit: String::new(),
            click_seen: false,
            selection: None,
            dragging: false,
            pointer: None,
        }
    }
}

impl TerminalPaneState {
    pub fn new() -> Self {
        Self::default()
    }

    /// PTY からの出力バイト列を処理してセルグリッドを更新
    pub fn process_output(&mut self, data: &[u8]) {
        let before = self.selected_text();
        let data = self.hvp.rewrite(data);
        self.parser.process(&data);
        // 選択した範囲の文字が変わったら（出力が流れたら）、選択を外す
        if before.is_some() && self.selected_text() != before {
            self.selection = None;
            self.dragging = false;
        }
    }

    /// 画面の左上からの位置（ピクセル）にあるセル
    fn cell_at(&self, p: iced::Point) -> CellPos {
        cell_at(p, self.rows, self.cols)
    }

    /// 選択範囲の文字（選択なしのときは `None`）。始点と終点は、どちらが先でもよく、終点のセルも含む。
    pub fn selected_text(&self) -> Option<String> {
        let (a, b) = self.selection?;
        let (start, end) = if a <= b { (a, b) } else { (b, a) };
        let screen = self.parser.screen();
        let text =
            screen.contents_between(start.row, start.col, end.row, (end.col + 1).min(self.cols));
        Some(
            text.lines()
                .map(str::trim_end)
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }

    /// 貼り付ける文字を、端末へ送るバイト列にして送る。ブラケット貼り付けのモードなら ESC[200~ … ESC[201~ で囲み、
    /// そうでなければ、改行を CR にする（端末のキー入力と同じ）。
    pub fn paste(&mut self, text: &str) {
        let bytes = paste_bytes(text, self.parser.screen().bracketed_paste());
        if !bytes.is_empty() {
            self.enqueue_input(bytes);
        }
    }

    /// 日本語入力で変換中か（変換中は、キーを送らない）
    pub fn is_composing(&self) -> bool {
        !self.preedit.is_empty()
    }

    /// 送信用バイト列をエンキューまたは即時送信
    pub fn enqueue_input(&mut self, data: Vec<u8>) {
        // 何か入力したら、いちばん下（今の画面）に戻る
        self.scroll_to_bottom();
        if let Some(ref sender) = self.input_sender {
            let _ = sender.send(TerminalOut::Input(data));
        } else {
            self.pending_inputs.push(data);
        }
    }

    /// 履歴を `lines` 行ぶん、過去へ（負なら、今の画面の方へ）動かす。全画面のプログラム（`less`、`vim`）には履歴がない。
    pub fn scroll_back(&mut self, lines: i32) {
        let screen = self.parser.screen_mut();
        if screen.alternate_screen() {
            return;
        }
        let current = screen.scrollback();
        let target = (current as i64 + i64::from(lines)).max(0) as usize;
        screen.set_scrollback(target);
        if screen.scrollback() != current {
            // 見える文字が動くので、選択は外す
            self.selection = None;
            self.dragging = false;
        }
    }

    /// いちばん下（今の画面）に戻る
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_back(i32::MIN / 2);
    }

    /// いま、過去の履歴を見ているか（今の画面より上へ動かしているか）
    pub fn is_scrolled_back(&self) -> bool {
        self.parser.screen().scrollback() > 0
    }

    /// 送信用キューを取り出す
    pub fn drain_pending_inputs(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.pending_inputs)
    }

    /// 画面の外のどこかをマウスで押した。画面のクリックなら、キー入力は画面へ送り続け、
    /// それ以外なら、送るのをやめる（画面のクリックのメッセージが、先に届く）。
    pub fn mouse_pressed_somewhere(&mut self) {
        if self.click_seen {
            self.click_seen = false;
        } else {
            self.focused = false;
        }
    }

    /// 画面の大きさに合わせて、行と桁を変える。変わったら、端末（PTY）にも伝える。
    fn resize_to(&mut self, size: Size) {
        let (rows, cols) = grid_for(size);
        if (rows, cols) == (self.rows, self.cols) {
            return;
        }
        self.rows = rows;
        self.cols = cols;
        self.parser.screen_mut().set_size(rows, cols);
        self.send_size();
    }

    fn send_size(&self) {
        if let Some(ref sender) = self.input_sender {
            let _ = sender.send(TerminalOut::Resize {
                rows: self.rows,
                cols: self.cols,
            });
        }
    }

    /// UI メッセージの処理
    pub fn update(&mut self, msg: TerminalPaneMessage) {
        match msg {
            TerminalPaneMessage::ConnectPressed => {
                self.local = false;
                self.connect_rows = self.rows;
                self.connect_cols = self.cols;
                self.connect_requested = true;
                self.status_text = String::from("Connecting to PTY session...");
            }
            #[cfg(not(target_arch = "wasm32"))]
            TerminalPaneMessage::ConnectLocalPressed => {
                self.local = true;
                self.connect_rows = self.rows;
                self.connect_cols = self.cols;
                self.connect_requested = true;
                self.status_text = String::from("Starting the local shell...");
            }
            TerminalPaneMessage::DisconnectPressed => {
                self.connect_requested = false;
                self.connected = false;
                self.input_sender = None;
                self.status_text = String::from("Disconnected");
            }
            TerminalPaneMessage::CommandInputChanged(cmd) => {
                self.command_input = cmd;
            }
            TerminalPaneMessage::CommandSubmitted => {
                if !self.command_input.is_empty() {
                    let mut bytes = self.command_input.as_bytes().to_vec();
                    bytes.push(b'\n');
                    self.enqueue_input(bytes);
                    self.command_input.clear();
                } else {
                    self.enqueue_input(vec![b'\n']);
                }
            }
            TerminalPaneMessage::SendControlKey(bytes) => {
                self.enqueue_input(bytes);
            }
            TerminalPaneMessage::PtyOutputReceived(bytes) => {
                self.process_output(&bytes);
            }
            TerminalPaneMessage::ChannelReady(sender) => {
                // 未送信キューをフラッシュ
                for pending in self.drain_pending_inputs() {
                    let _ = sender.send(TerminalOut::Input(pending));
                }
                self.input_sender = Some(sender);
                // つないでいる間に画面の大きさが変わっていたら、伝える
                if (self.rows, self.cols) != (self.connect_rows, self.connect_cols) {
                    self.send_size();
                }
            }
            TerminalPaneMessage::Clicked => {
                // すでにキー入力を送っているときだけ、印を付ける（最初のクリックでは、
                // 「どこかをクリックした」の通知が、まだ購読されていないので、来ない）
                self.click_seen = self.focused;
                self.focused = true;
                // 押した位置から、選択を始める（ボタンを離したとき、動いていなければ選択なしにする）
                self.dragging = true;
                self.selection = self.pointer.map(|p| {
                    let pos = self.cell_at(p);
                    (pos, pos)
                });
            }
            TerminalPaneMessage::Moved(point) => {
                self.pointer = Some(point);
                if self.dragging {
                    let pos = self.cell_at(point);
                    if let Some((_, end)) = self.selection.as_mut() {
                        *end = pos;
                    }
                }
            }
            TerminalPaneMessage::Released => {
                self.dragging = false;
                if let Some((a, b)) = self.selection
                    && a == b
                {
                    self.selection = None;
                }
            }
            // コピーと貼り付けは、クリップボードを使うので、アプリ側（`Yui::update`）で処理する
            TerminalPaneMessage::CopyPressed | TerminalPaneMessage::PastePressed => {}
            TerminalPaneMessage::Scrolled(delta) => {
                let lines = match delta {
                    iced::mouse::ScrollDelta::Lines { y, .. } => y * WHEEL_LINES,
                    iced::mouse::ScrollDelta::Pixels { y, .. } => y / cell_height(),
                };
                self.scroll_back(lines.round() as i32);
            }
            TerminalPaneMessage::Resized(size) => self.resize_to(size),
            TerminalPaneMessage::ToggleInputLine => self.show_input_line = !self.show_input_line,
            TerminalPaneMessage::Ime(event) => match event {
                crate::terminal_widget::ImeEvent::Opened
                | crate::terminal_widget::ImeEvent::Closed => self.preedit.clear(),
                crate::terminal_widget::ImeEvent::Preedit(text) => self.preedit = text,
                crate::terminal_widget::ImeEvent::Commit(text) => {
                    // 確定した文字は、キー入力と同じく、そのままバイト列として送る
                    self.preedit.clear();
                    if self.connected {
                        self.enqueue_input(text.into_bytes());
                    }
                }
            },
            TerminalPaneMessage::Connected => {
                self.connected = true;
                self.connect_requested = true;
                // つないだら、すぐに画面へ直接入力できるように、フォーカスを渡す（押した位置は関係しない）
                self.focused = true;
                self.status_text = String::from(if self.local {
                    "Connected (local shell)"
                } else {
                    "Connected (Host PTY Active)"
                });
            }
            TerminalPaneMessage::Disconnected => {
                self.connected = false;
                self.connect_requested = false;
                self.input_sender = None;
                self.status_text = String::from("Disconnected");
            }
            TerminalPaneMessage::Error(err) => {
                self.connected = false;
                self.connect_requested = false;
                self.input_sender = None;
                self.status_text = format!("Error: {err}");
            }
        }
    }
}

/// vt100::Color を iced::Color に変換する純粋関数
///
/// ANSI標準16色、6x6x6カラーキューブ（16..232）、24段階グレースケール（232..256）、
/// および 24bit RGB (Truecolor) を iced::Color に正確にマッピングします。
pub fn vt100_color_to_iced(color: vt100::Color, default_fg: Color) -> Color {
    match color {
        vt100::Color::Default => default_fg,
        vt100::Color::Rgb(r, g, b) => Color::from_rgb8(r, g, b),
        vt100::Color::Idx(idx) => match idx {
            // ANSI 標準 16色
            0 => Color::from_rgb(0.25, 0.25, 0.25), // Black (暗灰として視認性を確保)
            1 => Color::from_rgb(0.85, 0.20, 0.20), // Red
            2 => Color::from_rgb(0.20, 0.85, 0.20), // Green
            3 => Color::from_rgb(0.85, 0.85, 0.20), // Yellow
            4 => Color::from_rgb(0.30, 0.50, 0.95), // Blue
            5 => Color::from_rgb(0.85, 0.30, 0.85), // Magenta
            6 => Color::from_rgb(0.25, 0.85, 0.85), // Cyan
            7 => Color::from_rgb(0.85, 0.85, 0.85), // White / Light Gray
            8 => Color::from_rgb(0.45, 0.45, 0.45), // Bright Black (Dark Gray)
            9 => Color::from_rgb(1.00, 0.35, 0.35), // Bright Red
            10 => Color::from_rgb(0.35, 1.00, 0.35), // Bright Green
            11 => Color::from_rgb(1.00, 1.00, 0.35), // Bright Yellow
            12 => Color::from_rgb(0.50, 0.70, 1.00), // Bright Blue
            13 => Color::from_rgb(1.00, 0.45, 1.00), // Bright Magenta
            14 => Color::from_rgb(0.45, 1.00, 1.00), // Bright Cyan
            15 => Color::from_rgb(1.00, 1.00, 1.00), // Bright White
            // 6x6x6 カラーキューブ (16..232)
            16..=231 => {
                let index = idx - 16;
                let r = (index / 36) % 6;
                let g = (index / 6) % 6;
                let b = index % 6;
                let conv = |c: u8| {
                    if c == 0 {
                        0.0
                    } else {
                        (c as f32 * 40.0 + 55.0) / 255.0
                    }
                };
                Color::from_rgb(conv(r), conv(g), conv(b))
            }
            // 24段階グレースケール (232..256)
            232..=255 => {
                let gray = ((idx - 232) as f32 * 10.0 + 8.0) / 255.0;
                Color::from_rgb(gray, gray, gray)
            }
        },
    }
}

/// 画面のセルの位置（行・桁）。大小は、行、桁の順
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct CellPos {
    pub row: u16,
    pub col: u16,
}

/// 画面（マウス領域）の左上からの位置を、セルにする。画面の外側（余白）は、端のセルにそろえる。
pub fn cell_at(p: iced::Point, rows: u16, cols: u16) -> CellPos {
    let col = ((p.x - SCREEN_PADDING) / cell_width()).floor().max(0.0) as u16;
    let row = ((p.y - SCREEN_PADDING) / cell_height()).floor().max(0.0) as u16;
    CellPos {
        row: row.min(rows.saturating_sub(1)),
        col: col.min(cols.saturating_sub(1)),
    }
}

/// 貼り付ける文字を、端末へ送るバイト列にする
pub fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    if text.is_empty() {
        return Vec::new();
    }
    if bracketed {
        let mut out = b"\x1b[200~".to_vec();
        out.extend_from_slice(text.as_bytes());
        out.extend_from_slice(b"\x1b[201~");
        out
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    }
}

/// Shift+PageUp / Shift+PageDown（履歴を 1 画面ぶん動かす）。`1` は過去へ、`-1` は今の画面の方へ。
pub fn scroll_key(key: &Key, modifiers: Modifiers) -> Option<i32> {
    if !modifiers.shift() || modifiers.control() || modifiers.alt() || modifiers.logo() {
        return None;
    }
    match key {
        Key::Named(Named::PageUp) => Some(1),
        Key::Named(Named::PageDown) => Some(-1),
        _ => None,
    }
}

/// コピー・貼り付けのキー操作
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClipboardKey {
    Copy,
    Paste,
}

/// Ctrl+Shift+C / V、macOS の Cmd+C / V、Shift+Insert。Ctrl+C は、端末の割り込みなので、コピーにしない。
pub fn clipboard_key(key: &Key, modifiers: Modifiers) -> Option<ClipboardKey> {
    if let Key::Named(Named::Insert) = key {
        return (modifiers.shift() && !modifiers.control()).then_some(ClipboardKey::Paste);
    }
    let Key::Character(c) = key else {
        return None;
    };
    let chord = modifiers.logo() || (modifiers.control() && modifiers.shift());
    if !chord {
        return None;
    }
    match c.to_lowercase().as_str() {
        "c" => Some(ClipboardKey::Copy),
        "v" => Some(ClipboardKey::Paste),
        _ => None,
    }
}

/// セル (row, col) が、選択範囲（始点が先）に入っているか
fn in_selection(selection: Option<(CellPos, CellPos)>, row: u16, col: u16) -> bool {
    let Some((start, end)) = selection else {
        return false;
    };
    let p = CellPos { row, col };
    start <= p && p <= end
}

/// 1 セルの見た目（前景・背景・太字・下線）。同じ見た目が続く文字は 1 つのスパンにまとめる。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CellStyle {
    pub fg: Color,
    /// `None` は端末の背景色のまま（スパンに背景を付けない）
    pub bg: Option<Color>,
    pub bold: bool,
    pub underline: bool,
}

impl CellStyle {
    pub fn plain(fg: Color) -> Self {
        Self {
            fg,
            bg: None,
            bold: false,
            underline: false,
        }
    }

    /// vt100 のセルから見た目を決める。反転（SGR 7）は前景と背景を入れ替える。
    pub fn of(cell: &vt100::Cell, default_fg: Color, default_bg: Color) -> Self {
        let fg = vt100_color_to_iced(cell.fgcolor(), default_fg);
        let bg = match cell.bgcolor() {
            vt100::Color::Default => None,
            c => Some(vt100_color_to_iced(c, default_bg)),
        };
        let (fg, bg) = if cell.inverse() {
            (bg.unwrap_or(default_bg), Some(fg))
        } else {
            (fg, bg)
        };
        Self {
            fg,
            bg,
            bold: cell.bold(),
            underline: cell.underline(),
        }
    }
}

impl TerminalPaneState {
    /// ターミナルペインの UI 構築
    pub fn view(&self) -> Element<'_, TerminalPaneMessage> {
        // 1. 上部コントロールバー
        let connect_button = if self.connected {
            button(text("Disconnect").size(12))
                .style(crate::ui_style::secondary_button)
                .on_press(TerminalPaneMessage::DisconnectPressed)
        } else {
            button(text("Connect (Host)").size(12))
                .style(crate::ui_style::secondary_button)
                .on_press(TerminalPaneMessage::ConnectPressed)
        };

        let status_label = text(&self.status_text).size(12);
        let input_line_button = button(
            text(if self.show_input_line {
                "Hide input line"
            } else {
                "Show input line"
            })
            .size(12),
        )
        .style(crate::ui_style::secondary_button)
        .on_press(TerminalPaneMessage::ToggleInputLine);

        let connect_controls = row![connect_button].spacing(6);
        // standalone（デスクトップ版）では、同じマシンのシェルにもつなげる
        #[cfg(not(target_arch = "wasm32"))]
        let connect_controls = if self.connected {
            connect_controls
        } else {
            connect_controls.push(
                button(text("Connect (Local shell)").size(12))
                    .style(crate::ui_style::secondary_button)
                    .on_press(TerminalPaneMessage::ConnectLocalPressed),
            )
        };

        let mut toolbar = row![connect_controls, input_line_button, status_label]
            .spacing(8)
            .align_y(iced::Alignment::Center);
        if self.is_scrolled_back() {
            toolbar = toolbar.push(
                text(format!(
                    "History: {} lines up (type, or Shift+PageDown, to return)",
                    self.parser.screen().scrollback()
                ))
                .size(12)
                .color(palette().terminal_cursor),
            );
        }

        // 2. セルグリッド画面
        let screen = self.parser.screen();
        let (cursor_row, cursor_col) = screen.cursor_position();
        // 履歴を見ているときは、カーソルを描かない（カーソルは、今の画面のもの）
        // 履歴を見ているとき、と、プログラムがカーソルを隠しているとき（`ESC[?25l`。btop など）は、カーソルを描かない
        let show_cursor = self.connected && !self.is_scrolled_back() && !screen.hide_cursor();

        let selection = self
            .selection
            .map(|(a, b)| if a <= b { (a, b) } else { (b, a) });
        let default_fg = palette().terminal_fg;
        let default_bg = palette().terminal_bg;
        // 変換中は、カーソルのブロックを出さず、変換中の文字を、カーソルの位置に下線つきで描く
        let composing = !self.preedit.is_empty();
        let mut grid_rows: Vec<Vec<GridCell>> = Vec::with_capacity(usize::from(self.rows));
        for r in 0..self.rows {
            let mut row_cells: Vec<GridCell> = Vec::with_capacity(usize::from(self.cols));
            for c in 0..self.cols {
                let is_cursor = show_cursor && !composing && r == cursor_row && c == cursor_col;
                let (text, mut style, wide) = match screen.cell(r, c) {
                    // 全角の 2 列目は、1 列目の字に含まれる（描かない）
                    Some(cell) if cell.is_wide_continuation() => {
                        row_cells.push(GridCell {
                            text: String::new(),
                            fg: default_fg,
                            bg: None,
                            bold: false,
                            underline: false,
                            wide: false,
                        });
                        continue;
                    }
                    Some(cell) => {
                        let contents = cell.contents();
                        let text = if contents.is_empty() {
                            " ".to_string()
                        } else {
                            contents.to_string()
                        };
                        (
                            text,
                            CellStyle::of(cell, default_fg, default_bg),
                            cell.is_wide(),
                        )
                    }
                    None => (" ".to_string(), CellStyle::plain(default_fg), false),
                };
                if in_selection(selection, r, c) {
                    style.bg = Some(palette().selection);
                    style.fg = default_bg;
                }
                if is_cursor {
                    style.bg = Some(palette().terminal_cursor);
                    style.fg = default_bg;
                }
                row_cells.push(GridCell {
                    text,
                    fg: style.fg,
                    bg: style.bg,
                    bold: style.bold || is_cursor,
                    underline: style.underline,
                    wide,
                });
            }
            grid_rows.push(row_cells);
        }

        // 変換中の文字を、カーソルの位置から並べる（全角は 2 列）
        if composing && cursor_row < self.rows {
            use unicode_width::UnicodeWidthChar;
            let row = &mut grid_rows[usize::from(cursor_row)];
            let mut col = usize::from(cursor_col);
            for ch in self.preedit.chars() {
                if col >= row.len() {
                    break;
                }
                let width = ch.width().unwrap_or(1).max(1);
                row[col] = GridCell {
                    text: ch.to_string(),
                    fg: default_fg,
                    bg: Some(default_bg),
                    bold: false,
                    underline: true,
                    wide: width == 2,
                };
                if width == 2 && col + 1 < row.len() {
                    row[col + 1] = GridCell {
                        text: String::new(),
                        fg: default_fg,
                        bg: Some(default_bg),
                        bold: false,
                        underline: false,
                        wide: false,
                    };
                }
                col += width;
            }
        }

        let grid = TerminalGrid {
            rows: grid_rows,
            cell_width: cell_width(),
            cell_height: cell_height(),
            font_size: crate::fonts::terminal_font_size(),
            regular: crate::fonts::mono_font(false),
            bold: crate::fonts::mono_font(true),
            wide: crate::fonts::cjk_font(),
        };

        let focused = self.focused;
        let terminal_screen = container(grid)
            .width(Fill)
            .height(Fill)
            .style(move |_theme| container::Style {
                background: Some(palette().terminal_bg.into()),
                text_color: Some(palette().terminal_fg),
                // キー入力を送っているときは、枠の色を変える
                border: if focused {
                    iced::Border {
                        color: palette().terminal_cursor,
                        width: 1.5,
                        radius: 4.0.into(),
                    }
                } else {
                    iced::border::rounded(4)
                },
                ..Default::default()
            })
            .padding(SCREEN_PADDING);
        // 大きさを測り（行・桁を決める）、クリックでキー入力を受け取る
        // 変換の窓を、カーソルの位置に合わせる（画面の余白の中の、セルの位置）
        let ime_cursor = iced::Rectangle::new(
            iced::Point::new(
                SCREEN_PADDING + f32::from(cursor_col) * cell_width(),
                SCREEN_PADDING + f32::from(cursor_row) * cell_height(),
            ),
            Size::new(cell_width(), cell_height()),
        );
        let terminal_screen = crate::terminal_widget::TerminalScreen::new(
            mouse_area(
                sensor(terminal_screen)
                    .on_show(TerminalPaneMessage::Resized)
                    .on_resize(TerminalPaneMessage::Resized),
            )
            .on_press(TerminalPaneMessage::Clicked)
            .on_move(TerminalPaneMessage::Moved)
            .on_release(TerminalPaneMessage::Released)
            .on_scroll(TerminalPaneMessage::Scrolled),
        )
        .with_ime(
            focused && self.connected,
            ime_cursor,
            TerminalPaneMessage::Ime,
        );

        // 3. 下部入力バー & クイックキー
        let input_field = text_input("Type command here and press Enter...", &self.command_input)
            .on_input(TerminalPaneMessage::CommandInputChanged)
            .on_submit(TerminalPaneMessage::CommandSubmitted)
            .size(12)
            .width(Fill);

        let enter_btn = button(text("Send ⏎").size(12))
            .style(crate::ui_style::secondary_button)
            .on_press(TerminalPaneMessage::CommandSubmitted);

        let ctrl_c_btn = button(text("Ctrl+C").size(12))
            .style(crate::ui_style::secondary_button)
            .on_press(TerminalPaneMessage::SendControlKey(vec![0x03]));

        let tab_btn = button(text("Tab ⇥").size(12))
            .style(crate::ui_style::secondary_button)
            .on_press(TerminalPaneMessage::SendControlKey(vec![0x09]));

        let copy_btn = button(text("Copy").size(12))
            .style(crate::ui_style::secondary_button)
            .on_press_maybe(
                self.selection
                    .is_some()
                    .then_some(TerminalPaneMessage::CopyPressed),
            );
        let paste_btn = button(text("Paste").size(12))
            .style(crate::ui_style::secondary_button)
            .on_press(TerminalPaneMessage::PastePressed);

        // 入力欄を隠しているときは、入力欄と Enter を外す（Ctrl+C、Tab、Copy、Paste は残す）
        let bottom_bar = if self.show_input_line {
            row![
                input_field,
                enter_btn,
                ctrl_c_btn,
                tab_btn,
                copy_btn,
                paste_btn
            ]
        } else {
            row![ctrl_c_btn, tab_btn, copy_btn, paste_btn]
        }
        .spacing(6)
        .align_y(iced::Alignment::Center);

        column![toolbar, terminal_screen, bottom_bar]
            .spacing(4)
            .width(Fill)
            .height(Fill)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(c: &str) -> Key {
        Key::Character(c.into())
    }

    fn bytes(key: &Key, mods: Modifiers) -> Option<Vec<u8>> {
        key_bytes(key, mods, None, false)
    }

    #[test]
    fn test_key_bytes_for_text_control_and_named_keys() {
        let none = Modifiers::empty();
        // 文字は、シフトなどを反映した `text` を優先する
        assert_eq!(
            key_bytes(&key("a"), Modifiers::SHIFT, Some("A"), false),
            Some(b"A".to_vec())
        );
        assert_eq!(bytes(&key("a"), none), Some(b"a".to_vec()));
        // 日本語など
        assert_eq!(
            key_bytes(&key("あ"), none, Some("あ"), false),
            Some("あ".as_bytes().to_vec())
        );
        // Ctrl+文字 は制御文字、Alt+文字 は ESC つき
        assert_eq!(bytes(&key("c"), Modifiers::CTRL), Some(vec![0x03]));
        assert_eq!(bytes(&key("d"), Modifiers::CTRL), Some(vec![0x04]));
        assert_eq!(bytes(&key("["), Modifiers::CTRL), Some(vec![0x1b]));
        assert_eq!(bytes(&key("b"), Modifiers::ALT), Some(b"\x1bb".to_vec()));
        // Cmd は送らない。Ctrl+Shift は、コピー・貼り付けのために空けてある
        assert_eq!(bytes(&key("c"), Modifiers::LOGO), None);
        assert_eq!(bytes(&key("c"), Modifiers::CTRL | Modifiers::SHIFT), None);
        // 名前のあるキー
        assert_eq!(bytes(&Key::Named(Named::Enter), none), Some(b"\r".to_vec()));
        assert_eq!(bytes(&Key::Named(Named::Backspace), none), Some(vec![0x7f]));
        assert_eq!(bytes(&Key::Named(Named::Tab), none), Some(b"\t".to_vec()));
        assert_eq!(
            bytes(&Key::Named(Named::Tab), Modifiers::SHIFT),
            Some(b"\x1b[Z".to_vec())
        );
        assert_eq!(bytes(&Key::Named(Named::Escape), none), Some(vec![0x1b]));
        assert_eq!(bytes(&Key::Named(Named::Space), none), Some(b" ".to_vec()));
        assert_eq!(
            bytes(&Key::Named(Named::Delete), none),
            Some(b"\x1b[3~".to_vec())
        );
        assert_eq!(
            bytes(&Key::Named(Named::PageUp), none),
            Some(b"\x1b[5~".to_vec())
        );
        assert_eq!(
            bytes(&Key::Named(Named::F5), none),
            Some(b"\x1b[15~".to_vec())
        );
        // 修飾キーだけは送らない
        assert_eq!(bytes(&Key::Named(Named::Shift), Modifiers::SHIFT), None);
    }

    #[test]
    fn test_arrow_keys_follow_the_application_cursor_mode() {
        let up = Key::Named(Named::ArrowUp);
        assert_eq!(
            key_bytes(&up, Modifiers::empty(), None, false),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            key_bytes(&up, Modifiers::empty(), None, true),
            Some(b"\x1bOA".to_vec())
        );
        // vt100 が、アプリケーションカーソルキーのモードを覚えている（vim や less が入れる）
        let mut state = TerminalPaneState::new();
        assert!(!state.parser.screen().application_cursor());
        state.process_output(b"\x1b[?1h");
        assert!(state.parser.screen().application_cursor());
    }

    #[test]
    fn test_grid_for_fits_the_screen_and_clamps() {
        // 大きな画面ほど、行・桁が増える
        let (r1, c1) = grid_for(Size::new(800.0, 400.0));
        let (r2, c2) = grid_for(Size::new(1600.0, 800.0));
        assert!(r2 > r1 && c2 > c1);
        // 文字の大きさ 12 px の等幅（7.2 px × 16.6 px）で、はみ出さない
        assert!(f32::from(c1) * cell_width() <= 800.0);
        assert!(f32::from(r1) * cell_height() <= 400.0);
        // 極端に小さい・大きいときは、範囲に収める
        assert_eq!(grid_for(Size::new(0.0, 0.0)), (2, 20));
        assert_eq!(grid_for(Size::new(100000.0, 100000.0)), (200, 500));
    }

    #[test]
    fn test_resize_changes_the_screen_and_tells_the_pty_once() {
        let mut state = TerminalPaneState::new();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        state.update(TerminalPaneMessage::ChannelReady(tx));
        // 接続したときと同じ大きさなら、何も送らない
        assert!(rx.try_recv().is_err());

        let size = Size::new(900.0, 500.0);
        let (rows, cols) = grid_for(size);
        state.update(TerminalPaneMessage::Resized(size));
        assert_eq!((state.rows, state.cols), (rows, cols));
        assert_eq!(state.parser.screen().size(), (rows, cols));
        assert_eq!(rx.try_recv(), Ok(TerminalOut::Resize { rows, cols }));
        // 同じ大きさがもう一度来ても、送らない
        state.update(TerminalPaneMessage::Resized(size));
        assert!(rx.try_recv().is_err());
        // 接続の URL に入る大きさは、変えない（変えると、つなぎ直しになる）
        assert_eq!((state.connect_rows, state.connect_cols), (24, 80));
    }

    #[test]
    fn test_resize_json_matches_the_backend_message() {
        assert_eq!(
            TerminalOut::resize_json(30, 100),
            r#"{"type":"resize","rows":30,"cols":100}"#
        );
    }

    #[test]
    fn test_connecting_takes_the_keyboard_and_the_input_line_toggles() {
        let mut state = TerminalPaneState::new();
        assert!(state.show_input_line, "the input line is shown by default");
        assert!(!state.focused);
        state.update(TerminalPaneMessage::Connected);
        assert!(state.focused, "a new connection gets the keyboard at once");
        state.update(TerminalPaneMessage::ToggleInputLine);
        assert!(!state.show_input_line);
        state.update(TerminalPaneMessage::ToggleInputLine);
        assert!(state.show_input_line);
    }

    #[test]
    fn test_ime_preedit_is_shown_and_the_commit_is_sent_to_the_shell() {
        use crate::terminal_widget::ImeEvent;
        let mut state = TerminalPaneState::new();
        state.update(TerminalPaneMessage::Connected);
        state.update(TerminalPaneMessage::Ime(ImeEvent::Preedit("にほん".into())));
        assert!(state.is_composing(), "the preedit is shown while composing");
        state.update(TerminalPaneMessage::Ime(ImeEvent::Commit("日本".into())));
        assert!(!state.is_composing());
        assert_eq!(
            state.drain_pending_inputs(),
            vec!["日本".as_bytes().to_vec()]
        );
    }

    #[test]
    fn test_ime_commit_is_dropped_when_not_connected() {
        use crate::terminal_widget::ImeEvent;
        let mut state = TerminalPaneState::new();
        state.update(TerminalPaneMessage::Ime(ImeEvent::Commit("a".into())));
        assert!(state.drain_pending_inputs().is_empty());
        assert!(!state.is_composing());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn test_local_shell_and_backend_connections_are_told_apart() {
        let mut state = TerminalPaneState::new();
        state.update(TerminalPaneMessage::ConnectLocalPressed);
        assert!(state.connect_requested && state.local);
        state.update(TerminalPaneMessage::Connected);
        assert_eq!(state.status_text, "Connected (local shell)");
        state.update(TerminalPaneMessage::DisconnectPressed);
        state.update(TerminalPaneMessage::ConnectPressed);
        assert!(state.connect_requested && !state.local);
    }

    #[test]
    fn test_focus_follows_clicks() {
        let mut state = TerminalPaneState::new();
        assert!(!state.focused);
        // 最初のクリック: 画面にキー入力を送り始める。「どこかをクリックした」の通知はまだ来ない
        state.update(TerminalPaneMessage::Clicked);
        assert!(state.focused);
        // 画面をもう一度クリック: 画面のクリックのメッセージが先、「どこかをクリックした」が後
        state.update(TerminalPaneMessage::Clicked);
        state.mouse_pressed_somewhere();
        assert!(state.focused);
        // ほかをクリック: やめる
        state.mouse_pressed_somewhere();
        assert!(!state.focused);
    }

    #[test]
    fn test_vt100_parser_and_screen_state() {
        let mut state = TerminalPaneState::new();
        assert_eq!(state.rows, 24);
        assert_eq!(state.cols, 80);

        // 1. テキスト出力
        state.process_output(b"Hello from YUI Terminal\r\nLine 2");
        let screen = state.parser.screen();
        let contents = screen.contents();
        assert!(contents.contains("Hello from YUI Terminal"));
        assert!(contents.contains("Line 2"));

        // 2. ANSI エスケープシーケンス（カーソル移動・クリア）
        // 画面クリア \x1b[2J
        state.process_output(b"\x1b[2J\x1b[1;1HNew Cleared Line");
        let screen2 = state.parser.screen();
        let contents2 = screen2.contents();
        assert!(contents2.contains("New Cleared Line"));
    }

    #[test]
    fn test_terminal_pane_input_queue() {
        let mut state = TerminalPaneState::new();
        state.enqueue_input(b"ls -la\n".to_vec());
        state.enqueue_input(vec![0x03]); // Ctrl+C

        let queued = state.drain_pending_inputs();
        assert_eq!(queued.len(), 2);
        assert_eq!(queued[0], b"ls -la\n");
        assert_eq!(queued[1], vec![0x03]);

        // drain 後は空
        assert!(state.drain_pending_inputs().is_empty());
    }

    #[test]
    fn test_vt100_color_to_iced_mapping() {
        let default_fg = Color::from_rgb(0.9, 0.9, 0.9);

        // 1. Default
        assert_eq!(
            vt100_color_to_iced(vt100::Color::Default, default_fg),
            default_fg
        );

        // 2. ANSI 16色 (Red = 1, Green = 2, Bright Red = 9)
        let red = vt100_color_to_iced(vt100::Color::Idx(1), default_fg);
        assert!(red.r > 0.8 && red.g < 0.3 && red.b < 0.3);

        let green = vt100_color_to_iced(vt100::Color::Idx(2), default_fg);
        assert!(green.g > 0.8 && green.r < 0.3 && green.b < 0.3);

        let bright_red = vt100_color_to_iced(vt100::Color::Idx(9), default_fg);
        assert_eq!(bright_red.r, 1.0);

        // 3. 6x6x6 カラーキューブ (16..232)
        // idx = 16 -> r=0, g=0, b=0 -> black
        let cube_0 = vt100_color_to_iced(vt100::Color::Idx(16), default_fg);
        assert_eq!(cube_0.r, 0.0);
        assert_eq!(cube_0.g, 0.0);
        assert_eq!(cube_0.b, 0.0);

        // 4. グレースケール (232..256)
        let gray = vt100_color_to_iced(vt100::Color::Idx(240), default_fg);
        assert_eq!(gray.r, gray.g);
        assert_eq!(gray.g, gray.b);

        // 5. RGB (Truecolor)
        let truecolor = vt100_color_to_iced(vt100::Color::Rgb(128, 64, 32), default_fg);
        assert_eq!(truecolor, Color::from_rgb8(128, 64, 32));
    }

    #[test]
    fn test_colored_terminal_screen_and_attributes() {
        let mut state = TerminalPaneState::new();

        // 赤テキスト "ERROR: " + 緑テキスト "OK" + リセット
        state.process_output(b"\x1b[31mERROR: \x1b[32mOK\x1b[0m\n");
        let screen = state.parser.screen();

        // "ERROR: " の最初の文字 'E' は赤色 (Idx(1))
        let cell_e = screen.cell(0, 0).expect("cell (0, 0) should exist");
        assert_eq!(cell_e.contents(), "E");
        assert_eq!(cell_e.fgcolor(), vt100::Color::Idx(1));

        // "OK" の 'O' (列7) は緑色 (Idx(2))
        let cell_o = screen.cell(0, 7).expect("cell (0, 7) should exist");
        assert_eq!(cell_o.contents(), "O");
        assert_eq!(cell_o.fgcolor(), vt100::Color::Idx(2));
    }

    #[test]
    fn test_cell_style_background_inverse_and_underline() {
        let default_fg = Color::WHITE;
        let default_bg = Color::BLACK;
        let mut parser = vt100::Parser::new(2, 20, 0);
        parser.process(b"a\x1b[41mb\x1b[0m\x1b[7mc\x1b[0m\x1b[31;7md\x1b[0m\x1b[4me");
        let s = parser.screen();
        let st = |c| CellStyle::of(s.cell(0, c).unwrap(), default_fg, default_bg);
        assert_eq!(st(0), CellStyle::plain(default_fg));
        let red = vt100_color_to_iced(vt100::Color::Idx(1), default_fg);
        assert_eq!(st(1).bg, Some(red));
        assert_eq!(st(1).fg, default_fg);
        // 反転: 前景と背景（既定色）を入れ替える
        assert_eq!((st(2).fg, st(2).bg), (default_bg, Some(default_fg)));
        // 赤文字の反転: 赤い背景に既定の背景色の文字
        assert_eq!((st(3).fg, st(3).bg), (default_bg, Some(red)));
        assert!(st(4).underline && !st(0).underline);
    }

    fn at(row: u16, col: u16) -> iced::Point {
        // セルの真ん中
        iced::Point::new(
            SCREEN_PADDING + (f32::from(col) + 0.5) * cell_width(),
            SCREEN_PADDING + (f32::from(row) + 0.5) * cell_height(),
        )
    }

    fn drag(state: &mut TerminalPaneState, from: (u16, u16), to: (u16, u16)) {
        state.update(TerminalPaneMessage::Moved(at(from.0, from.1)));
        state.update(TerminalPaneMessage::Clicked);
        state.update(TerminalPaneMessage::Moved(at(to.0, to.1)));
        state.update(TerminalPaneMessage::Released);
    }

    #[test]
    fn test_cell_at_maps_pixels_and_clamps() {
        assert_eq!(cell_at(at(2, 5), 24, 80), CellPos { row: 2, col: 5 });
        // 余白・画面の外は端のセル
        assert_eq!(
            cell_at(iced::Point::new(-5.0, -5.0), 24, 80),
            CellPos { row: 0, col: 0 }
        );
        assert_eq!(
            cell_at(iced::Point::new(9999.0, 9999.0), 24, 80),
            CellPos { row: 23, col: 79 }
        );
    }

    #[test]
    fn test_drag_selects_text_and_click_clears_it() {
        let mut state = TerminalPaneState::new();
        state.process_output(b"hello world\r\nsecond line");
        drag(&mut state, (0, 6), (0, 10));
        assert_eq!(state.selected_text().as_deref(), Some("world"));
        // 右下から左上へ引いても同じ
        drag(&mut state, (0, 10), (0, 6));
        assert_eq!(state.selected_text().as_deref(), Some("world"));
        // 複数行（行末の空白は落とす）
        drag(&mut state, (0, 6), (1, 5));
        assert_eq!(state.selected_text().as_deref(), Some("world\nsecond"));
        // 動かさないクリックは、選択なし
        drag(&mut state, (0, 3), (0, 3));
        assert_eq!(state.selected_text(), None);
    }

    #[test]
    fn test_selection_is_dropped_when_the_selected_text_changes() {
        let mut state = TerminalPaneState::new();
        state.process_output(b"hello world");
        drag(&mut state, (0, 0), (0, 4));
        // 選択の外に出力が増えても、そのまま
        state.process_output(b"\r\nmore");
        assert_eq!(state.selected_text().as_deref(), Some("hello"));
        // 選択した文字が変わったら、外す
        state.process_output(b"\x1b[1;1HJ");
        assert_eq!(state.selection, None);
    }

    #[test]
    fn test_paste_bytes_and_bracketed_mode() {
        assert_eq!(paste_bytes("a\nb\r\nc", false), b"a\rb\rc");
        assert_eq!(paste_bytes("a\nb", true), b"\x1b[200~a\nb\x1b[201~");
        assert!(paste_bytes("", true).is_empty());
        let mut state = TerminalPaneState::new();
        state.paste("ls\n");
        assert_eq!(state.drain_pending_inputs(), vec![b"ls\r".to_vec()]);
        state.process_output(b"\x1b[?2004h");
        state.paste("x");
        assert_eq!(
            state.drain_pending_inputs(),
            vec![b"\x1b[200~x\x1b[201~".to_vec()]
        );
    }

    #[test]
    fn test_clipboard_keys_leave_ctrl_c_alone() {
        let ctrl_shift = Modifiers::CTRL | Modifiers::SHIFT;
        assert_eq!(
            clipboard_key(&key("C"), ctrl_shift),
            Some(ClipboardKey::Copy)
        );
        assert_eq!(
            clipboard_key(&key("v"), ctrl_shift),
            Some(ClipboardKey::Paste)
        );
        assert_eq!(
            clipboard_key(&key("c"), Modifiers::LOGO),
            Some(ClipboardKey::Copy)
        );
        assert_eq!(
            clipboard_key(&Key::Named(Named::Insert), Modifiers::SHIFT),
            Some(ClipboardKey::Paste)
        );
        // Ctrl+C は割り込みなので、コピーにしない
        assert_eq!(clipboard_key(&key("c"), Modifiers::CTRL), None);
        assert_eq!(clipboard_key(&key("c"), Modifiers::empty()), None);
    }

    #[test]
    fn test_scrollback_shows_old_lines_and_typing_returns() {
        let mut state = TerminalPaneState::new();
        for i in 0..60 {
            state.process_output(format!("line{i}\r\n").as_bytes());
        }
        let top = |s: &TerminalPaneState| {
            s.parser
                .screen()
                .contents()
                .lines()
                .next()
                .unwrap()
                .to_string()
        };
        assert!(!state.is_scrolled_back());
        let live = top(&state);
        state.scroll_back(10);
        assert!(state.is_scrolled_back());
        assert_ne!(top(&state), live);
        // 出力が増えても、見ている行は動かない
        let seen = top(&state);
        state.process_output(b"more\r\n");
        assert_eq!(top(&state), seen);
        // 履歴の端で止まり、負の値で戻る
        state.scroll_back(100_000);
        assert_eq!(top(&state), "line0");
        state.scroll_back(-100_000);
        assert!(!state.is_scrolled_back());
        // 入力すると、いちばん下に戻る
        state.scroll_back(5);
        state.enqueue_input(b"x".to_vec());
        assert!(!state.is_scrolled_back());
    }

    #[test]
    fn test_scrollback_ignores_full_screen_programs_and_drops_the_selection() {
        let mut state = TerminalPaneState::new();
        for i in 0..40 {
            state.process_output(format!("l{i}\r\n").as_bytes());
        }
        state.selection = Some((CellPos { row: 0, col: 0 }, CellPos { row: 0, col: 1 }));
        state.scroll_back(3);
        assert_eq!(state.selection, None);
        state.scroll_to_bottom();
        state.process_output(b"\x1b[?1049h");
        state.scroll_back(3);
        assert!(!state.is_scrolled_back());
    }

    #[test]
    fn test_wheel_and_scroll_keys() {
        let mut state = TerminalPaneState::new();
        for i in 0..60 {
            state.process_output(format!("l{i}\r\n").as_bytes());
        }
        state.update(TerminalPaneMessage::Scrolled(
            iced::mouse::ScrollDelta::Lines { x: 0.0, y: 2.0 },
        ));
        assert_eq!(state.parser.screen().scrollback(), 6);
        state.update(TerminalPaneMessage::Scrolled(
            iced::mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
        ));
        assert_eq!(state.parser.screen().scrollback(), 3);
        assert_eq!(
            scroll_key(&Key::Named(Named::PageUp), Modifiers::SHIFT),
            Some(1)
        );
        assert_eq!(
            scroll_key(&Key::Named(Named::PageDown), Modifiers::SHIFT),
            Some(-1)
        );
        // Shift なしの PageUp は、プログラム（`less` など）へ送る
        assert_eq!(
            scroll_key(&Key::Named(Named::PageUp), Modifiers::empty()),
            None
        );
    }
}

#[cfg(test)]
mod hvp_tests {
    use super::HvpRewriter;

    #[test]
    fn test_hvp_becomes_cup() {
        let mut r = HvpRewriter::default();
        assert_eq!(r.rewrite(b"\x1b[2;3fX"), b"\x1b[2;3HX".to_vec());
        assert_eq!(r.rewrite(b"\x1b[10;127f"), b"\x1b[10;127H".to_vec());
        // 他の制御列と、CSI でない `ESC f` は変えない
        assert_eq!(
            r.rewrite(b"\x1b[2;3H\x1b[31m\x1bf"),
            b"\x1b[2;3H\x1b[31m\x1bf".to_vec()
        );
    }

    #[test]
    fn test_split_sequences_are_rewritten() {
        let mut r = HvpRewriter::default();
        let mut out = Vec::new();
        for chunk in [&b"ab\x1b"[..], b"[1", b";4", b"f"] {
            out.extend(r.rewrite(chunk));
        }
        assert_eq!(out, b"ab\x1b[1;4H".to_vec());
    }
}
