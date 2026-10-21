//! 書き出す画像（PNG、SVG、アニメーション）の、キャンバスの大きさと構図の設定。
//!
//! 形は、ウィンドウと同じ縦横比、決まった縦横比（1:1、4:3、3:2、16:9）、または幅と高さの指定（Custom）。
//! 構図は、画面と同じ視野角のまま（As on screen）か、構造全体がキャンバスに収まるようにカメラの距離を
//! 合わせるか（Fit all。向きは今のまま）。3 つのダイアログが同じ部品を使う。

use iced::widget::{row, text, text_input};
use iced::{Alignment, Element, Length};

use crate::ui_style::palette;
use crate::viewport::camera::fit_camera_to_points;
use yui_core::state::CameraState;

/// 画像の一辺の長さの上限（ピクセル）。ほとんどのアダプターのテクスチャの限界に収まる大きさ。
pub const MAX_SIDE: u32 = 16384;
/// 画像の一辺の長さの下限（ピクセル）
pub const MIN_SIDE: u32 = 16;

/// キャンバスの形
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanvasShape {
    /// 3D ペイン（分子図）と同じ縦横比。ペインの大きさが分からないとき（まだ描かれていない）は、ウィンドウの縦横比
    Pane,
    Square,
    R4x3,
    R3x2,
    R16x9,
    /// 幅と高さを直接指定する
    Custom,
}

impl CanvasShape {
    pub const ALL: [CanvasShape; 6] = [
        CanvasShape::Pane,
        CanvasShape::Square,
        CanvasShape::R4x3,
        CanvasShape::R3x2,
        CanvasShape::R16x9,
        CanvasShape::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            CanvasShape::Pane => "3D pane",
            CanvasShape::Square => "1:1",
            CanvasShape::R4x3 => "4:3",
            CanvasShape::R3x2 => "3:2",
            CanvasShape::R16x9 => "16:9",
            CanvasShape::Custom => "Custom",
        }
    }

    /// 幅 / 高さ（`Pane` は、3D ペインの値 `pane_aspect` を使う。`Custom` は決まらないので `None`）
    pub fn aspect(self, pane_aspect: f32) -> Option<f32> {
        match self {
            CanvasShape::Pane => Some(pane_aspect),
            CanvasShape::Square => Some(1.0),
            CanvasShape::R4x3 => Some(4.0 / 3.0),
            CanvasShape::R3x2 => Some(1.5),
            CanvasShape::R16x9 => Some(16.0 / 9.0),
            CanvasShape::Custom => None,
        }
    }
}

/// キャンバスの設定
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CanvasOptions {
    pub shape: CanvasShape,
    /// `Custom` のときの幅と高さ（0 は、入力欄が空）
    pub custom_width: u32,
    pub custom_height: u32,
    /// 構造全体がキャンバスに収まるよう、カメラの距離を合わせるか（`false` は、画面と同じ視野角のまま）
    pub fit_all: bool,
}

impl Default for CanvasOptions {
    fn default() -> Self {
        Self {
            shape: CanvasShape::Pane,
            custom_width: 1600,
            custom_height: 900,
            fit_all: false,
        }
    }
}

/// キャンバスの設定の操作
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanvasAction {
    Shape(CanvasShape),
    CustomWidth(u32),
    CustomHeight(u32),
    FitAll(bool),
}

impl CanvasOptions {
    pub fn apply(&mut self, action: CanvasAction) {
        match action {
            CanvasAction::Shape(s) => self.shape = s,
            CanvasAction::CustomWidth(v) => self.custom_width = v.min(MAX_SIDE * 10),
            CanvasAction::CustomHeight(v) => self.custom_height = v.min(MAX_SIDE * 10),
            CanvasAction::FitAll(v) => self.fit_all = v,
        }
    }

    /// 画像の大きさ（幅、高さ）。`width` は、`Custom` 以外のときの幅。`Custom` は、指定の幅と高さ。
    /// 範囲（`MIN_SIDE`〜`MAX_SIDE`）に入らなければ、理由つきのエラー。
    pub fn size(&self, width: u32, pane_aspect: f32) -> Result<(u32, u32), String> {
        let (w, h) = match self.shape.aspect(pane_aspect) {
            None => (self.custom_width, self.custom_height),
            Some(aspect) => {
                if !(aspect.is_finite() && aspect > 0.0) {
                    return Err("The view has no usable size.".to_string());
                }
                (width, (width as f32 / aspect).round() as u32)
            }
        };
        let ok = |v: u32| (MIN_SIDE..=MAX_SIDE).contains(&v);
        if !ok(w) || !ok(h) {
            return Err(format!(
                "The image size {w} x {h} is out of range ({MIN_SIDE} to {MAX_SIDE} px on each side)."
            ));
        }
        Ok((w, h))
    }

