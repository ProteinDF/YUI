// SPDX-FileCopyrightText: The Yui development team
// SPDX-License-Identifier: GPL-3.0-or-later

//! ウィンドウ最下部のステータスバー。
//!
//! 操作の結果のメッセージ（情報・警告・エラー）、読み込んでいる構造の原子数・結合数、
//! 表示中のモデル、分子表面の生成中表示、協調編集の接続状態を1行で表示する。
//! 表示内容の組み立て（`build_status_bar_fields`）は純粋関数とし、描画（`view_status_bar`）と分ける。

use crate::ui_style::palette;
use iced::widget::{Space, container, row, text};
use iced::{Alignment, Color, Element, Length};

/// ステータスバーの高さ（論理ピクセル）。
///
/// メニューの高さの上限など、ウィンドウの残りの高さを求める処理から参照する。
pub const STATUS_BAR_HEIGHT: f32 = 24.0;

// ステータスバーの色は、今のテーマのパレットから取る（`ui_style`）。
fn bar_background() -> Color {
    palette().status_bg
}
fn bar_border() -> Color {
    palette().status_border
}
fn text_normal() -> Color {
    palette().text_secondary
}
fn text_muted() -> Color {
    palette().text_subtle
}
fn text_warning() -> Color {
    palette().warning
}
fn text_error() -> Color {
    palette().error
}
fn text_online() -> Color {
    palette().online
}

/// メッセージの種類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusLevel {
    Info,
    Warning,
    Error,
}

/// 操作の結果のメッセージ（文字列と種類）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusMessage {
    pub text: String,
    pub level: StatusLevel,
}

impl StatusMessage {
    pub fn info(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            level: StatusLevel::Info,
        }
    }

    pub fn warning(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            level: StatusLevel::Warning,
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            level: StatusLevel::Error,
        }
    }
}

/// 協調編集（backend）との接続状態
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionStatus {
    /// backend を使わない単体動作
    Standalone,
    /// backend へ接続を試みている
    Connecting,
    /// backend と接続済み
    Online,
    /// 初期状態の取得に失敗した
    Offline(String),
}

impl ConnectionStatus {
    /// ウィンドウのタイトルと同じ判定で接続状態を求める。
    pub fn from_flags(
        collab_connected: bool,
        rest_error: Option<&str>,
        has_collab_url: bool,
    ) -> Self {
        if collab_connected {
            Self::Online
        } else if let Some(err) = rest_error {
            Self::Offline(err.to_string())
        } else if has_collab_url {
            Self::Connecting
        } else {
            Self::Standalone
        }
    }
}

/// ステータスバーに表示する内容を求めるための入力
#[derive(Debug, Clone)]
pub struct StatusBarInput<'a> {
    pub message: Option<&'a StatusMessage>,
    /// 構造が読み込まれていれば (原子数, 結合数)
    pub structure_counts: Option<(usize, usize)>,
    /// 表示中のモデルのキー（`None` は全モデル表示）
    pub active_model: Option<&'a str>,
    /// 構造に含まれるモデルのキー（並び順）
    pub model_keys: &'a [&'a str],
    pub surface_generating: bool,
    /// 再生中か（表示中のモデルの欄に示す）
    pub playing: bool,
    /// 軌跡を開いているときの、今のフレームの表示（例: `step 30 (4/5)`）。あれば、モデルの欄の代わりに出す。
    pub trajectory_label: Option<&'a str>,
    pub connection: &'a ConnectionStatus,
}

/// ステータスバーの1つの欄
#[derive(Debug, Clone, PartialEq)]
pub struct StatusField {
    pub text: String,
    pub color: Color,
}

/// ステータスバーの全欄（左寄せのメッセージと、右寄せの情報）
#[derive(Debug, Clone, PartialEq)]
pub struct StatusBarFields {
    pub message: Option<StatusField>,
    pub right: Vec<StatusField>,
}

fn level_color(level: StatusLevel) -> Color {
    match level {
        StatusLevel::Info => text_normal(),
        StatusLevel::Warning => text_warning(),
        StatusLevel::Error => text_error(),
    }
}

