//! メニューまわりの共通のスタイル（architecture.md 8.2節・8.3節）。
//!
//! 色は直接書かず、色の役割ごとの値をまとめた `UiPalette` から取る。
//! 今は暗いテーマの値（`UiPalette::dark()`）だけで、テーマの切り替えは後続のタスク（#7）で行う。

use iced::widget::{button, container};
use iced::{Border, Color, Shadow, Theme, Vector};
use std::sync::atomic::{AtomicU8, Ordering};

/// UI のテーマ（暗い / 明るい）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeKind {
    /// 暗いテーマ（既定）
    #[default]
    Dark,
    /// 明るいテーマ
    Light,
}

impl ThemeKind {
    /// 設定ファイル（`theme:`）に書く名前
    pub fn as_str(self) -> &'static str {
        match self {
            ThemeKind::Dark => "dark",
            ThemeKind::Light => "light",
        }
    }

    /// 設定ファイルの名前から求める（大文字・小文字と前後の空白は区別しない）
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "dark" => Some(ThemeKind::Dark),
            "light" => Some(ThemeKind::Light),
            _ => None,
        }
    }

    /// 画面に出す名前
    pub fn label(self) -> &'static str {
        match self {
            ThemeKind::Dark => "Dark",
            ThemeKind::Light => "Light",
        }
    }

    /// 標準のウィジェット（ボタン、テキスト入力、スクロールバーなど）に使う iced のテーマ。
    /// 組み込みの暗い / 明るいテーマの、強調色（`primary`）だけを、UI の強調色（`accent`。設定ファイルで変えられる）にする。
    pub fn iced_theme(self) -> Theme {
        let (name, base) = match self {
            ThemeKind::Dark => ("YUI Dark", Theme::Dark),
            ThemeKind::Light => ("YUI Light", Theme::Light),
        };
        let mut colors = base.palette();
        colors.primary = palette_for(self).accent;
        Theme::custom(name, colors)
    }
}

/// 今のテーマ。`Yui::view` の先頭で更新し、スタイル関数は `palette()` で参照する
/// （色の取得のために、すべての描画関数へパレットを渡して回らなくてよいようにする）。
static CURRENT_THEME: AtomicU8 = AtomicU8::new(0);

/// 今のテーマを設定する
pub fn set_theme(kind: ThemeKind) {
    CURRENT_THEME.store(kind as u8, Ordering::Relaxed);
}

/// 今のテーマ
pub fn theme() -> ThemeKind {
    match CURRENT_THEME.load(Ordering::Relaxed) {
        1 => ThemeKind::Light,
        _ => ThemeKind::Dark,
    }
}

/// 今のテーマのパレット（設定ファイルの `ui.colors` などで上書きされた色を含む）
pub fn palette() -> UiPalette {
    palette_for(theme())
}

/// 指定したテーマのパレット（設定ファイルの上書きを含む）
pub fn palette_for(kind: ThemeKind) -> UiPalette {
    let mut palette = UiPalette::for_theme(kind);
    let s = ui_settings();
    let themed = match kind {
        ThemeKind::Dark => &s.dark_colors,
        ThemeKind::Light => &s.light_colors,
    };
    for (name, color) in s.colors.iter().chain(themed) {
        palette.set_color(name, *color);
    }
    palette
}

/// タブの見た目
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabStyle {
    /// 選択中のタブの下に、アクセント色の線を引く（既定）
    #[default]
    Underline,
    /// 選択中のタブの背景を塗る
    Filled,
}

impl TabStyle {
    /// 設定ファイル（`ui.tab_style`）に書く名前から求める（大文字・小文字と前後の空白は区別しない）
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "underline" => Some(TabStyle::Underline),
            "filled" => Some(TabStyle::Filled),
            _ => None,
        }
    }
}

/// 設定ファイルの `ui:` で変えられる、メニューとタブの見た目（検証済み。範囲外の値は既定値に戻してある）
#[derive(Debug, Clone, PartialEq)]
pub struct UiSettings {
    pub tab_style: TabStyle,
    /// タブの文字の大きさ（ピクセル）
    pub tab_font_size: f32,
    /// メニューやダイアログのカードの角の丸み（ピクセル。項目の角は、その半分）
    pub corner_radius: f32,
    /// 色の上書き（名前は `COLOR_NAMES`）。`colors` は両方のテーマ、`dark_colors` / `light_colors` は、そのテーマのとき
    pub colors: Vec<(String, Color)>,
    pub dark_colors: Vec<(String, Color)>,
    pub light_colors: Vec<(String, Color)>,
    /// タブがなくなった左・右・下のパネルを、自動で隠す（既定は `false`: 残して案内を出す）
    pub hide_empty_docks: bool,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            tab_style: TabStyle::default(),
            tab_font_size: TAB_FONT_SIZE_DEFAULT,
            corner_radius: CORNER_RADIUS_DEFAULT,
            colors: Vec::new(),
            dark_colors: Vec::new(),
            light_colors: Vec::new(),
            hide_empty_docks: false,
        }
    }
}

