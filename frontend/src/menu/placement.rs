//! メニューをウィンドウの中に収める位置と高さの計算（architecture.md 8.3節）。
//!
//! iced では、描画前にメニューの実際の高さを得にくい。そこで、メニューモデルの項目から高さを見積もる
//! （文字の高さ = 文字の大きさ × 1.3（iced の既定の行の高さ）、ボタンはそれに縦の余白を足す）。
//! 見積もりが実際と少しずれても、はみ出さないよう余白（`MARGIN`）を取り、収まらないときは
//! スクロールで補う。

use iced::Size;

use super::model::{GroupLayout, MenuItem};
use super::render::Surface;
use crate::status_bar::STATUS_BAR_HEIGHT;

/// ウィンドウの端からメニューまで空ける余白
pub const MARGIN: f32 = 8.0;
/// iced の既定の行の高さ（文字の大きさに対する倍率）
const LINE_HEIGHT: f32 = 1.3;
/// メニューバーの高さの見積もり（文字 12 の行 + ボタンの縦の余白 6 + 帯の縦の余白 4 + 下の区切り線 1）
pub const MENU_BAR_HEIGHT: f32 = 12.0 * LINE_HEIGHT + 6.0 + 4.0 + crate::ui_style::SEPARATOR_HEIGHT;
/// 区切り線の項目の高さ（線 + 上下の余白）
const SEPARATOR_ITEM_HEIGHT: f32 = crate::ui_style::SEPARATOR_HEIGHT + 2.0 * SEPARATOR_PADDING;
/// 区切り線の上下の余白
pub const SEPARATOR_PADDING: f32 = 3.0;
/// メニューに最低限残す高さ（ウィンドウが極端に低いときでも 0 にしない）
const MIN_HEIGHT: f32 = 80.0;

/// メニューを置く領域（メニューを重ねる層）の大きさ。ウィンドウから、上のメニューバーと下のステータスバーを除く。
pub fn layer_area(window: Size) -> Size {
    Size::new(
        window.width,
        (window.height - MENU_BAR_HEIGHT - STATUS_BAR_HEIGHT).max(MIN_HEIGHT),
    )
}

/// メニューの置き場所
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// 領域の左端からメニューの左端まで
    pub left: f32,
    /// 領域の上端からメニューの上端まで
    pub top: f32,
    /// メニューの高さの上限（これを超える内容はスクロールする）
    pub max_height: f32,
}

/// 右クリックのメニューの置き場所。カーソルの位置を左上にして開き、はみ出すときは向きを変える。
///
/// - 右端に収まらなければ、カーソルの左側に開く（それでも収まらなければ左端に寄せる）。
/// - 下端に収まらなければ、カーソルの上側に開く（それでも収まらなければ、領域に収まる位置までずらす）。
/// - 内容が領域より高ければ、高さを領域に収まる値に制限する。
pub fn place_at_cursor(cursor: [f32; 2], menu: Size, area: Size) -> Placement {
    let max_height = (area.height - 2.0 * MARGIN).max(MIN_HEIGHT.min(area.height));
    let height = menu.height.min(max_height);

    let left = if cursor[0] + menu.width + MARGIN <= area.width {
        cursor[0].max(MARGIN)
    } else if cursor[0] - menu.width >= MARGIN {
        cursor[0] - menu.width
    } else {
        (area.width - menu.width - MARGIN).max(MARGIN)
    };

    let top = if cursor[1] + height + MARGIN <= area.height {
        cursor[1].max(MARGIN)
    } else if cursor[1] - height >= MARGIN {
        cursor[1] - height
    } else {
        (area.height - height - MARGIN).max(MARGIN)
    };

    Placement {
        left,
        top,
        max_height,
    }
}

/// ドロップダウンの置き場所。メニューバーのボタンの真下（領域の上端）に、`left` に合わせて置く。
/// 高さは、領域に収まる値に制限する。
pub fn place_dropdown(left: f32, menu: Size, area: Size) -> Placement {
    let max_height = (area.height - MARGIN).max(MIN_HEIGHT.min(area.height));
    let left = left
        .min((area.width - menu.width - MARGIN).max(0.0))
        .max(0.0);
    Placement {
        left,
        top: 0.0,
        max_height,
    }
}

/// メニューの内容（カードの余白を含む）の高さを見積もる
pub fn estimate_height(items: &[MenuItem], surface: Surface) -> f32 {
    let (spacing, padding) = (surface.spacing(), surface.padding());
    2.0 * padding + column_height(items, surface, spacing)
}

fn column_height(items: &[MenuItem], surface: Surface, spacing: f32) -> f32 {
    if items.is_empty() {
        return 0.0;
    }
    let sum: f32 = items.iter().map(|i| item_height(i, surface)).sum();
    sum + spacing * (items.len() - 1) as f32
}

fn text_height(size: f32) -> f32 {
    size * LINE_HEIGHT
}

