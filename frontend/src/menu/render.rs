//! メニューモデルから、画面（iced のウィジェット）を組み立てる。

use iced::widget::{button, column, container, mouse_area, row, scrollable, text};
use iced::{Element, Fill, Length, Padding, Size};

use super::command::CommandInvocation;
use super::model::{CommandItem, GroupLayout, ItemStyle, MenuItem};
use super::placement::{self, Placement};
use crate::ui_style::{self, UiPalette};

/// 項目を描く場所。見出しなどの見た目が、場所ごとに違う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// File / View / Help のドロップダウン
    Dropdown,
    /// 右クリックのメニューのカード
    Card,
}

impl Surface {
    /// 項目どうしの縦の間隔
    pub(super) fn spacing(self) -> f32 {
        match self {
            Surface::Dropdown => 2.0,
            Surface::Card => 8.0,
        }
    }

    /// カードの内側の余白
    pub(super) fn padding(self) -> f32 {
        match self {
            Surface::Dropdown => 4.0,
            Surface::Card => 10.0,
        }
    }

    /// 小見出しとその下の項目の縦の間隔
    pub(super) fn submenu_spacing(self) -> f32 {
        match self {
            Surface::Dropdown => 2.0,
            Surface::Card => 3.0,
        }
    }
}

/// メニューを開く位置の指定
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Anchor {
    /// 右クリックの位置（カーソルを左上にして開く。はみ出すときは向きを変える）
    Cursor([f32; 2]),
    /// メニューバーのボタンの真下。値は、領域の左端からメニューの左端まで
    Dropdown { left: f32 },
}

/// 項目を、縦に並べたウィジェットにする。押された項目のコマンドは `on_command` でメッセージにする。
fn render_items_in<'a, Message: Clone + 'a>(
    palette: UiPalette,
    items: &[MenuItem],
    surface: Surface,
    on_command: impl Fn(CommandInvocation) -> Message + Copy + 'a,
) -> Vec<Element<'a, Message>> {
    items
        .iter()
        .map(|item| render_item(palette, item, surface, on_command))
        .collect()
}

fn render_item<'a, Message: Clone + 'a>(
    palette: UiPalette,
    item: &MenuItem,
    surface: Surface,
    on_command: impl Fn(CommandInvocation) -> Message + Copy + 'a,
) -> Element<'a, Message> {
    match item {
        MenuItem::Command(c) => render_command(palette, c, on_command),
        MenuItem::Separator => container(ui_style::menu_separator(palette))
            .padding([placement::SEPARATOR_PADDING, 8.0])
            .into(),
        MenuItem::Heading(label) => match surface {
            Surface::Dropdown => container(text(label.clone()).size(11).color(palette.text_muted))
                .padding([2, 12])
                .into(),
            Surface::Card => text(label.clone())
                .size(11)
                .color(palette.text_muted)
                .into(),
        },
        MenuItem::Header { title, subtitle } => column![
            text(title.clone()).size(11).color(palette.text_muted),
            text(subtitle.clone()).size(13).color(palette.text),
        ]
        .spacing(2)
        .into(),
        MenuItem::Group { layout, items } => {
            let children = render_items_in(palette, items, surface, on_command);
            match layout {
                GroupLayout::Row { spacing } => row(children).spacing(f32::from(*spacing)).into(),
                GroupLayout::Column { spacing } => {
                    column(children).spacing(f32::from(*spacing)).into()
                }
            }
        }
        MenuItem::Submenu { label, items } => {
            let mut children = vec![render_item(
                palette,
                &MenuItem::Heading(label.clone()),
                surface,
                on_command,
            )];
            children.extend(render_items_in(palette, items, surface, on_command));
            column(children).spacing(surface.submenu_spacing()).into()
        }
    }
}