/// `ui.tab_font_size` の既定値と範囲
pub const TAB_FONT_SIZE_DEFAULT: f32 = 12.0;
pub const TAB_FONT_SIZE_MIN: f64 = 8.0;
pub const TAB_FONT_SIZE_MAX: f64 = 24.0;
/// `ui.corner_radius` の既定値と範囲
pub const CORNER_RADIUS_DEFAULT: f32 = 8.0;
pub const CORNER_RADIUS_MIN: f64 = 0.0;
pub const CORNER_RADIUS_MAX: f64 = 16.0;

/// 起動時に決めた `ui:` の設定（一度だけ設定する。設定しなければ既定値）
static UI_SETTINGS: std::sync::OnceLock<UiSettings> = std::sync::OnceLock::new();

/// 起動時に、設定ファイルの `ui:` を記録する（2 回目以降は無視する）
pub fn set_ui_settings(settings: UiSettings) {
    let _ = UI_SETTINGS.set(settings);
}

/// 今の `ui:` の設定
pub fn ui_settings() -> &'static UiSettings {
    UI_SETTINGS.get_or_init(UiSettings::default)
}

/// 設定ファイルで上書きできる色の名前（`UiPalette` の同名のフィールド）
pub const COLOR_NAMES: &[&str] = &[
    "accent",
    "bar_bg",
    "card_bg",
    "card_border",
    "text",
    "text_muted",
    "text_disabled",
    "item_hover_bg",
    "item_active_bg",
    "separator",
    "tab_bar_bg",
    "tab_indicator",
    "tab_active_text",
    "tab_inactive_text",
    "tab_active_bg",
    "tab_hover_bg",
    "status_bg",
    "status_border",
    "panel_bg",
    "panel_border",
];

/// `#rrggbb` か `#rrggbbaa`（16 進。`#` は省略できる）から、色を求める
pub fn parse_hex_color(text: &str) -> Option<Color> {
    let hex = text.trim().trim_start_matches('#');
    if !(hex.len() == 6 || hex.len() == 8) || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    let a = if hex.len() == 8 { byte(6)? } else { 255 };
    Some(Color::from_rgba8(
        byte(0)?,
        byte(2)?,
        byte(4)?,
        f32::from(a) / 255.0,
    ))
}

/// UI の色の役割ごとの値。
///
/// 3D ビューポートの背景色（`AppState.scene.background_color`）と、分子の表示のための色
/// （原子・鎖・二次構造など）、ターミナルの ANSI の色は、UI のテーマとは別なので、ここには含めない。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiPalette {
    /// カード（メニュー、ダイアログ）の背景
    pub card_bg: Color,
    /// カードの枠線
    pub card_border: Color,
    /// カードの影
    pub shadow: Color,
    /// 文字（最も濃い）
    pub text: Color,
    /// 値や本文の文字
    pub text_value: Color,
    /// 副次的な文字
    pub text_secondary: Color,
    /// 控えめな文字（見出しなど）
    pub text_muted: Color,
    /// さらに控えめな文字（ラベル、補足）
    pub text_subtle: Color,
    /// 最も薄い文字（目立たせない補足）
    pub text_faint: Color,
    /// 押せない項目の文字
    pub text_disabled: Color,
    /// 項目にマウスを重ねたときの背景
    pub item_hover_bg: Color,
    /// 項目を押している間、または開いているメニューのボタンの背景
    pub item_active_bg: Color,
    /// 強調色
    pub accent: Color,
    /// 区切り線
    pub separator: Color,
    /// メニューバーの帯の背景
    pub bar_bg: Color,
    /// 注意を促す項目（Delete など）の背景と文字
    pub danger_bg: Color,
    pub danger_text: Color,
    /// モーダルの後ろを暗くする色
    pub scrim: Color,
    /// 選択された行・項目の背景
    pub row_selected_bg: Color,
    /// 選択された項目の背景（強め / 弱め）と、弱めの背景の上の文字
    pub select_bg: Color,
    pub select_bg_soft: Color,
    pub select_text: Color,
    /// 操作の種類ごとのチップ（選択中）の背景。上の文字は `on_chip`
    pub chip_blue_bg: Color,
    pub chip_teal_bg: Color,
    pub chip_orange_bg: Color,
    pub on_chip: Color,
    /// 選ばれていないチップの背景
    pub chip_bg: Color,
    /// 削除ボタン（Representation 一覧）の背景と文字
    pub danger_soft_bg: Color,
    pub danger_soft_text: Color,
    /// 削除ボタン（Named Selections）の背景・マウスを重ねたときの背景・文字
    pub danger_btn_bg: Color,
    pub danger_btn_hover_bg: Color,
    pub danger_btn_text: Color,
    /// 表示中を示すアイコンの色
    pub visible_icon: Color,
    /// 一覧の枠（パネル）の背景と枠線
    pub panel_bg: Color,
    pub panel_border: Color,
    /// 一覧の見出しの背景
    pub header_bg: Color,
    /// 選ばれた項目の名前を強調する文字
    pub highlight_text: Color,
    /// 保存ボタン（金色）の通常・マウスを重ねたとき・押している間の背景と、その上の文字
    pub gold: Color,
    pub gold_hover: Color,
    pub gold_pressed: Color,
    pub on_gold: Color,
    /// ステータスバーの背景と枠線
    pub status_bg: Color,
    pub status_border: Color,
    /// 状態の色（警告、エラー、接続中）
    pub warning: Color,
    pub error: Color,
    pub online: Color,
    /// 上部の案内の帯の背景
    pub banner_bg: Color,
    /// 選択の強調（ハロー / グロー）の色。分子の色と区別しやすい色にする。
    pub selection: Color,
    /// タブバーの背景、選択中のタブの下線（と上の文字）、選択中・非選択のタブの文字、
    /// 選択中のタブの背景（`tab_style: filled` のとき）、マウスを重ねたときのタブの背景
    pub tab_bar_bg: Color,
    pub tab_indicator: Color,
    pub tab_active_text: Color,
    pub tab_inactive_text: Color,
    pub tab_active_bg: Color,
    pub tab_hover_bg: Color,
    /// ターミナルの背景・既定の文字・カーソル（ANSI の色は端末のデータなので別）
    pub terminal_bg: Color,
    pub terminal_fg: Color,
    pub terminal_cursor: Color,
}