    /// 書き出しに使う視点。`fit_all` なら、向きは今の視点のまま、構造全体がキャンバス（縦横比 `aspect`）に
    /// 収まるように、注視点と距離を合わせる。`points` は、合わせる対象の原子の座標。
    pub fn camera(
        &self,
        base: &CameraState,
        points: impl IntoIterator<Item = [f32; 3]>,
        aspect: f32,
    ) -> CameraState {
        if self.fit_all {
            fit_keep_direction(base, points, aspect)
        } else {
            base.clone()
        }
    }
}

/// 今の視点の向きと up を保ったまま、`points` 全体が縦横比 `aspect` に収まる注視点と距離にする。
/// 座標がなければ、視点をそのまま返す。
pub fn fit_keep_direction(
    base: &CameraState,
    points: impl IntoIterator<Item = [f32; 3]>,
    aspect: f32,
) -> CameraState {
    let Some(fitted) = fit_camera_to_points(points, base.fov, aspect) else {
        return base.clone();
    };
    let dist = fitted.eye[2] - fitted.target[2]; // fit_camera_to_points は +z 側に置く
    let dir = crate::math::vec3_normalize(crate::math::vec3_sub(base.eye, base.target));
    if dir == [0.0; 3] {
        return base.clone();
    }
    CameraState {
        eye: [
            fitted.target[0] + dir[0] * dist,
            fitted.target[1] + dir[1] * dist,
            fitted.target[2] + dir[2] * dist,
        ],
        target: fitted.target,
        up: base.up,
        fov: base.fov,
    }
}

/// ダイアログの選択肢のボタン（選ばれているものは強調する）
pub fn chip<'a, Message: Clone + 'a>(
    label: String,
    selected: bool,
    message: Message,
) -> Element<'a, Message> {
    iced::widget::button(text(label).size(12))
        .padding([3, 10])
        .style(crate::ui_style::choice_button(selected))
        .on_press(message)
        .into()
}

/// 入力欄の文字を、数（数字以外は捨てる。空は 0）にする
fn parse_digits(s: &str) -> u32 {
    s.chars()
        .filter(char::is_ascii_digit)
        .take(6)
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}

fn number_text(v: u32) -> String {
    if v == 0 { String::new() } else { v.to_string() }
}

