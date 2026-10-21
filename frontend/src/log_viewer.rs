//! ログビューワー UI および状態管理モジュール
//!
//! architecture.md 13.3節・13.4節に基づき、tail -f 風の自動スクロール、
//! リングバッファ（上限行数）、部分文字列フィルタリング、ファイル監視接続を提供します。

use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Element, Fill, Length};
use serde::{Deserialize, Serialize};

/// ログストリーミングメッセージ（backend と共通仕様）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum LogStreamMessage {
    Connected { path: String, file_exists: bool },
    Lines(Vec<String>),
    Waiting { path: String },
    Error(String),
}

/// ログビューワーの UI メッセージ
#[derive(Debug, Clone)]
pub enum LogViewerMessage {
    PathInputChanged(String),
    ConnectPressed,
    DisconnectPressed,
    FilterChanged(String),
    ToggleAutoScroll,
    ClearPressed,
    StreamMessageReceived(LogStreamMessage),
    ConnectionClosed,
}

/// クライアント側で保持する最大行数
pub const DEFAULT_MAX_LOG_LINES: usize = 5000;

/// ログビューワーの状態
#[derive(Debug, Clone)]
pub struct LogViewerState {
    pub lines: Vec<String>,
    pub max_lines: usize,
    pub filter: String,
    pub auto_scroll: bool,
    pub path_input: String,
    pub active_log_path: Option<String>,
    pub status_text: String,
}

impl Default for LogViewerState {
    fn default() -> Self {
        Self {
            lines: Vec::new(),
            max_lines: DEFAULT_MAX_LOG_LINES,
            filter: String::new(),
            auto_scroll: true,
            path_input: String::from("job.log"),
            active_log_path: None,
            status_text: String::from("Idle (enter log file path and click Connect)"),
        }
    }
}

impl LogViewerState {
    pub fn new() -> Self {
        Self::default()
    }

    /// 行をリングバッファに追加し、上限を超えた古い行を破棄する
    pub fn push_lines(&mut self, new_lines: Vec<String>) {
        if new_lines.is_empty() {
            return;
        }

        self.lines.extend(new_lines);
        if self.lines.len() > self.max_lines {
            let overflow = self.lines.len() - self.max_lines;
            self.lines.drain(0..overflow);
        }
    }

    pub fn clear(&mut self) {
        self.lines.clear();
    }

    /// フィルタ文字列に一致する行のイテレータ
    pub fn filtered_lines(&self) -> impl Iterator<Item = (usize, &str)> {
        let filter = self.filter.trim();
        self.lines
            .iter()
            .enumerate()
            .filter(move |(_idx, line)| {
                if filter.is_empty() {
                    true
                } else {
                    line.contains(filter)
                }
            })
            .map(|(idx, line)| (idx, line.as_str()))
    }

    /// WebSocket メッセージを受信して状態を更新
    pub fn handle_message(&mut self, message: LogViewerMessage) {
        match message {
            LogViewerMessage::PathInputChanged(p) => {
                self.path_input = p;
            }
            LogViewerMessage::ConnectPressed => {
                let trimmed = self.path_input.trim();
                if !trimmed.is_empty() {
                    self.active_log_path = Some(trimmed.to_string());
                    self.status_text = format!("Connecting to '{}'...", trimmed);
                }
            }
            LogViewerMessage::DisconnectPressed => {
                self.active_log_path = None;
                self.status_text = String::from("Disconnected");
            }
            LogViewerMessage::FilterChanged(f) => {
                self.filter = f;
            }
            LogViewerMessage::ToggleAutoScroll => {
                self.auto_scroll = !self.auto_scroll;
            }
            LogViewerMessage::ClearPressed => {
                self.clear();
            }
            LogViewerMessage::StreamMessageReceived(stream_msg) => match stream_msg {
                LogStreamMessage::Connected { path, file_exists } => {
                    self.status_text = if file_exists {
                        format!("Connected: watching '{}'", path)
                    } else {
                        format!("Watching '{}' (waiting for file creation)", path)
                    };
                }
                LogStreamMessage::Waiting { path } => {
                    self.status_text = format!("Waiting for '{}' to be created...", path);
                }
                LogStreamMessage::Lines(lines) => {
                    self.push_lines(lines);
                    if let Some(ref path) = self.active_log_path {
                        self.status_text =
                            format!("Watching '{}' ({} lines in buffer)", path, self.lines.len());
                    }
                }
                LogStreamMessage::Error(err) => {
                    self.status_text = format!("Error: {}", err);
                }
            },
            LogViewerMessage::ConnectionClosed => {
                self.status_text = String::from("Stream closed / Disconnected");
            }
        }
    }