/// 状態からステータスバーの各欄の文字列と色を決める（純粋関数）。
pub fn build_status_bar_fields(input: &StatusBarInput<'_>) -> StatusBarFields {
    let message = input.message.map(|m| StatusField {
        text: m.text.clone(),
        color: level_color(m.level),
    });

    let mut right = Vec::new();

    if input.surface_generating {
        right.push(StatusField {
            text: "Generating surface...".to_string(),
            color: text_muted(),
        });
    }

    if let Some((atoms, bonds)) = input.structure_counts {
        right.push(StatusField {
            text: format!("{atoms} atoms, {bonds} bonds"),
            color: text_normal(),
        });

        // 軌跡を開いているときは、今のフレームを示す
        if let Some(label) = input.trajectory_label {
            let label = if input.playing {
                format!("Playing {label}")
            } else {
                label.to_string()
            };
            right.push(StatusField {
                text: label,
                color: text_normal(),
            });
        }
        // マルチモデルのときだけ、表示中のモデルを示す
        let total = input.model_keys.len();
        if total > 1 && input.trajectory_label.is_none() {
            let label = match input.active_model {
                Some(key) => match input.model_keys.iter().position(|k| *k == key) {
                    Some(idx) => format!("{key} ({}/{total})", idx + 1),
                    None => format!("{key} (?/{total})"),
                },
                None => format!("All models ({total})"),
            };
            let label = if input.playing {
                format!("Playing {label}")
            } else {
                label
            };
            right.push(StatusField {
                text: label,
                color: text_normal(),
            });
        }
    }

    let connection = match input.connection {
        ConnectionStatus::Standalone => StatusField {
            text: "Local".to_string(),
            color: text_muted(),
        },
        ConnectionStatus::Connecting => StatusField {
            text: "Connecting...".to_string(),
            color: text_warning(),
        },
        ConnectionStatus::Online => StatusField {
            text: "Online".to_string(),
            color: text_online(),
        },
        ConnectionStatus::Offline(err) => StatusField {
            text: format!("Offline: {err}"),
            color: text_error(),
        },
    };
    right.push(connection);

    StatusBarFields { message, right }
}