/// ダイアログに並べる、キャンバスの設定の行（形、Custom のときの大きさ、構図）。
/// `chip` は、ダイアログごとの見た目の選択肢のボタンを作る関数。
pub fn view_canvas_rows<'a, Message: Clone + 'a>(
    options: CanvasOptions,
    chip: impl Fn(String, bool, Message) -> Element<'a, Message> + Copy,
    on_action: impl Fn(CanvasAction) -> Message + Copy + 'a,
) -> Vec<Element<'a, Message>> {
    let label = |s: &'static str| text(s).size(12).width(Length::Fixed(80.0));
    let mut shape = row![label("Shape")].spacing(6).align_y(Alignment::Center);
    for s in CanvasShape::ALL {
        shape = shape.push(chip(
            s.label().to_string(),
            options.shape == s,
            on_action(CanvasAction::Shape(s)),
        ));
    }
    let mut rows: Vec<Element<'a, Message>> = vec![shape.into()];
    if options.shape == CanvasShape::Custom {
        let input = |placeholder: &'static str, value: u32, set: fn(u32) -> CanvasAction| {
            text_input(placeholder, &number_text(value))
                .size(12)
                .width(Length::Fixed(70.0))
                .on_input(move |s| on_action(set(parse_digits(&s))))
        };
        rows.push(
            row![
                label("Size"),
                input("width", options.custom_width, CanvasAction::CustomWidth),
                text("x").size(12),
                input("height", options.custom_height, CanvasAction::CustomHeight),
                text("px").size(12).color(palette().text_subtle),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
            .into(),
        );
    }
    rows.push(
        row![
            label("Framing"),
            chip(
                "As on screen".to_string(),
                !options.fit_all,
                on_action(CanvasAction::FitAll(false)),
            ),
            chip(
                "Fit all".to_string(),
                options.fit_all,
                on_action(CanvasAction::FitAll(true)),
            ),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into(),
    );
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam() -> CameraState {
        CameraState {
            eye: [0.0, 0.0, 30.0],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            fov: 45.0,
        }
    }

    #[test]
    fn test_size_from_the_shape_and_the_width() {
        let o = CanvasOptions::default();
        assert_eq!(o.size(2000, 4.0 / 3.0), Ok((2000, 1500)), "window aspect");
        let sq = CanvasOptions {
            shape: CanvasShape::Square,
            ..o
        };
        assert_eq!(sq.size(1000, 2.0), Ok((1000, 1000)));
        let wide = CanvasOptions {
            shape: CanvasShape::R16x9,
            ..o
        };
        assert_eq!(wide.size(1920, 1.0), Ok((1920, 1080)));
        // 縦横比が不正（ウィンドウが 0）
        assert!(o.size(2000, 0.0).is_err());
        assert!(o.size(2000, f32::NAN).is_err());
    }

    #[test]
    fn test_custom_size_is_used_as_given_and_checked() {
        let custom = CanvasOptions {
            shape: CanvasShape::Custom,
            custom_width: 1234,
            custom_height: 567,
            ..CanvasOptions::default()
        };
        assert_eq!(
            custom.size(9999, 1.0),
            Ok((1234, 567)),
            "the width chips are ignored"
        );
        let bad = |w, h| {
            CanvasOptions {
                custom_width: w,
                custom_height: h,
                ..custom
            }
            .size(1000, 1.0)
        };
        assert!(bad(0, 100).is_err() && bad(100, 0).is_err(), "empty field");
        assert!(bad(8, 100).is_err(), "too small");
        assert!(bad(16, 16).is_ok());
        assert!(bad(MAX_SIDE, MAX_SIDE).is_ok());
        assert!(bad(MAX_SIDE + 1, 100).is_err(), "too large");
        // 極端な縦横比の形（3D ペインが非常に細い）は、範囲外として断る
        assert!(CanvasOptions::default().size(1000, 0.001).is_err());
    }

    #[test]
    fn test_actions_change_the_options() {
        let mut o = CanvasOptions::default();
        o.apply(CanvasAction::Shape(CanvasShape::Custom));
        o.apply(CanvasAction::CustomWidth(800));
        o.apply(CanvasAction::CustomHeight(0));
        o.apply(CanvasAction::FitAll(true));
        assert_eq!(
            (o.shape, o.custom_width, o.custom_height, o.fit_all),
            (CanvasShape::Custom, 800, 0, true)
        );
        // 入力欄の文字: 数字以外は捨て、空は 0、長すぎる入力は 6 桁まで
        assert_eq!(parse_digits("12a3"), 123);
        assert_eq!(parse_digits(""), 0);
        assert_eq!(parse_digits("abc"), 0);
        assert_eq!(parse_digits("12345678"), 123456);
        assert_eq!(number_text(0), "");
        assert_eq!(number_text(42), "42");
    }

    #[test]
    fn test_fit_all_keeps_the_direction_and_contains_the_points_at_any_aspect() {
        // 横に長い点の集まり（x 方向に ±20 Å）
        let points = vec![[-20.0, 0.0, 0.0], [20.0, 0.0, 0.0], [0.0, 3.0, 0.0]];
        let mut base = cam();
        base.eye = [10.0, 10.0, 10.0]; // 斜めの向き
        let dir = crate::math::vec3_normalize(crate::math::vec3_sub(base.eye, base.target));

        // 縦長のキャンバスは、横長のキャンバスより遠くから見る（横幅が収まるように）
        let tall = fit_keep_direction(&base, points.clone(), 0.5);
        let wide = fit_keep_direction(&base, points.clone(), 2.0);
        let d = |c: &CameraState| {
            let v = crate::math::vec3_sub(c.eye, c.target);
            (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
        };
        assert!(d(&tall) > d(&wide), "{} vs {}", d(&tall), d(&wide));
        // 向きと up と視野角は変わらない。注視点は点の集まりの中心
        for c in [&tall, &wide] {
            let v = crate::math::vec3_normalize(crate::math::vec3_sub(c.eye, c.target));
            assert!((0..3).all(|i| (v[i] - dir[i]).abs() < 1e-5));
            assert_eq!((c.up, c.fov), (base.up, base.fov));
            assert!(c.target[0].abs() < 1e-5 && (c.target[1] - 1.5).abs() < 1e-5);
        }
        // 点がなければ、そのまま
        assert_eq!(fit_keep_direction(&base, Vec::new(), 1.0), base);
        // `fit_all` が false なら、視点は変わらない
        let o = CanvasOptions::default();
        assert_eq!(o.camera(&base, points.clone(), 0.5), base);
        let fit = CanvasOptions { fit_all: true, ..o };
        assert_eq!(fit.camera(&base, points, 0.5), tall);
    }

    #[test]
    fn test_rows_build_for_every_shape() {
        for shape in CanvasShape::ALL {
            let o = CanvasOptions {
                shape,
                ..CanvasOptions::default()
            };
            let rows = view_canvas_rows::<CanvasAction>(
                o,
                |label, _, action| iced::widget::button(text(label)).on_press(action).into(),
                |a| a,
            );
            assert_eq!(rows.len(), if shape == CanvasShape::Custom { 3 } else { 2 });
        }
    }
}
