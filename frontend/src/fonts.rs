//! アプリで使うフォントの定義。
//!
//! エディタとターミナルのフォント（名前・大きさ・太さ）は、ここの 1 か所から取る。
//! 後で利用者ごとの設定から名前と大きさを変えられるようにするための入口である。
//!
//! - 既定の UI フォント: Noto Sans JP（`main.rs` で登録）
//! - 等幅フォント: JetBrains Mono（OFL、Regular と Bold を同梱）。日本語の文字は等幅フォントにないため、
//!   フォント探索の代替（フォールバック）で Noto Sans JP が使われる。

use std::sync::OnceLock;

use iced::Font;
use iced::font::{Family, Stretch, Style, Weight};

use crate::user_settings::FontChoice;

/// 同梱の等幅フォントのファミリー名
pub const MONO_FONT_NAME: &str = "JetBrains Mono";
/// 同梱の UI フォント（日本語を含む）のファミリー名
pub const UI_FONT_NAME: &str = "Noto Sans JP";

/// 同梱の等幅フォントのデータ（Regular, Bold）
pub const MONO_FONT_REGULAR: &[u8] = include_bytes!("../assets/JetBrainsMono-Regular.ttf");
pub const MONO_FONT_BOLD: &[u8] = include_bytes!("../assets/JetBrainsMono-Bold.ttf");

/// エディタの文字の大きさ（ピクセル）の既定値
pub const EDITOR_FONT_SIZE: f32 = 14.0;
/// ターミナルの文字の大きさ（ピクセル）の既定値
pub const TERMINAL_FONT_SIZE: f32 = 12.0;

/// 起動時に決めた、エディタとターミナルのフォント（ファミリー名は、実行中ずっと使うので、起動時に 1 回だけ確保する）
static CHOICE: OnceLock<(&'static str, f32)> = OnceLock::new();
/// 起動時に決めた、ターミナルの文字の大きさ
static TERMINAL_SIZE: OnceLock<f32> = OnceLock::new();

/// 利用者ごとの設定から決めたフォントを記録する（`main` で、アプリを始める前に 1 回だけ呼ぶ）。
/// 呼ばなければ、同梱の等幅フォントと既定の大きさになる。
pub fn init(choice: &FontChoice) {
    // iced のフォントの名前は &'static str を要求する。設定から決まる名前を 1 回だけ確保して使い続ける。
    let name: &'static str = Box::leak(choice.family.clone().into_boxed_str());
    let _ = CHOICE.set((name, choice.size));
    let _ = TERMINAL_SIZE.set(choice.terminal_size);
}

fn current() -> (&'static str, f32) {
    CHOICE
        .get()
        .copied()
        .unwrap_or((MONO_FONT_NAME, EDITOR_FONT_SIZE))
}

/// 等幅フォント（エディタ・ターミナル用）。`bold` で太字にする。
pub fn mono_font(bold: bool) -> Font {
    Font {
        family: Family::Name(current().0),
        weight: if bold { Weight::Bold } else { Weight::Normal },
        stretch: Stretch::Normal,
        style: Style::Normal,
    }
}

/// 端末の全角（日本語など）の字のフォント。同梱の Noto Sans JP（全角はどの字も 1.0 倍の幅）
pub fn cjk_font() -> Font {
    Font {
        family: Family::Name(UI_FONT_NAME),
        weight: Weight::Normal,
        stretch: Stretch::Normal,
        style: Style::Normal,
    }
}

/// ターミナルの文字の大きさ（ピクセル）
pub fn terminal_font_size() -> f32 {
    TERMINAL_SIZE.get().copied().unwrap_or(TERMINAL_FONT_SIZE)
}

/// エディタの文字の大きさ（ピクセル）
pub fn editor_font_size() -> f32 {
    current().1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mono_font_spec() {
        let regular = mono_font(false);
        assert_eq!(regular.family, Family::Name(MONO_FONT_NAME));
        assert_eq!(regular.weight, Weight::Normal);
        assert_eq!(mono_font(true).weight, Weight::Bold);
    }

    #[test]
    fn test_bundled_fonts_are_truetype() {
        // TrueType (0x00010000) のヘッダーで始まり、極端に小さくないこと
        for data in [MONO_FONT_REGULAR, MONO_FONT_BOLD] {
            assert_eq!(&data[..4], &[0x00, 0x01, 0x00, 0x00]);
            assert!(data.len() > 100_000);
        }
    }
}