impl UiPalette {
    /// テーマからパレットを選ぶ
    pub const fn for_theme(kind: ThemeKind) -> Self {
        match kind {
            ThemeKind::Dark => Self::dark(),
            ThemeKind::Light => Self::light(),
        }
    }

    /// 暗いテーマ（カードの色は、従来の右クリックのメニューの値。灰色の文字は近い値を段階にまとめてある）
    pub const fn dark() -> Self {
        Self {
            card_bg: Color::from_rgba(0.12, 0.13, 0.16, 0.96),
            card_border: Color::from_rgba(0.3, 0.35, 0.45, 0.8),
            shadow: Color::from_rgba(0.0, 0.0, 0.0, 0.5),
            text: Color::from_rgb(0.92, 0.93, 0.95),
            text_value: Color::from_rgb(0.8, 0.85, 0.95),
            text_secondary: Color::from_rgb(0.7, 0.72, 0.82),
            text_muted: Color::from_rgb(0.65, 0.67, 0.72),
            text_subtle: Color::from_rgb(0.52, 0.56, 0.65),
            text_faint: Color::from_rgb(0.43, 0.47, 0.56),
            text_disabled: Color::from_rgb(0.42, 0.44, 0.48),
            item_hover_bg: Color::from_rgba(1.0, 1.0, 1.0, 0.10),
            item_active_bg: Color::from_rgba(1.0, 1.0, 1.0, 0.18),
            accent: Color::from_rgb(0.36, 0.56, 0.95),
            separator: Color::from_rgba(0.3, 0.35, 0.45, 0.7),
            bar_bg: Color::from_rgb(0.10, 0.11, 0.14),
            danger_bg: Color::from_rgba(0.7, 0.1, 0.1, 0.4),
            danger_text: Color::from_rgb(1.0, 0.5, 0.5),
            scrim: Color::from_rgba(0.0, 0.0, 0.0, 0.45),
            row_selected_bg: Color::from_rgba(0.2, 0.35, 0.55, 0.45),
            select_bg: Color::from_rgba(0.2, 0.6, 0.9, 0.4),
            select_bg_soft: Color::from_rgba(0.2, 0.6, 0.9, 0.2),
            select_text: Color::from_rgb(0.5, 0.8, 1.0),
            chip_blue_bg: Color::from_rgba(0.2, 0.5, 0.8, 0.6),
            chip_teal_bg: Color::from_rgba(0.2, 0.6, 0.5, 0.6),
            chip_orange_bg: Color::from_rgba(0.7, 0.4, 0.2, 0.6),
            on_chip: Color::WHITE,
            chip_bg: Color::from_rgba(1.0, 1.0, 1.0, 0.04),
            danger_soft_bg: Color::from_rgba(0.9, 0.2, 0.2, 0.3),
            danger_soft_text: Color::from_rgb(1.0, 0.4, 0.4),
            danger_btn_bg: Color::from_rgba(0.4, 0.2, 0.2, 0.4),
            danger_btn_hover_bg: Color::from_rgba(0.7, 0.2, 0.2, 0.8),
            danger_btn_text: Color::from_rgb(0.9, 0.8, 0.8),
            visible_icon: Color::from_rgb(0.3, 0.85, 0.6),
            panel_bg: Color::from_rgba(0.15, 0.16, 0.20, 0.75),
            panel_border: Color::from_rgba(0.3, 0.35, 0.45, 0.5),
            header_bg: Color::from_rgba(0.2, 0.3, 0.45, 0.6),
            highlight_text: Color::from_rgb(1.0, 0.88, 0.3),
            gold: Color::from_rgb(0.8, 0.68, 0.1),
            gold_hover: Color::from_rgb(0.9, 0.75, 0.1),
            gold_pressed: Color::from_rgb(0.75, 0.6, 0.05),
            on_gold: Color::BLACK,
            status_bg: Color::from_rgb(0.09, 0.10, 0.12),
            status_border: Color::from_rgb(0.22, 0.24, 0.29),
            warning: Color::from_rgb(0.95, 0.78, 0.30),
            error: Color::from_rgb(0.96, 0.45, 0.45),
            online: Color::from_rgb(0.45, 0.85, 0.55),
            banner_bg: Color::from_rgb(45.0 / 255.0, 52.0 / 255.0, 60.0 / 255.0),
            selection: Color::from_rgb(0.25, 0.85, 1.0),
            tab_bar_bg: Color::from_rgb(0.10, 0.11, 0.14),
            tab_indicator: Color::from_rgb(0.36, 0.56, 0.95),
            tab_active_text: Color::from_rgb(0.92, 0.93, 0.95),
            tab_inactive_text: Color::from_rgb(0.55, 0.58, 0.66),
            tab_active_bg: Color::from_rgba(1.0, 1.0, 1.0, 0.09),
            tab_hover_bg: Color::from_rgba(1.0, 1.0, 1.0, 0.06),
            terminal_bg: Color::from_rgb(0.08, 0.08, 0.08),
            terminal_fg: Color::from_rgb(0.9, 0.9, 0.9),
            terminal_cursor: Color::from_rgb(0.35, 0.85, 1.0),
        }
    }