/// ステータスバーを描画する。
pub fn view_status_bar<'a, Message: 'a>(fields: StatusBarFields) -> Element<'a, Message> {
    let mut bar = row![].spacing(16).align_y(Alignment::Center);

    if let Some(msg) = fields.message {
        bar = bar.push(text(msg.text).size(12).color(msg.color));
    }

    bar = bar.push(Space::new().width(Length::Fill));

    for field in fields.right {
        bar = bar.push(text(field.text).size(12).color(field.color));
    }

    container(bar)
        .width(Length::Fill)
        .height(Length::Fixed(STATUS_BAR_HEIGHT))
        .padding([0, 10])
        .align_y(Alignment::Center)
        .style(|_theme| container::Style {
            background: Some(bar_background().into()),
            border: iced::Border {
                color: bar_border(),
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input<'a>(
        message: Option<&'a StatusMessage>,
        structure_counts: Option<(usize, usize)>,
        active_model: Option<&'a str>,
        model_keys: &'a [&'a str],
        surface_generating: bool,
        connection: &'a ConnectionStatus,
    ) -> StatusBarInput<'a> {
        StatusBarInput {
            message,
            structure_counts,
            active_model,
            model_keys,
            surface_generating,
            playing: false,
            trajectory_label: None,
            connection,
        }
    }

    #[test]
    fn test_message_colors_follow_level() {
        let conn = ConnectionStatus::Standalone;
        let err = StatusMessage::error("Export failed");
        let warn = StatusMessage::warning("No structure loaded to export.");
        let info = StatusMessage::info("Loaded structure");

        let f = build_status_bar_fields(&input(Some(&err), None, None, &[], false, &conn));
        assert_eq!(f.message.unwrap().color, text_error());
        let f = build_status_bar_fields(&input(Some(&warn), None, None, &[], false, &conn));
        assert_eq!(f.message.unwrap().color, text_warning());
        let f = build_status_bar_fields(&input(Some(&info), None, None, &[], false, &conn));
        let m = f.message.unwrap();
        assert_eq!(m.color, text_normal());
        assert_eq!(m.text, "Loaded structure");
    }

    #[test]
    fn test_no_structure_hides_counts_and_model() {
        let conn = ConnectionStatus::Standalone;
        let f = build_status_bar_fields(&input(None, None, None, &[], false, &conn));
        assert!(f.message.is_none());
        // 接続状態の欄だけ
        assert_eq!(f.right.len(), 1);
        assert_eq!(f.right[0].text, "Local");
    }

    #[test]
    fn test_single_model_shows_counts_only() {
        let conn = ConnectionStatus::Standalone;
        let keys = ["model_1"];
        let f = build_status_bar_fields(&input(None, Some((782, 793)), None, &keys, false, &conn));
        let texts: Vec<_> = f.right.iter().map(|x| x.text.as_str()).collect();
        assert_eq!(texts, vec!["782 atoms, 793 bonds", "Local"]);
    }

    #[test]
    fn test_multi_model_shows_active_model() {
        let conn = ConnectionStatus::Standalone;
        let keys = ["model_1", "model_2", "model_3"];
        let f = build_status_bar_fields(&input(
            None,
            Some((2346, 2400)),
            Some("model_2"),
            &keys,
            false,
            &conn,
        ));
        assert!(f.right.iter().any(|x| x.text == "model_2 (2/3)"));

        let f =
            build_status_bar_fields(&input(None, Some((2346, 2400)), None, &keys, false, &conn));
        assert!(f.right.iter().any(|x| x.text == "All models (3)"));
    }

    #[test]
    fn test_surface_generating_field() {
        let conn = ConnectionStatus::Standalone;
        let f = build_status_bar_fields(&input(None, Some((9, 8)), None, &[], true, &conn));
        assert_eq!(f.right[0].text, "Generating surface...");
        let f = build_status_bar_fields(&input(None, Some((9, 8)), None, &[], false, &conn));
        assert!(f.right.iter().all(|x| x.text != "Generating surface..."));
    }

    #[test]
    fn test_connection_states() {
        assert_eq!(
            ConnectionStatus::from_flags(false, None, false),
            ConnectionStatus::Standalone
        );
        assert_eq!(
            ConnectionStatus::from_flags(false, None, true),
            ConnectionStatus::Connecting
        );
        assert_eq!(
            ConnectionStatus::from_flags(true, Some("ignored"), true),
            ConnectionStatus::Online
        );
        assert_eq!(
            ConnectionStatus::from_flags(false, Some("timeout"), true),
            ConnectionStatus::Offline("timeout".to_string())
        );

        let cases = [
            (ConnectionStatus::Standalone, "Local", text_muted()),
            (
                ConnectionStatus::Connecting,
                "Connecting...",
                text_warning(),
            ),
            (ConnectionStatus::Online, "Online", text_online()),
            (
                ConnectionStatus::Offline("timeout".to_string()),
                "Offline: timeout",
                text_error(),
            ),
        ];
        for (conn, label, color) in cases {
            let f = build_status_bar_fields(&input(None, None, None, &[], false, &conn));
            let last = f.right.last().unwrap();
            assert_eq!(last.text, label);
            assert_eq!(last.color, color);
        }
    }

    #[test]
    fn test_trajectory_frame_replaces_the_model_field() {
        let conn = ConnectionStatus::Standalone;
        let mut inp = input(
            None,
            Some((10, 9)),
            None,
            &["model_1", "model_2"],
            false,
            &conn,
        );
        inp.trajectory_label = Some("step 30 (4/5)");
        let f = build_status_bar_fields(&inp);
        assert!(f.right.iter().any(|x| x.text == "step 30 (4/5)"));
        assert!(
            !f.right.iter().any(|x| x.text.contains("All models")),
            "the model field is not shown with a trajectory"
        );
        inp.playing = true;
        let f = build_status_bar_fields(&inp);
        assert!(f.right.iter().any(|x| x.text == "Playing step 30 (4/5)"));
        // 構造がないときは、軌跡の欄も出ない
        let mut none = input(None, None, None, &[], false, &conn);
        none.trajectory_label = Some("step 1 (1/2)");
        assert!(
            !build_status_bar_fields(&none)
                .right
                .iter()
                .any(|x| x.text.starts_with("step"))
        );
    }
}