    /// ログビューワー UI の描画
    pub fn view<'a>(&'a self) -> Element<'a, LogViewerMessage> {
        // ツールバー
        let is_connected = self.active_log_path.is_some();
        let connect_button = if is_connected {
            button(text("Disconnect").size(12))
                .padding([3, 10])
                .style(crate::ui_style::secondary_button)
                .on_press(LogViewerMessage::DisconnectPressed)
        } else {
            button(text("Connect").size(12))
                .padding([3, 10])
                .style(crate::ui_style::secondary_button)
                .on_press(LogViewerMessage::ConnectPressed)
        };

        let auto_scroll_label = if self.auto_scroll {
            "Auto Scroll: ON"
        } else {
            "Auto Scroll: OFF"
        };

        let toolbar = row![
            text("Path:")
                .size(12)
                .color(crate::ui_style::palette().text_subtle),
            text_input("e.g. job.log", &self.path_input)
                .on_input(LogViewerMessage::PathInputChanged)
                .size(12)
                .width(Length::Fixed(200.0)),
            connect_button,
            text("Filter:")
                .size(12)
                .color(crate::ui_style::palette().text_subtle),
            text_input("Search/filter lines...", &self.filter)
                .on_input(LogViewerMessage::FilterChanged)
                .size(12)
                .width(Length::Fixed(180.0)),
            button(text(auto_scroll_label).size(12))
                .padding([3, 10])
                .style(crate::ui_style::secondary_button)
                .on_press(LogViewerMessage::ToggleAutoScroll),
            button(text("Clear").size(12))
                .padding([3, 10])
                .style(crate::ui_style::secondary_button)
                .on_press(LogViewerMessage::ClearPressed),
            text(&self.status_text)
                .size(12)
                .color(crate::ui_style::palette().text_subtle),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);

        // ログテキスト表示
        let mut log_column = column![].spacing(2);
        for (_idx, line) in self.filtered_lines() {
            log_column = log_column.push(text(line.to_string()).size(12));
        }

        let log_content = container(log_column).width(Fill).padding(6);

        let scroll_view = scrollable(log_content).width(Fill).height(Fill);

        column![container(toolbar).padding([6, 8]), scroll_view]
            .spacing(2)
            .width(Fill)
            .height(Fill)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ring_buffer_capacity() {
        let mut viewer = LogViewerState::new();
        viewer.max_lines = 5;

        let initial_lines = vec![
            "line 1".to_string(),
            "line 2".to_string(),
            "line 3".to_string(),
        ];
        viewer.push_lines(initial_lines);
        assert_eq!(viewer.lines.len(), 3);
        assert_eq!(viewer.lines[0], "line 1");

        // 追加行で上限を超過
        let more_lines = vec![
            "line 4".to_string(),
            "line 5".to_string(),
            "line 6".to_string(),
            "line 7".to_string(),
        ];
        viewer.push_lines(more_lines);
        assert_eq!(viewer.lines.len(), 5);
        // line 1, 2 が破棄され、line 3〜7 が残る
        assert_eq!(viewer.lines[0], "line 3");
        assert_eq!(viewer.lines[4], "line 7");
    }

    #[test]
    fn test_filtered_lines() {
        let mut viewer = LogViewerState::new();
        viewer.push_lines(vec![
            "INFO: Starting job".to_string(),
            "DEBUG: Iteration 1 norm=0.45".to_string(),
            "WARN: High gradient".to_string(),
            "DEBUG: Iteration 2 norm=0.12".to_string(),
            "INFO: Completed".to_string(),
        ]);

        // 1. フィルタなし
        let all: Vec<&str> = viewer.filtered_lines().map(|(_, l)| l).collect();
        assert_eq!(all.len(), 5);

        // 2. 部分一致フィルタ
        viewer.filter = "DEBUG".to_string();
        let debugs: Vec<&str> = viewer.filtered_lines().map(|(_, l)| l).collect();
        assert_eq!(debugs.len(), 2);
        assert_eq!(debugs[0], "DEBUG: Iteration 1 norm=0.45");
        assert_eq!(debugs[1], "DEBUG: Iteration 2 norm=0.12");

        // 3. 該当なし
        viewer.filter = "ERROR".to_string();
        let empty: Vec<&str> = viewer.filtered_lines().map(|(_, l)| l).collect();
        assert!(empty.is_empty());
    }

    #[test]
    fn test_handle_stream_message() {
        let mut viewer = LogViewerState::new();
        viewer.handle_message(LogViewerMessage::StreamMessageReceived(
            LogStreamMessage::Connected {
                path: "test.log".to_string(),
                file_exists: true,
            },
        ));
        assert!(viewer.status_text.contains("test.log"));

        viewer.handle_message(LogViewerMessage::StreamMessageReceived(
            LogStreamMessage::Lines(vec!["Test line 1".to_string()]),
        ));
        assert_eq!(viewer.lines.len(), 1);
        assert_eq!(viewer.lines[0], "Test line 1");
    }
}
