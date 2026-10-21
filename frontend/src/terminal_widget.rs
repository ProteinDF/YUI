//! 端末の画面を包むウィジェット（段階 2-1）。
//!
//! 画面の中身（枠、行、マウス）は、今のまま子が描き、処理も子へ渡す。木構造も変えない（`iced` の `Themer` と同じ書き方）。
//! 段階 2-2 で、ここが IME のイベント（`Event::InputMethod`）を受け、変換中の文字の位置を伝えるように足す。

use iced::advanced::input_method::{self, InputMethod, Purpose};
use iced::advanced::widget::Operation;
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Element, Event, Length, Rectangle, Size, Vector, window};

/// 日本語入力（IME）の出来事。変換中の文字（Preedit）と、確定した文字（Commit）など。
#[derive(Debug, Clone, PartialEq)]
pub enum ImeEvent {
    Opened,
    Preedit(String),
    Commit(String),
    Closed,
}

/// IME の設定（フォーカス中だけ、変換の窓の位置を伝える）
struct Ime<'a, Message> {
    focused: bool,
    /// 入力の位置（カーソルのセル）。ウィジェットの左上からの位置
    cursor: Rectangle,
    on_event: Box<dyn Fn(ImeEvent) -> Message + 'a>,
}

/// 端末の画面（子の要素をそのまま描き、処理を渡す）
pub struct TerminalScreen<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    ime: Option<Ime<'a, Message>>,
}

impl<'a, Message, Theme, Renderer> TerminalScreen<'a, Message, Theme, Renderer> {
    pub fn new(content: impl Into<Element<'a, Message, Theme, Renderer>>) -> Self {
        Self {
            content: content.into(),
            ime: None,
        }
    }

    /// IME を設定する。`focused` のとき、`cursor`（ウィジェットの左上からの位置）に変換の窓を合わせ、
    /// IME の出来事を `on_event` でメッセージにする。
    pub fn with_ime(
        mut self,
        focused: bool,
        cursor: Rectangle,
        on_event: impl Fn(ImeEvent) -> Message + 'a,
    ) -> Self {
        self.ime = Some(Ime {
            focused,
            cursor,
            on_event: Box::new(on_event),
        });
        self
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for TerminalScreen<'_, Message, Theme, Renderer>
where
    Renderer: iced::advanced::Renderer,
{
    fn tag(&self) -> tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> tree::State {
        self.content.as_widget().state()
    }

    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }

    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(tree, layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if let Some(ime) = &self.ime {
            match event {
                Event::InputMethod(input_method_event) => {
                    let ime_event = match input_method_event {
                        input_method::Event::Opened => ImeEvent::Opened,
                        input_method::Event::Preedit(text, _) => ImeEvent::Preedit(text.clone()),
                        input_method::Event::Commit(text) => ImeEvent::Commit(text.clone()),
                        input_method::Event::Closed => ImeEvent::Closed,
                    };
                    shell.publish((ime.on_event)(ime_event));
                    shell.capture_event();
                }
                // IME の要求は、描画の時にだけ受け付けられる
                Event::Window(window::Event::RedrawRequested(_)) if ime.focused => {
                    let bounds = layout.bounds();
                    let cursor = Rectangle {
                        x: bounds.x + ime.cursor.x,
                        y: bounds.y + ime.cursor.y,
                        ..ime.cursor
                    };
                    shell.request_input_method(&InputMethod::<String>::Enabled {
                        cursor,
                        purpose: Purpose::Terminal,
                        preedit: None,
                    });
                }
                _ => {}
            }
        }

        self.content.as_widget_mut().update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content
            .as_widget()
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content
            .as_widget()
            .draw(tree, renderer, theme, style, layout, cursor, viewport);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(tree, layout, renderer, viewport, translation)
    }
}

impl<'a, Message, Theme, Renderer> From<TerminalScreen<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: iced::advanced::Renderer + 'a,
{
    fn from(screen: TerminalScreen<'a, Message, Theme, Renderer>) -> Self {
        Element::new(screen)
    }
}
