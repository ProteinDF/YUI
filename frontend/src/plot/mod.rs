//! 汎用の散布図の描画（`yui_core::plot::PlotData` を描く。クリックはまだ扱わない）。
//!
//! 軸、目盛り、点を `canvas::Program` で描く。色は `ui_style::palette()` から取るので、両方のテーマで
//! 同じ部品を使える。`highlighted` に入っている点は、3D の選択と同じ色（`palette.selection`）で強調する。

pub mod scale;

use std::collections::BTreeSet;

use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke, Text};
use iced::{Element, Length, Pixels, Point, Rectangle, Renderer, Size, Theme, alignment, mouse};

use crate::ui_style::{UiPalette, palette};
use scale::{Mapping, format_tick, nice_ticks, tick_step};
use yui_core::plot::{PlotData, PlotKey};

/// 目盛りの数の目安（1 本の軸あたり）
const TICK_COUNT: usize = 6;
/// 点の半径と、強調された点の半径（ピクセル。強調には輪郭の線もつく）
const POINT_RADIUS: f32 = 3.0;
const HIGHLIGHT_RADIUS: f32 = 5.0;
/// 軸の外側の余白（ピクセル）。左と下には、目盛りの文字と軸の名前が入る。
const MARGIN_LEFT: f32 = 56.0;
const MARGIN_BOTTOM: f32 = 40.0;
const MARGIN_TOP: f32 = 22.0;
const MARGIN_RIGHT: f32 = 16.0;
const TEXT_SIZE: f32 = 12.0;

/// 系列ごとの色（順に取り、足りなければ繰り返す）
pub fn series_color(palette: &UiPalette, index: usize) -> iced::Color {
    let colors = [
        palette.accent,
        palette.chip_orange_bg,
        palette.chip_teal_bg,
        palette.gold,
        palette.chip_blue_bg,
        palette.danger_btn_bg,
    ];
    colors[index % colors.len()]
}

/// 軸の内側（点を描く長方形）。キャンバスが小さすぎるときは `None`。
pub fn inner_rect(bounds: Size) -> Option<Rectangle> {
    let width = bounds.width - MARGIN_LEFT - MARGIN_RIGHT;
    let height = bounds.height - MARGIN_TOP - MARGIN_BOTTOM;
    (width >= 1.0 && height >= 1.0).then(|| {
        Rectangle::new(
            Point::new(MARGIN_LEFT, MARGIN_TOP),
            Size::new(width, height),
        )
    })
}

pub struct PlotProgram<'a> {
    pub data: &'a PlotData,
    pub highlighted: &'a BTreeSet<PlotKey>,
}

/// 散布図の部品。親の大きさいっぱいに広がる。
pub fn view<'a, M: 'a>(data: &'a PlotData, highlighted: &'a BTreeSet<PlotKey>) -> Element<'a, M> {
    Canvas::new(PlotProgram { data, highlighted })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

impl<M> canvas::Program<M> for PlotProgram<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let palette = palette();
        let mut frame = Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), palette.panel_bg);
        let Some(rect) = inner_rect(bounds.size()) else {
            return vec![frame.into_geometry()];
        };
        let mapping = Mapping {
            x: self.data.x_range(),
            y: self.data.y_range(),
            rect,
        };
        draw_axes(&mut frame, &palette, self.data, &mapping);
        draw_points(&mut frame, &palette, self.data, self.highlighted, &mapping);
        vec![frame.into_geometry()]
    }
}

fn draw_axes(frame: &mut Frame, palette: &UiPalette, data: &PlotData, m: &Mapping) {
    let rect = m.rect;
    frame.stroke_rectangle(
        rect.position(),
        rect.size(),
        Stroke::default()
            .with_color(palette.separator)
            .with_width(1.0),
    );
    let x_ticks = nice_ticks(m.x.0, m.x.1, TICK_COUNT);
    let y_ticks = nice_ticks(m.y.0, m.y.1, TICK_COUNT);
    let x_step = tick_step(&x_ticks).unwrap_or(1.0);
    let y_step = tick_step(&y_ticks).unwrap_or(1.0);
    let grid = Stroke::default()
        .with_color(palette.separator.scale_alpha(0.4))
        .with_width(1.0);
    for t in &x_ticks {
        let x = m.to_screen(*t, m.y.0).x;
        frame.stroke(
            &Path::line(Point::new(x, rect.y), Point::new(x, rect.y + rect.height)),
            grid,
        );
        frame.fill_text(label(
            format_tick(*t, x_step),
            Point::new(x, rect.y + rect.height + 4.0),
            palette,
            alignment::Horizontal::Center,
            alignment::Vertical::Top,
        ));
    }
    for t in &y_ticks {
        let y = m.to_screen(m.x.0, *t).y;
        frame.stroke(
            &Path::line(Point::new(rect.x, y), Point::new(rect.x + rect.width, y)),
            grid,
        );
        frame.fill_text(label(
            format_tick(*t, y_step),
            Point::new(rect.x - 6.0, y),
            palette,
            alignment::Horizontal::Right,
            alignment::Vertical::Center,
        ));
    }
    frame.fill_text(label(
        data.x_axis.label.clone(),
        Point::new(rect.x + rect.width / 2.0, rect.y + rect.height + 22.0),
        palette,
        alignment::Horizontal::Center,
        alignment::Vertical::Top,
    ));
    // 縦軸の名前は、上の左端に横書きで置く（回転した文字は使わない）
    frame.fill_text(label(
        data.y_axis.label.clone(),
        Point::new(rect.x, 0.0),
        palette,
        alignment::Horizontal::Left,
        alignment::Vertical::Top,
    ));
}

