//! 端末の画面を、セル（行 × 列）の格子として描くウィジェット（段階 1）。
//!
//! 各セルは、列の幅（`cell_width`）と行の高さ（`cell_height`）の箱に描く。全角（2 列）の字は、2 列の箱の中央に置く。
//! 点字など、フォントに頼らずに描く文字は、後の段階で扱う。

use iced::advanced::layout::{self, Layout};
use iced::advanced::mouse;
use iced::advanced::renderer::{self, Quad};
use iced::advanced::text::Text;
use iced::advanced::widget::{Tree, Widget};
use iced::alignment::Vertical;
use iced::{Border, Color, Font, Length, Pixels, Point, Rectangle, Shadow, Size};

/// 1 セルの描き方（vt100 のセルから、前もって決めておく）
#[derive(Debug, Clone, PartialEq)]
pub struct GridCell {
    /// セルの文字（空なら、背景だけを描く）
    pub text: String,
    pub fg: Color,
    /// `None` なら、背景を描かない
    pub bg: Option<Color>,
    pub bold: bool,
    pub underline: bool,
    /// 全角（2 列を使う）の字か
    pub wide: bool,
}

/// 描く格子（行の列）と、セルの大きさ
pub struct TerminalGrid {
    pub rows: Vec<Vec<GridCell>>,
    pub cell_width: f32,
    pub cell_height: f32,
    pub font_size: f32,
    pub regular: Font,
    pub bold: Font,
    /// 全角（日本語など）の字のフォント。OS のフォントに頼らず、同梱の Noto Sans JP で描く
    pub wide: Font,
}

impl TerminalGrid {
    fn font_for(&self, bold: bool) -> Font {
        if bold { self.bold } else { self.regular }
    }
}

impl<Message, Theme> Widget<Message, Theme, iced::Renderer> for TerminalGrid {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, Length::Fill, Length::Fill)
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut iced::Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let origin = layout.bounds().position();
        let clip = layout.bounds();

        for (r, row) in self.rows.iter().enumerate() {
            let top = origin.y + r as f32 * self.cell_height;
            // 同じ見た目が続く文字は、1 本の文字列にまとめて描く（`rich_text` と同じ並び。字の間隔が、通常の文字列と同じになる）
            let mut run: Option<Run> = None;

            for (c, cell) in row.iter().enumerate() {
                let left = origin.x + c as f32 * self.cell_width;
                let width = if cell.wide {
                    2.0 * self.cell_width
                } else {
                    self.cell_width
                };
                let cell_bounds =
                    Rectangle::new(Point::new(left, top), Size::new(width, self.cell_height));

                if let Some(bg) = cell.bg {
                    fill(renderer, cell_bounds, bg);
                }
                if cell.underline && !cell.text.is_empty() {
                    fill(
                        renderer,
                        Rectangle::new(
                            Point::new(left, top + self.cell_height - 2.0),
                            Size::new(width, 1.0),
                        ),
                        cell.fg,
                    );
                }

                // 点字: フォントを使わず、8 つの点を円で描く（1 列の幅に収まる）。並びは、ここで終える
                if let Some(pattern) = braille_pattern(&cell.text) {
                    if let Some(finished) = run.take() {
                        self.draw_run(renderer, &finished, top, clip);
                    }
                    draw_braille(renderer, pattern, cell_bounds, cell.fg);
                    continue;
                }

                // 全角: 2 列の箱の中央に、1 字だけ描く。それまでの並びは、ここで終える
                if cell.wide {
                    if let Some(finished) = run.take() {
                        self.draw_run(renderer, &finished, top, clip);
                    }
                    if !cell.text.is_empty() {
                        // 同梱の Noto Sans JP は 1 つの太さだけなので、太字の指定があっても同じフォントで描く
                        self.draw_text(
                            renderer,
                            &cell.text,
                            self.wide,
                            cell.fg,
                            cell_bounds,
                            iced::advanced::text::Alignment::Center,
                            clip,
                        );
                    }
                    continue;
                }

                // 2 列目（全角の後ろ）は、何も描かない。並びも終える
                if cell.text.is_empty() {
                    if let Some(finished) = run.take() {
                        self.draw_run(renderer, &finished, top, clip);
                    }
                    continue;
                }

                match run.as_mut() {
                    Some(current)
                        if current.fg == cell.fg
                            && current.bold == cell.bold
                            && current.underline == cell.underline =>
                    {
                        current.text.push_str(&cell.text);
                        current.cells += 1;
                    }
                    _ => {
                        if let Some(finished) = run.take() {
                            self.draw_run(renderer, &finished, top, clip);
                        }
                        run = Some(Run {
                            left,
                            text: cell.text.clone(),
                            cells: 1,
                            fg: cell.fg,
                            bold: cell.bold,
                            underline: cell.underline,
                        });
                    }
                }
            }

            if let Some(finished) = run.take() {
                self.draw_run(renderer, &finished, top, clip);
            }
        }
    }
}

/// 点字の 8 つの点の位置（セルの中の、左上からの割合）。点字のビット（1 の位が点 1）の順。
/// 点字は縦 4 × 横 2 の点（U+2800〜28FF）。点 1〜3 と 7 が左の列、点 4〜6 と 8 が右の列。
pub const BRAILLE_POSITIONS: [(f32, f32); 8] = [
    (0.3, 0.15),
    (0.3, 0.40),
    (0.3, 0.65),
    (0.7, 0.15),
    (0.7, 0.40),
    (0.7, 0.65),
    (0.3, 0.90),
    (0.7, 0.90),
];