    /// 明るいテーマ。ターミナルは、ANSI の色が暗い背景を前提にしているため、暗いままにする。
    pub const fn light() -> Self {
        Self {
            card_bg: Color::from_rgba(0.99, 0.99, 1.0, 0.98),
            card_border: Color::from_rgb(0.72, 0.75, 0.82),
            shadow: Color::from_rgba(0.0, 0.0, 0.0, 0.25),
            text: Color::from_rgb(0.12, 0.13, 0.16),
            text_value: Color::from_rgb(0.14, 0.17, 0.24),
            text_secondary: Color::from_rgb(0.28, 0.30, 0.38),
            text_muted: Color::from_rgb(0.38, 0.40, 0.46),
            text_subtle: Color::from_rgb(0.42, 0.45, 0.52),
            text_faint: Color::from_rgb(0.50, 0.53, 0.60),
            text_disabled: Color::from_rgb(0.62, 0.64, 0.68),
            item_hover_bg: Color::from_rgba(0.0, 0.0, 0.0, 0.07),
            item_active_bg: Color::from_rgba(0.0, 0.0, 0.0, 0.13),
            accent: Color::from_rgb(0.15, 0.40, 0.85),
            separator: Color::from_rgb(0.80, 0.82, 0.86),
            bar_bg: Color::from_rgb(0.93, 0.94, 0.96),
            danger_bg: Color::from_rgba(0.85, 0.2, 0.2, 0.18),
            danger_text: Color::from_rgb(0.7, 0.1, 0.1),
            scrim: Color::from_rgba(0.0, 0.0, 0.0, 0.30),
            row_selected_bg: Color::from_rgba(0.25, 0.5, 0.9, 0.25),
            select_bg: Color::from_rgba(0.2, 0.5, 0.9, 0.28),
            select_bg_soft: Color::from_rgba(0.2, 0.5, 0.9, 0.12),
            select_text: Color::from_rgb(0.1, 0.35, 0.7),
            chip_blue_bg: Color::from_rgba(0.15, 0.45, 0.8, 0.9),
            chip_teal_bg: Color::from_rgba(0.1, 0.5, 0.45, 0.9),
            chip_orange_bg: Color::from_rgba(0.75, 0.4, 0.1, 0.9),
            on_chip: Color::WHITE,
            chip_bg: Color::from_rgba(0.0, 0.0, 0.0, 0.05),
            danger_soft_bg: Color::from_rgba(0.85, 0.2, 0.2, 0.18),
            danger_soft_text: Color::from_rgb(0.75, 0.1, 0.1),
            danger_btn_bg: Color::from_rgba(0.85, 0.3, 0.3, 0.2),
            danger_btn_hover_bg: Color::from_rgba(0.85, 0.25, 0.25, 0.5),
            danger_btn_text: Color::from_rgb(0.6, 0.1, 0.1),
            visible_icon: Color::from_rgb(0.1, 0.6, 0.35),
            panel_bg: Color::from_rgba(0.95, 0.96, 0.98, 0.9),
            panel_border: Color::from_rgba(0.7, 0.73, 0.8, 0.7),
            header_bg: Color::from_rgba(0.75, 0.83, 0.95, 0.7),
            highlight_text: Color::from_rgb(0.75, 0.5, 0.0),
            gold: Color::from_rgb(0.9, 0.72, 0.1),
            gold_hover: Color::from_rgb(0.95, 0.78, 0.2),
            gold_pressed: Color::from_rgb(0.8, 0.62, 0.05),
            on_gold: Color::BLACK,
            status_bg: Color::from_rgb(0.91, 0.92, 0.95),
            status_border: Color::from_rgb(0.75, 0.77, 0.83),
            warning: Color::from_rgb(0.65, 0.45, 0.0),
            error: Color::from_rgb(0.75, 0.15, 0.15),
            online: Color::from_rgb(0.1, 0.55, 0.25),
            banner_bg: Color::from_rgb(0.88, 0.90, 0.94),
            selection: Color::from_rgb(0.10, 0.45, 0.95),
            tab_bar_bg: Color::from_rgb(0.93, 0.94, 0.96),
            tab_indicator: Color::from_rgb(0.15, 0.40, 0.85),
            tab_active_text: Color::from_rgb(0.12, 0.13, 0.16),
            tab_inactive_text: Color::from_rgb(0.42, 0.45, 0.52),
            tab_active_bg: Color::from_rgba(0.0, 0.0, 0.0, 0.08),
            tab_hover_bg: Color::from_rgba(0.0, 0.0, 0.0, 0.05),
            terminal_bg: Color::from_rgb(0.08, 0.08, 0.08),
            terminal_fg: Color::from_rgb(0.9, 0.9, 0.9),
            terminal_cursor: Color::from_rgb(0.35, 0.85, 1.0),
        }
    }
}