fn item_height(item: &MenuItem, surface: Surface) -> f32 {
    match item {
        MenuItem::Command(c) => {
            let s = c.style;
            text_height(f32::from(s.size)) + 2.0 * f32::from(s.padding[0])
        }
        MenuItem::Separator => SEPARATOR_ITEM_HEIGHT,
        MenuItem::Heading(_) => heading_height(surface),
        MenuItem::Header { .. } => text_height(11.0) + text_height(13.0) + 2.0,
        MenuItem::Group { layout, items } => match layout {
            GroupLayout::Row { .. } => items
                .iter()
                .map(|i| item_height(i, surface))
                .fold(0.0, f32::max),
            GroupLayout::Column { spacing } => column_height(items, surface, f32::from(*spacing)),
        },
        MenuItem::Submenu { items, .. } => {
            heading_height(surface)
                + surface.submenu_spacing()
                + column_height(items, surface, surface.submenu_spacing())
        }
    }
}

fn heading_height(surface: Surface) -> f32 {
    match surface {
        // 文字 11 + 縦の余白 2 + 2
        Surface::Dropdown => text_height(11.0) + 4.0,
        Surface::Card => text_height(11.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Size = Size::new(800.0, 600.0);
    const MENU: Size = Size::new(200.0, 300.0);

    #[test]
    fn opens_at_the_cursor_in_the_middle() {
        let p = place_at_cursor([300.0, 200.0], MENU, AREA);
        assert_eq!((p.left, p.top), (300.0, 200.0));
        assert_eq!(p.max_height, 600.0 - 2.0 * MARGIN);
    }

    #[test]
    fn shifts_into_the_window_at_the_bottom_right_corner() {
        let p = place_at_cursor([790.0, 590.0], MENU, AREA);
        // 左側・上側に開く
        assert_eq!((p.left, p.top), (590.0, 290.0));
        assert!(p.left + MENU.width + MARGIN <= AREA.width);
        assert!(p.top + MENU.height + MARGIN <= AREA.height);
    }

    #[test]
    fn stays_inside_when_neither_side_has_room() {
        // 幅 200 のメニューを、幅 220 の領域の中ほどで開く。左右どちらにも収まらない
        let area = Size::new(220.0, 300.0);
        let p = place_at_cursor([110.0, 150.0], Size::new(200.0, 250.0), area);
        assert!(p.left >= MARGIN && p.left + 200.0 <= area.width);
        assert!(p.top >= MARGIN && p.top + 250.0 + MARGIN <= area.height + 0.001);
    }

    #[test]
    fn limits_the_height_when_larger_than_the_window() {
        let tall = Size::new(200.0, 2000.0);
        let p = place_at_cursor([300.0, 300.0], tall, AREA);
        assert_eq!(p.max_height, AREA.height - 2.0 * MARGIN);
        // 制限した高さで、領域に収まる位置になる
        assert!(p.top >= MARGIN);
        assert!(p.top + p.max_height + MARGIN <= AREA.height);
    }

    #[test]
    fn never_places_outside_for_any_cursor() {
        for x in (0..=800).step_by(50) {
            for y in (0..=600).step_by(50) {
                for menu in [Size::new(240.0, 120.0), Size::new(270.0, 900.0)] {
                    let p = place_at_cursor([x as f32, y as f32], menu, AREA);
                    let h = menu.height.min(p.max_height);
                    assert!(p.left >= 0.0 && p.left + menu.width <= AREA.width);
                    assert!(p.top >= 0.0 && p.top + h <= AREA.height);
                }
            }
        }
    }

    #[test]
    fn dropdown_is_limited_to_the_window_height() {
        let low = Size::new(800.0, 150.0);
        let p = place_dropdown(60.0, Size::new(220.0, 400.0), low);
        assert_eq!((p.left, p.top), (60.0, 0.0));
        assert_eq!(p.max_height, 150.0 - MARGIN);
        let p = place_dropdown(60.0, Size::new(220.0, 100.0), AREA);
        assert_eq!(p.max_height, AREA.height - MARGIN);
    }

    #[test]
    fn dropdown_is_pulled_back_when_the_window_is_narrow() {
        let p = place_dropdown(190.0, Size::new(220.0, 100.0), Size::new(300.0, 400.0));
        assert!(p.left + 220.0 <= 300.0);
    }

    #[test]
    fn layer_area_excludes_the_bars_and_stays_positive() {
        let a = layer_area(Size::new(800.0, 600.0));
        assert_eq!(a.height, 600.0 - MENU_BAR_HEIGHT - STATUS_BAR_HEIGHT);
        assert!(layer_area(Size::new(800.0, 10.0)).height > 0.0);
    }

    #[test]
    fn estimated_height_grows_with_items() {
        use crate::menu::{builtin_items, location};
        let view = builtin_items(location::VIEW, None);
        let h = estimate_height(&view, Surface::Dropdown);
        assert!(h > 150.0 && h < 1000.0, "view menu height {h}");
        assert!(estimate_height(&view[..2], Surface::Dropdown) < h);
        assert_eq!(estimate_height(&[], Surface::Card), 20.0);
    }
}