fn render_command<'a, Message: Clone + 'a>(
    palette: UiPalette,
    item: &CommandItem,
    on_command: impl Fn(CommandInvocation) -> Message + Copy + 'a,
) -> Element<'a, Message> {
    let ItemStyle {
        size,
        padding,
        fill,
        danger,
    } = item.style;
    let mut btn =
        button(text(item.display_label()).size(f32::from(size))).padding(Padding::from(padding));
    if fill {
        btn = btn.width(Fill);
    }
    btn = if danger {
        btn.style(move |_theme, status| ui_style::danger_button_style(&palette, status))
    } else {
        btn.style(move |_theme, status| ui_style::item_button_style(&palette, status))
    };
    if item.enabled {
        btn = btn.on_press(on_command(item.command.clone()));
    }
    btn.into()
}

/// 全面を覆う透明な層を作り、外側のクリックで閉じるようにしたうえで、メニューのカードを置く。
///
/// - 位置は `Anchor` から、ウィンドウに収まるように決める（`placement`）。
/// - 内容の高さがウィンドウに収まらないときだけ、カードの中をスクロールできるようにする。
/// - `width` はカードの幅（右端・下端から向きを変えるかどうかの計算に使うので、固定の値にする）。
pub fn popup_menu<'a, Message: Clone + 'a>(
    items: &[MenuItem],
    surface: Surface,
    width: f32,
    anchor: Anchor,
    window: Size,
    on_command: impl Fn(CommandInvocation) -> Message + Copy + 'a,
    on_close: Message,
) -> Element<'a, Message> {
    let palette = crate::ui_style::palette();
    let area = placement::layer_area(window);
    let height = placement::estimate_height(items, surface);
    let menu_size = Size::new(width, height);
    let Placement {
        left,
        top,
        max_height,
    } = match anchor {
        Anchor::Cursor(pos) => placement::place_at_cursor(pos, menu_size, area),
        Anchor::Dropdown { left } => placement::place_dropdown(left, menu_size, area),
    };

    let content = column(render_items_in(palette, items, surface, on_command))
        .spacing(surface.spacing())
        .padding(Padding::from(surface.padding()));
    // 見積もりが上限を超えるときだけスクロールにする（収まるときは、従来と同じ作り）
    let body: Element<'a, Message> = if height > max_height {
        scrollable(content).height(Length::Fill).into()
    } else {
        content.into()
    };
    let mut card = container(body)
        .width(width)
        .style(move |_theme| ui_style::menu_card_style(&palette));
    if height > max_height {
        card = card.height(max_height);
    } else {
        card = card.max_height(max_height);
    }

    let backdrop = mouse_area(
        container(iced::widget::Space::new())
            .width(Fill)
            .height(Fill),
    )
    .on_press(on_close);

    let positioned = container(card).width(Fill).height(Fill).padding(Padding {
        top,
        left,
        right: 0.0,
        bottom: 0.0,
    });

    iced::widget::stack![backdrop, positioned].into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::{builtin_items, location};

    /// 通常の高さと、ウィンドウに収まらない高さ（スクロールにする）のどちらでも、組み立てられること
    #[test]
    fn popup_builds_with_and_without_scrolling() {
        let items = builtin_items(location::CONTEXT_VIEWPORT, None);
        for window in [Size::new(1024.0, 768.0), Size::new(300.0, 120.0)] {
            for anchor in [
                Anchor::Cursor([290.0, 110.0]),
                Anchor::Dropdown { left: 60.0 },
            ] {
                let _: Element<'_, ()> =
                    popup_menu(&items, Surface::Card, 270.0, anchor, window, |_| (), ());
            }
        }
    }

    /// 低いウィンドウでは、見積もりの高さが上限を超える（= スクロールになる）こと
    #[test]
    fn tall_menu_needs_scrolling_in_a_low_window() {
        let items = builtin_items(location::VIEW, None);
        let height = placement::estimate_height(&items, Surface::Dropdown);
        let low = placement::layer_area(Size::new(800.0, 200.0));
        let p = placement::place_at_cursor([100.0, 50.0], Size::new(270.0, height), low);
        assert!(height > p.max_height);
        let tall = placement::layer_area(Size::new(800.0, 1200.0));
        let p = placement::place_at_cursor([100.0, 50.0], Size::new(270.0, height), tall);
        assert!(height <= p.max_height);
    }
}