impl UiPalette {
    /// 名前（`COLOR_NAMES`）の色を置き換える。知らない名前なら `false`
    pub fn set_color(&mut self, name: &str, color: Color) -> bool {
        let slot = match name {
            "accent" => &mut self.accent,
            "bar_bg" => &mut self.bar_bg,
            "card_bg" => &mut self.card_bg,
            "card_border" => &mut self.card_border,
            "text" => &mut self.text,
            "text_muted" => &mut self.text_muted,
            "text_disabled" => &mut self.text_disabled,
            "item_hover_bg" => &mut self.item_hover_bg,
            "item_active_bg" => &mut self.item_active_bg,
            "separator" => &mut self.separator,
            "tab_bar_bg" => &mut self.tab_bar_bg,
            "tab_indicator" => &mut self.tab_indicator,
            "tab_active_text" => &mut self.tab_active_text,
            "tab_inactive_text" => &mut self.tab_inactive_text,
            "tab_active_bg" => &mut self.tab_active_bg,
            "tab_hover_bg" => &mut self.tab_hover_bg,
            "status_bg" => &mut self.status_bg,
            "status_border" => &mut self.status_border,
            "panel_bg" => &mut self.panel_bg,
            "panel_border" => &mut self.panel_border,
            _ => return false,
        };
        *slot = color;
        true
    }
}

impl Default for UiPalette {
    fn default() -> Self {
        Self::dark()
    }
}

/// カード（メニュー、ダイアログ）の角丸と、項目の角丸（その半分）
fn card_radius() -> f32 {
    ui_settings().corner_radius
}
fn item_radius() -> f32 {
    ui_settings().corner_radius / 2.0
}

/// メニューやダイアログのカードのスタイル（背景・枠線・影）
pub fn menu_card_style(palette: &UiPalette) -> container::Style {
    container::Style {
        background: Some(palette.card_bg.into()),
        border: Border {
            color: palette.card_border,
            width: 1.0,
            radius: card_radius().into(),
        },
        shadow: Shadow {
            color: palette.shadow,
            offset: Vector::new(0.0, 6.0),
            blur_radius: 18.0,
        },
        ..container::Style::default()
    }
}

/// 項目のボタンのスタイル。通常は背景なし、マウスを重ねたときだけ背景を付け、押せないときは文字を暗くする。
pub fn item_button_style(palette: &UiPalette, status: button::Status) -> button::Style {
    let (background, text_color) = match status {
        button::Status::Active => (None, palette.text),
        button::Status::Hovered => (Some(palette.item_hover_bg), palette.text),
        button::Status::Pressed => (Some(palette.item_active_bg), palette.text),
        button::Status::Disabled => (None, palette.text_disabled),
    };
    button::Style {
        background: background.map(Into::into),
        text_color,
        border: Border::default().rounded(item_radius()),
        ..button::Style::default()
    }
}

/// 注意を促す項目（Delete）のスタイル。通常から背景を付け、マウスを重ねると濃くする。
pub fn danger_button_style(palette: &UiPalette, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => {
            Color::from_rgba(0.85, 0.15, 0.15, 0.55)
        }
        _ => palette.danger_bg,
    };
    let text_color = if status == button::Status::Disabled {
        palette.text_disabled
    } else {
        palette.danger_text
    };
    button::Style {
        background: Some(background.into()),
        text_color,
        border: Border::default().rounded(item_radius()),
        ..button::Style::default()
    }
}

/// 確定のボタン（OK など）のスタイル。強調色で塗る。
pub fn accent_button_style(palette: &UiPalette, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => Color {
            a: 0.85,
            ..palette.accent
        },
        _ => palette.accent,
    };
    button::Style {
        background: Some(background.into()),
        text_color: Color::WHITE,
        border: Border::default().rounded(item_radius()),
        ..button::Style::default()
    }
}