fn label(
    content: String,
    position: Point,
    palette: &UiPalette,
    align_x: alignment::Horizontal,
    align_y: alignment::Vertical,
) -> Text {
    Text {
        content,
        position,
        color: palette.text_secondary,
        size: Pixels(TEXT_SIZE),
        align_x: align_x.into(),
        align_y,
        ..Text::default()
    }
}

fn draw_points(
    frame: &mut Frame,
    palette: &UiPalette,
    data: &PlotData,
    highlighted: &BTreeSet<PlotKey>,
    m: &Mapping,
) {
    let inside = |p: Point| m.rect.contains(p);
    // 強調された点を最後に描き、ほかの点に隠されないようにする
    for (i, series) in data.series.iter().enumerate() {
        let color = series_color(palette, i);
        for point in series
            .points
            .iter()
            .filter(|p| !highlighted.contains(&p.key))
        {
            let at = m.to_screen(point.x, point.y);
            if inside(at) {
                frame.fill(&Path::circle(at, POINT_RADIUS), color);
            }
        }
    }
    for series in &data.series {
        for point in series
            .points
            .iter()
            .filter(|p| highlighted.contains(&p.key))
        {
            let at = m.to_screen(point.x, point.y);
            if inside(at) {
                let circle = Path::circle(at, HIGHLIGHT_RADIUS);
                frame.fill(&circle, palette.selection);
                // 系列の色と近いテーマでも見分けられるように、輪郭をつける
                frame.stroke(
                    &circle,
                    Stroke::default().with_color(palette.text).with_width(1.5),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inner_rect_leaves_room_for_labels() {
        let r = inner_rect(Size::new(400.0, 300.0)).expect("a rectangle");
        assert_eq!(r.x, MARGIN_LEFT);
        assert_eq!(r.y, MARGIN_TOP);
        assert_eq!(r.width, 400.0 - MARGIN_LEFT - MARGIN_RIGHT);
        assert_eq!(r.height, 300.0 - MARGIN_TOP - MARGIN_BOTTOM);
    }

    #[test]
    fn inner_rect_is_none_when_too_small() {
        assert!(inner_rect(Size::new(50.0, 300.0)).is_none());
        assert!(inner_rect(Size::new(400.0, 40.0)).is_none());
        assert!(inner_rect(Size::ZERO).is_none());
    }

    #[test]
    fn series_colors_repeat() {
        let p = crate::ui_style::palette_for(crate::ui_style::ThemeKind::Dark);
        assert_eq!(series_color(&p, 0), series_color(&p, 6));
        assert_ne!(series_color(&p, 0), series_color(&p, 1));
    }

    fn sample() -> PlotData {
        use yui_core::plot::{Axis, Point as P, Series};
        let wave = |name: &str, phase: f64, base: u64| {
            Series::new(
                name,
                (0..24u32).map(|i| {
                    let x = f64::from(i) * 15.0 - 180.0;
                    P {
                        x,
                        y: 150.0 * (x.to_radians() + phase).sin(),
                        key: PlotKey(base + u64::from(i)),
                    }
                }),
            )
        };
        let mut data = PlotData::new(
            Axis::new("phi (deg)").with_range(-180.0, 180.0),
            Axis::new("psi (deg)").with_range(-180.0, 180.0),
        );
        data.series.push(wave("a", 0.0, 0));
        data.series.push(wave("b", 1.5, 100));
        data
    }

    fn picture(kind: crate::ui_style::ThemeKind, name: &str) {
        let data = sample();
        let highlighted: BTreeSet<PlotKey> = [3, 4, 5, 110].map(PlotKey).into();
        crate::ui_style::set_theme(kind);
        let element: Element<'_, ()> = iced::widget::container(view(&data, &highlighted))
            .width(480)
            .height(320)
            .into();
        let mut ui = iced_test::simulator(element);
        let snapshot = ui.snapshot(&iced::Theme::Dark).expect("take a snapshot");
        if let Ok(dir) = std::env::var("YUI_UI_SHOTS_DIR") {
            let base = std::path::Path::new(&dir).join(name);
            let _ = std::fs::remove_file(base.with_extension("png"));
            std::fs::create_dir_all(&dir).expect("create the picture folder");
            snapshot.matches_image(&base).expect("write the picture");
        }
    }

    /// 両方のテーマで描ける（`YUI_UI_SHOTS_DIR` があれば、確認用の画像も書く）
    #[test]
    fn draws_in_both_themes() {
        let _gpu = crate::test_gpu::lock();
        picture(crate::ui_style::ThemeKind::Dark, "plot-dark");
        picture(crate::ui_style::ThemeKind::Light, "plot-light");
        crate::ui_style::set_theme(crate::ui_style::ThemeKind::Dark);
    }
}
