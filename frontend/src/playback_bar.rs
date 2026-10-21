//! 再生の操作のバー（architecture.md 8.10節）。分子図の下端に置く。

use iced::widget::{Space, button, row, slider, text};
use iced::{Alignment, Element, Length, Padding};

use crate::playback::{LoopMode, MAX_FPS, MIN_FPS, PlaybackAction};
use crate::ui_style::palette;

/// 操作のバーに表示するもの
pub struct PlaybackBarInput {
    /// フレームの総数
    pub total: usize,
    /// 今のフレーム（全モデル表示のときは `None`）
    pub current: Option<usize>,
    /// 今のフレームの名前（例: `model_3`、`step 30`）
    pub label: Option<String>,
    /// 今のフレームがないとき（全モデル表示）に、「All models」と出すか
    pub all_label: bool,
    pub playing: bool,
    pub fps: u32,
    pub loop_mode: LoopMode,
}

/// フレームが 2 つ以上あるときだけ、バーを出す。1 つ以下なら `None`。
pub fn view_playback_bar<'a, Message: Clone + 'a>(
    input: &PlaybackBarInput,
    on_action: impl Fn(PlaybackAction) -> Message + Copy + 'a,
) -> Option<Element<'a, Message>> {
    if input.total < 2 {
        return None;
    }
    let btn = |label: &'static str, action: PlaybackAction| {
        button(text(label).size(11))
            .padding(Padding::from([2, 8]))
            .style(crate::ui_style::secondary_button)
            .on_press(on_action(action))
    };

    let last = (input.total - 1) as u32;
    let frame_slider = slider(0..=last, input.current.unwrap_or(0) as u32, move |v| {
        on_action(PlaybackAction::SetFrame(v as usize))
    })
    .on_release(on_action(PlaybackAction::ScrubEnd))
    .width(Length::Fill);

    let frame_text = match (input.current, &input.label) {
        (Some(i), Some(label)) => format!("{label} ({}/{})", i + 1, input.total),
        (Some(i), None) => format!("frame {}/{}", i + 1, input.total),
        _ if input.all_label => format!("All models ({})", input.total),
        _ => format!("{} frames", input.total),
    };

    let fps_slider = slider(MIN_FPS..=MAX_FPS, input.fps, move |v| {
        on_action(PlaybackAction::SetFps(v))
    })
    .width(Length::Fixed(80.0));

    let loop_button = button(text(format!("Loop: {}", input.loop_mode.label())).size(11))
        .padding(Padding::from([2, 8]))
        .style(crate::ui_style::secondary_button)
        .on_press(on_action(PlaybackAction::SetLoopMode(
            input.loop_mode.next(),
        )));

    Some(
        row![
            btn("|◀", PlaybackAction::First),
            btn("◀", PlaybackAction::Prev),
            btn(
                if input.playing { "Pause" } else { "Play" },
                PlaybackAction::PlayPause
            ),
            btn("▶", PlaybackAction::Next),
            btn("▶|", PlaybackAction::Last),
            frame_slider,
            text(frame_text).size(11).color(palette().text),
            Space::new().width(Length::Fixed(8.0)),
            fps_slider,
            text(format!("{} fps", input.fps))
                .size(11)
                .color(palette().text_secondary),
            loop_button,
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .padding(Padding::from([4, 8]))
        .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(total: usize) -> PlaybackBarInput {
        PlaybackBarInput {
            total,
            current: Some(0),
            label: Some("model_1".to_string()),
            all_label: true,
            playing: false,
            fps: 5,
            loop_mode: LoopMode::Repeat,
        }
    }

    #[test]
    fn test_bar_is_shown_only_with_two_or_more_frames() {
        assert!(view_playback_bar::<PlaybackAction>(&input(0), |a| a).is_none());
        assert!(view_playback_bar::<PlaybackAction>(&input(1), |a| a).is_none());
        assert!(view_playback_bar::<PlaybackAction>(&input(2), |a| a).is_some());
        assert!(view_playback_bar::<PlaybackAction>(&input(20), |a| a).is_some());
    }

    #[test]
    fn test_bar_builds_in_every_state() {
        for (current, label, playing) in [
            (None, None, false),
            (Some(19), Some("model_20".to_string()), true),
            (Some(3), None, false),
        ] {
            let mut i = input(20);
            i.current = current;
            i.label = label;
            i.playing = playing;
            assert!(view_playback_bar::<PlaybackAction>(&i, |a| a).is_some());
        }
    }
}