/// メニューバーのボタン（File / View / Help）のスタイル。開いているメニューは、背景の色で示す。
pub fn menu_bar_button_style(
    palette: &UiPalette,
    active: bool,
    status: button::Status,
) -> button::Style {
    let mut style = item_button_style(palette, status);
    if active {
        style.background = Some(palette.item_active_bg.into());
    }
    style
}

/// メニューバーの帯のスタイル
pub fn menu_bar_style(palette: &UiPalette) -> container::Style {
    container::Style {
        background: Some(palette.bar_bg.into()),
        ..container::Style::default()
    }
}

/// タブのボタンのスタイル。文字の色で、選択中か（濃い）、そうでないか（控えめ）を示し、
/// マウスを重ねると背景を淡く塗る。`Filled` の選択中のタブは、背景も塗る。角は、上だけ丸める。
pub fn tab_button_style(
    palette: &UiPalette,
    style: TabStyle,
    active: bool,
    status: button::Status,
) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    let background = if active && style == TabStyle::Filled {
        Some(palette.tab_active_bg)
    } else if hovered {
        Some(palette.tab_hover_bg)
    } else {
        None
    };
    button::Style {
        background: background.map(Into::into),
        text_color: if active || hovered {
            palette.tab_active_text
        } else {
            palette.tab_inactive_text
        },
        border: Border::default().rounded(iced::border::top(item_radius())),
        ..button::Style::default()
    }
}

/// 選択中のタブの下の線（選択中でなければ透明。タブの高さが変わらないように、いつも同じ高さで置く）
pub fn tab_indicator_style(palette: &UiPalette, active: bool) -> container::Style {
    container::Style {
        background: active.then(|| palette.tab_indicator.into()),
        ..container::Style::default()
    }
}

/// タブをドラッグしているとき、落とす先のエディタのグループに重ねる色（落とす先でなければ透明）
pub fn tab_drop_style(palette: &UiPalette, active: bool) -> container::Style {
    container::Style {
        background: active.then(|| {
            let mut color = palette.tab_indicator;
            color.a = 0.15;
            color.into()
        }),
        border: if active {
            Border::default().color(palette.tab_indicator).width(1.0)
        } else {
            Border::default()
        },
        ..container::Style::default()
    }
}

/// ドラッグ中のタブの四角形（メニューと同じカードの色に、アクセント色の枠）。
/// 背景は透かさない（透かすと、後ろの文字と重なって読めない）。
pub fn tab_drag_ghost_style(palette: &UiPalette) -> container::Style {
    let mut background = palette.card_bg;
    background.a = 1.0;
    container::Style {
        background: Some(background.into()),
        text_color: Some(palette.tab_active_text),
        border: Border::default()
            .color(palette.tab_indicator)
            .width(1.0)
            .rounded(item_radius()),
        shadow: iced::Shadow {
            color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.3),
            offset: iced::Vector::new(0.0, 2.0),
            blur_radius: 6.0,
        },
        ..container::Style::default()
    }
}

/// タブの線の太さ
pub const TAB_INDICATOR_HEIGHT: f32 = 2.0;

/// ペインのタイトルバーのスタイル（メニューバーと同じ帯の色）
pub fn pane_title_style(palette: &UiPalette) -> container::Style {
    container::Style {
        background: Some(palette.bar_bg.into()),
        text_color: Some(palette.text_muted),
        ..container::Style::default()
    }
}

/// ペイン全体のスタイル（細い枠線で、隣のペインと区切る。隣り合う 2 枚で線が二重になるので、淡くする）
pub fn pane_body_style(palette: &UiPalette) -> container::Style {
    container::Style {
        border: Border {
            color: Color {
                a: palette.separator.a * 0.6,
                ..palette.separator
            },
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}

/// タブバーの帯のスタイル
pub fn tab_bar_style(palette: &UiPalette) -> container::Style {
    container::Style {
        background: Some(palette.tab_bar_bg.into()),
        ..container::Style::default()
    }
}

/// 区切り線（細い横線）の高さ
pub const SEPARATOR_HEIGHT: f32 = 1.0;

/// 区切り線の部品（幅いっぱいの、高さ 1 の線）
pub fn menu_separator<'a, Message: 'a>(palette: UiPalette) -> iced::Element<'a, Message> {
    container(iced::widget::Space::new())
        .width(iced::Length::Fill)
        .height(SEPARATOR_HEIGHT)
        .style(move |_theme: &Theme| container::Style {
            background: Some(palette.separator.into()),
            ..container::Style::default()
        })
        .into()
}

/// 副次的なボタン（Connect / Clear / Cancel など）のスタイル。控えめな塗りと細い枠で、強調色は使わない。
pub fn secondary_button_style(palette: &UiPalette, status: button::Status) -> button::Style {
    let (background, text_color) = match status {
        button::Status::Active => (palette.chip_bg, palette.text),
        button::Status::Hovered => (palette.item_hover_bg, palette.text),
        button::Status::Pressed => (palette.item_active_bg, palette.text),
        button::Status::Disabled => (palette.chip_bg, palette.text_disabled),
    };
    button::Style {
        background: Some(background.into()),
        text_color,
        border: Border {
            color: palette.card_border,
            width: 1.0,
            radius: item_radius().into(),
        },
        ..button::Style::default()
    }
}

/// `.style(...)` にそのまま渡せる、副次的なボタン。
pub fn secondary_button(_theme: &Theme, status: button::Status) -> button::Style {
    secondary_button_style(&palette(), status)
}

/// `.style(...)` にそのまま渡せる、確定のボタン。
pub fn accent_button(_theme: &Theme, status: button::Status) -> button::Style {
    accent_button_style(&palette(), status)
}

/// `.style(...)` にそのまま渡せる、背景のない項目のボタン（ツリーの行など）。
pub fn flat_button(_theme: &Theme, status: button::Status) -> button::Style {
    item_button_style(&palette(), status)
}

/// ダイアログの選択肢のボタン。選ばれているものは強調色、ほかは副次的なボタン。
pub fn choice_button(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        if selected {
            accent_button(theme, status)
        } else {
            secondary_button(theme, status)
        }
    }
}