/// 点字の 1 文字（U+2800〜28FF）なら、点のビット（0〜255）を返す。
pub fn braille_pattern(text: &str) -> Option<u8> {
    let mut chars = text.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    let code = u32::from(ch);
    if (0x2800..=0x28FF).contains(&code) {
        Some((code - 0x2800) as u8)
    } else {
        None
    }
}

/// 同じ見た目が続く文字の並び（1 行の中で、左から続く列）
struct Run {
    left: f32,
    text: String,
    cells: usize,
    fg: Color,
    bold: bool,
    underline: bool,
}

impl TerminalGrid {
    fn draw_run(&self, renderer: &mut iced::Renderer, run: &Run, top: f32, clip: Rectangle) {
        if run.text.chars().all(|ch| ch == ' ') {
            return;
        }
        // 箱は 1 セル分広く取る（ぴったりだと、丸めの誤差で最後の字が折り返される）。左寄せなので、位置は変わらない
        let bounds = Rectangle::new(
            Point::new(run.left, top),
            Size::new((run.cells as f32 + 1.0) * self.cell_width, self.cell_height),
        );
        self.draw_text(
            renderer,
            &run.text,
            self.font_for(run.bold),
            run.fg,
            bounds,
            iced::advanced::text::Alignment::Left,
            clip,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_text(
        &self,
        renderer: &mut iced::Renderer,
        content: &str,
        font: Font,
        color: Color,
        bounds: Rectangle,
        align_x: iced::advanced::text::Alignment,
        clip: Rectangle,
    ) {
        use iced::advanced::text::Renderer as _;
        renderer.fill_text(
            Text {
                content: content.to_string(),
                bounds: bounds.size(),
                size: Pixels(self.font_size),
                line_height: iced::advanced::text::LineHeight::Relative(1.3),
                font,
                align_x,
                align_y: Vertical::Top,
                shaping: iced::advanced::text::Shaping::Advanced,
                wrapping: iced::advanced::text::Wrapping::None,
            },
            // 中央寄せのとき、iced は位置を「文字の中心」として扱う。箱の左端ではなく、箱の中心の x を渡す
            if align_x == iced::advanced::text::Alignment::Center {
                Point::new(bounds.center_x(), bounds.y)
            } else {
                bounds.position()
            },
            color,
            clip,
        );
    }
}

/// 点字の点を、セルの中に円で描く
fn draw_braille(renderer: &mut iced::Renderer, pattern: u8, cell: Rectangle, color: Color) {
    use iced::advanced::renderer::Renderer as _;
    // 点の半径は、セルの幅に比例させる（横の間隔に収まり、読める大きさ）
    let radius = (cell.width * 0.14).clamp(0.6, 2.0);
    for (bit, (fx, fy)) in BRAILLE_POSITIONS.iter().enumerate() {
        if pattern & (1 << bit) == 0 {
            continue;
        }
        let cx = cell.x + fx * cell.width;
        let cy = cell.y + fy * cell.height;
        renderer.fill_quad(
            Quad {
                bounds: Rectangle::new(
                    Point::new(cx - radius, cy - radius),
                    Size::new(2.0 * radius, 2.0 * radius),
                ),
                border: Border {
                    radius: radius.into(),
                    ..Border::default()
                },
                shadow: Shadow::default(),
                snap: true,
            },
            color,
        );
    }
}

fn fill(renderer: &mut iced::Renderer, bounds: Rectangle, color: Color) {
    use iced::advanced::renderer::Renderer as _;
    renderer.fill_quad(
        Quad {
            bounds,
            border: Border::default(),
            shadow: Shadow::default(),
            snap: true,
        },
        color,
    );
}

impl<'a, Message: 'a> From<TerminalGrid> for iced::Element<'a, Message> {
    fn from(grid: TerminalGrid) -> Self {
        iced::Element::new(grid)
    }
}

#[cfg(test)]
mod tests {
    use super::{BRAILLE_POSITIONS, braille_pattern};

    #[test]
    fn test_braille_pattern_is_the_dot_bits() {
        assert_eq!(braille_pattern("\u{2800}"), Some(0));
        assert_eq!(braille_pattern("\u{2801}"), Some(1));
        assert_eq!(braille_pattern("\u{28FF}"), Some(0xFF));
        assert_eq!(braille_pattern("a"), None);
        assert_eq!(braille_pattern("\u{2801}\u{2802}"), None);
        assert_eq!(braille_pattern(""), None);
    }

    #[test]
    fn test_braille_positions_stay_inside_the_cell() {
        assert_eq!(BRAILLE_POSITIONS.len(), 8);
        for (x, y) in BRAILLE_POSITIONS {
            assert!((0.0..1.0).contains(&x) && (0.0..1.0).contains(&y));
        }
        // 点 1〜3 は左の列、点 4〜6 は右の列
        assert!(BRAILLE_POSITIONS[0].0 < BRAILLE_POSITIONS[3].0);
        assert!(BRAILLE_POSITIONS[2].1 < BRAILLE_POSITIONS[6].1);
    }
}