/// ツリーで選ばれている行のボタン。
pub fn selected_flat_button(_theme: &Theme, status: button::Status) -> button::Style {
    let p = palette();
    let mut style = item_button_style(&p, status);
    style.background = Some(p.select_bg_soft.into());
    style.text_color = p.select_text;
    style
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_colors_are_parsed() {
        let c = parse_hex_color("#5b8def").unwrap();
        assert_eq!(c, Color::from_rgba8(0x5b, 0x8d, 0xef, 1.0));
        let c = parse_hex_color("  5B8DEF80 ").unwrap();
        assert!((c.a - 128.0 / 255.0).abs() < 1e-6);
        for bad in ["", "#fff", "#12345", "#12345g", "red", "#1234567"] {
            assert!(parse_hex_color(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn every_listed_color_name_can_be_set_and_others_cannot() {
        let mut p = UiPalette::dark();
        let c = Color::from_rgb(0.1, 0.2, 0.3);
        for name in COLOR_NAMES {
            let mut q = UiPalette::dark();
            assert!(q.set_color(name, c), "{name}");
            assert_ne!(q, UiPalette::dark(), "{name} did not change anything");
        }
        assert!(!p.set_color("no_such_color", c));
        assert_eq!(p, UiPalette::dark());
    }

    #[test]
    fn the_selected_tab_is_marked_and_a_hovered_one_is_lit() {
        let p = UiPalette::dark();
        let active = tab_button_style(&p, TabStyle::Underline, true, button::Status::Active);
        let idle = tab_button_style(&p, TabStyle::Underline, false, button::Status::Active);
        let hover = tab_button_style(&p, TabStyle::Underline, false, button::Status::Hovered);
        assert_eq!(active.text_color, p.tab_active_text);
        assert_eq!(idle.text_color, p.tab_inactive_text);
        assert!(idle.background.is_none() && hover.background.is_some());
        // 下線は、選択中だけ見える。`Filled` は、背景で示す
        assert!(tab_indicator_style(&p, true).background.is_some());
        assert!(tab_indicator_style(&p, false).background.is_none());
        let filled = tab_button_style(&p, TabStyle::Filled, true, button::Status::Active);
        assert!(filled.background.is_some() && active.background.is_none());
    }

    #[test]
    fn pane_titles_use_the_bar_color_and_panes_have_a_thin_border() {
        let p = UiPalette::dark();
        let title = pane_title_style(&p);
        assert_eq!(title.background, Some(p.bar_bg.into()));
        assert_eq!(title.text_color, Some(p.text_muted));
        let body = pane_body_style(&p);
        assert_eq!(body.border.width, 1.0);
        assert!(
            body.border.color.a < p.separator.a,
            "the border is fainter than a separator"
        );
        assert!(
            body.background.is_none(),
            "the pane keeps the window background"
        );
    }

    #[test]
    fn the_tab_style_name_is_parsed() {
        assert_eq!(TabStyle::parse(" Filled "), Some(TabStyle::Filled));
        assert_eq!(TabStyle::parse("UNDERLINE"), Some(TabStyle::Underline));
        assert_eq!(TabStyle::parse("pill"), None);
    }

    #[test]
    fn item_is_transparent_until_hovered() {
        let p = UiPalette::dark();
        assert!(
            item_button_style(&p, button::Status::Active)
                .background
                .is_none()
        );
        assert!(
            item_button_style(&p, button::Status::Hovered)
                .background
                .is_some()
        );
    }

    #[test]
    fn disabled_item_has_dim_text_and_no_background() {
        let p = UiPalette::dark();
        let s = item_button_style(&p, button::Status::Disabled);
        assert!(s.background.is_none());
        assert_eq!(s.text_color, p.text_disabled);
        assert_ne!(
            s.text_color,
            item_button_style(&p, button::Status::Active).text_color
        );
    }

    #[test]
    fn card_has_background_border_and_shadow() {
        let p = UiPalette::dark();
        let s = menu_card_style(&p);
        assert!(s.background.is_some());
        assert_eq!(s.border.color, p.card_border);
        assert_eq!(s.shadow.color, p.shadow);
    }

    #[test]
    fn accent_button_uses_the_accent_color() {
        let p = UiPalette::dark();
        let s = accent_button_style(&p, button::Status::Active);
        assert_eq!(s.background, Some(p.accent.into()));
    }

    #[test]
    fn open_menu_button_is_filled_without_hover() {
        let p = UiPalette::dark();
        assert!(
            menu_bar_button_style(&p, true, button::Status::Active)
                .background
                .is_some()
        );
        assert!(
            menu_bar_button_style(&p, false, button::Status::Active)
                .background
                .is_none()
        );
    }

    /// 相対輝度（WCAG の式）
    fn luminance(c: Color) -> f32 {
        let f = |v: f32| {
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b)
    }

    /// 背景に重ねた色（半透明を考慮）の見た目
    fn over(bg: Color, fg: Color) -> Color {
        Color::from_rgb(
            fg.r * fg.a + bg.r * (1.0 - fg.a),
            fg.g * fg.a + bg.g * (1.0 - fg.a),
            fg.b * fg.a + bg.b * (1.0 - fg.a),
        )
    }

    fn contrast(a: Color, b: Color) -> f32 {
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    /// 両方のテーマで、文字が背景から十分に区別できること
    #[test]
    fn text_is_readable_on_the_backgrounds_in_both_themes() {
        for kind in [ThemeKind::Dark, ThemeKind::Light] {
            let p = UiPalette::for_theme(kind);
            // 土台の色（窓の背景）は iced の標準のテーマ
            let window_bg = match kind {
                ThemeKind::Dark => Color::from_rgb(0.125, 0.129, 0.137),
                ThemeKind::Light => Color::WHITE,
            };
            let card = over(window_bg, p.card_bg);
            let panel = over(window_bg, p.panel_bg);
            for (name, fg) in [
                ("text", p.text),
                ("text_value", p.text_value),
                ("text_secondary", p.text_secondary),
                ("text_muted", p.text_muted),
                ("text_subtle", p.text_subtle),
            ] {
                for (bg_name, bg) in [("card", card), ("panel", panel), ("bar", p.bar_bg)] {
                    assert!(
                        contrast(fg, bg) >= 3.0,
                        "{kind:?}: {name} on {bg_name} = {}",
                        contrast(fg, bg)
                    );
                }
            }
            // 主な文字は特に濃く
            assert!(contrast(p.text, card) >= 7.0, "{kind:?}");
            // 状態の色は、ステータスバーの背景で読めること
            for (name, fg) in [
                ("warning", p.warning),
                ("error", p.error),
                ("online", p.online),
            ] {
                assert!(
                    contrast(fg, p.status_bg) >= 3.0,
                    "{kind:?}: {name} on status bar = {}",
                    contrast(fg, p.status_bg)
                );
            }
            assert!(contrast(p.text_secondary, p.status_bg) >= 3.0, "{kind:?}");
            assert!(contrast(p.text_subtle, p.status_bg) >= 3.0, "{kind:?}");
            assert!(contrast(p.text, p.banner_bg) >= 4.5, "{kind:?}");
            // 押せない項目の文字は、通常の文字より薄い
            assert!(luminance(p.text_disabled) != luminance(p.text), "{kind:?}");
        }
    }

    #[test]
    fn terminal_text_is_readable_in_both_themes() {
        for kind in [ThemeKind::Dark, ThemeKind::Light] {
            let p = UiPalette::for_theme(kind);
            assert!(contrast(p.terminal_fg, p.terminal_bg) >= 7.0, "{kind:?}");
            assert!(
                contrast(p.terminal_cursor, p.terminal_bg) >= 4.5,
                "{kind:?}"
            );
        }
    }

    #[test]
    fn dark_palette_keeps_the_original_card_colors() {
        // 従来の右クリックのメニューのカードの値
        let p = UiPalette::for_theme(ThemeKind::Dark);
        assert_eq!(p.card_bg, Color::from_rgba(0.12, 0.13, 0.16, 0.96));
        assert_eq!(p.card_border, Color::from_rgba(0.3, 0.35, 0.45, 0.8));
        assert_ne!(UiPalette::dark(), UiPalette::light());
    }

    #[test]
    fn theme_names_round_trip_and_select_iced_themes() {
        for kind in [ThemeKind::Dark, ThemeKind::Light] {
            assert_eq!(ThemeKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(ThemeKind::parse(" Light "), Some(ThemeKind::Light));
        assert_eq!(ThemeKind::parse("blue"), None);
        assert_eq!(ThemeKind::default(), ThemeKind::Dark);
        // 組み込みのテーマと同じ作りで、強調色だけが UI の強調色になる
        for (kind, base) in [
            (ThemeKind::Dark, Theme::Dark),
            (ThemeKind::Light, Theme::Light),
        ] {
            let (ours, theirs) = (kind.iced_theme().palette(), base.palette());
            assert_eq!(ours.primary, UiPalette::for_theme(kind).accent);
            assert_eq!(
                (ours.background, ours.text, ours.danger),
                (theirs.background, theirs.text, theirs.danger)
            );
        }
    }
}
