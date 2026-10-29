//! プロットの座標の計算（描画に依存しない純粋な関数）。
//!
//! 目盛りの刻み、データの座標から画面の座標への変換、画面の位置に最も近い点。

use iced::{Point, Rectangle};

/// `min..=max` に収まる、切りのよい目盛りの位置（およそ `count` 個）。
///
/// 刻みは 1・2・5 × 10 の冪。範囲が空・不正のとき、または `count` が 0 のときは空。
pub fn nice_ticks(min: f64, max: f64, count: usize) -> Vec<f64> {
    if count == 0 || !min.is_finite() || !max.is_finite() || max <= min {
        return Vec::new();
    }
    let raw = (max - min) / count as f64;
    let magnitude = 10f64.powf(raw.log10().floor());
    let norm = raw / magnitude;
    let step = magnitude
        * match norm {
            n if n <= 1.0 => 1.0,
            n if n <= 2.0 => 2.0,
            n if n <= 5.0 => 5.0,
            _ => 10.0,
        };
    // 浮動小数点の誤差で、端の目盛りを落とさない
    let eps = step * 1e-9;
    let first = ((min - eps) / step).ceil() as i64;
    let last = ((max + eps) / step).floor() as i64;
    (first..=last)
        .map(|k| {
            let v = k as f64 * step;
            // -0.0 を 0.0 にする
            if v == 0.0 { 0.0 } else { v }
        })
        .collect()
}

/// 目盛りの文字。刻みに合わせた小数の桁数にする。
pub fn format_tick(value: f64, step: f64) -> String {
    let decimals = if step > 0.0 && step.is_finite() {
        (-step.log10().floor()).clamp(0.0, 8.0) as usize
    } else {
        0
    };
    format!("{value:.decimals$}")
}

/// 2 つの目盛りの間隔（目盛りが 2 つ未満のときは `None`）
pub fn tick_step(ticks: &[f64]) -> Option<f64> {
    match ticks {
        [a, b, ..] => Some(b - a),
        _ => None,
    }
}

/// データの範囲と画面の長方形の対応。y は、画面では下向きなので反転する。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mapping {
    pub x: (f64, f64),
    pub y: (f64, f64),
    pub rect: Rectangle,
}

impl Mapping {
    pub fn to_screen(self, x: f64, y: f64) -> Point {
        let tx = (x - self.x.0) / (self.x.1 - self.x.0);
        let ty = (y - self.y.0) / (self.y.1 - self.y.0);
        Point::new(
            self.rect.x + tx as f32 * self.rect.width,
            self.rect.y + (1.0 - ty as f32) * self.rect.height,
        )
    }

    pub fn to_data(self, p: Point) -> (f64, f64) {
        let tx = f64::from((p.x - self.rect.x) / self.rect.width);
        let ty = 1.0 - f64::from((p.y - self.rect.y) / self.rect.height);
        (
            self.x.0 + tx * (self.x.1 - self.x.0),
            self.y.0 + ty * (self.y.1 - self.y.0),
        )
    }
}

/// `target` に最も近い点の番号。`max_dist` を超えるものは対象外（ちょうど等しいものは含む）。
/// 距離が同じときは、番号の小さい方。
pub fn nearest_point(target: Point, points: &[Point], max_dist: f32) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for (i, p) in points.iter().enumerate() {
        let d = p.distance(target);
        if d <= max_dist && best.is_none_or(|(_, b)| d < b) {
            best = Some((i, d));
        }
    }
    best.map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: &[f64], b: &[f64]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
    }

    #[test]
    fn ticks_use_round_steps() {
        assert!(close(
            &nice_ticks(0.0, 10.0, 5),
            &[0.0, 2.0, 4.0, 6.0, 8.0, 10.0]
        ));
        assert!(close(&nice_ticks(-180.0, 180.0, 4), &[-100.0, 0.0, 100.0]));
    }

    #[test]
    fn ticks_stay_inside_the_range() {
        let ticks = nice_ticks(-0.3, 9.7, 5);
        assert!(!ticks.is_empty());
        assert!(ticks.iter().all(|t| *t >= -0.3 && *t <= 9.7));
    }

    #[test]
    fn ticks_around_zero_have_no_negative_zero() {
        let ticks = nice_ticks(-180.0, 180.0, 4);
        assert!(ticks.contains(&0.0));
        assert!(ticks.iter().all(|t| !(*t == 0.0 && t.is_sign_negative())));
    }

    #[test]
    fn ticks_for_small_ranges() {
        let ticks = nice_ticks(0.0, 0.0005, 5);
        assert!(close(
            &ticks,
            &[0.0, 0.0001, 0.0002, 0.0003, 0.0004, 0.0005]
        ));
    }

    #[test]
    fn ticks_for_bad_input_are_empty() {
        assert!(nice_ticks(1.0, 1.0, 5).is_empty());
        assert!(nice_ticks(2.0, 1.0, 5).is_empty());
        assert!(nice_ticks(0.0, f64::NAN, 5).is_empty());
        assert!(nice_ticks(0.0, f64::INFINITY, 5).is_empty());
        assert!(nice_ticks(0.0, 1.0, 0).is_empty());
    }

    #[test]
    fn tick_text_follows_the_step() {
        assert_eq!(format_tick(2.0, 2.0), "2");
        assert_eq!(format_tick(0.5, 0.5), "0.5");
        assert_eq!(format_tick(0.0003, 0.0001), "0.0003");
        assert_eq!(format_tick(-180.0, 100.0), "-180");
        assert_eq!(tick_step(&[0.0, 0.5, 1.0]), Some(0.5));
        assert_eq!(tick_step(&[1.0]), None);
    }

    fn mapping() -> Mapping {
        Mapping {
            x: (0.0, 10.0),
            y: (-5.0, 5.0),
            rect: Rectangle::new(Point::new(100.0, 20.0), iced::Size::new(200.0, 100.0)),
        }
    }

    #[test]
    fn mapping_flips_y() {
        let m = mapping();
        assert_eq!(m.to_screen(0.0, 5.0), Point::new(100.0, 20.0));
        assert_eq!(m.to_screen(10.0, -5.0), Point::new(300.0, 120.0));
        assert_eq!(m.to_screen(5.0, 0.0), Point::new(200.0, 70.0));
    }

    #[test]
    fn mapping_round_trips() {
        let m = mapping();
        let p = m.to_screen(2.5, -3.0);
        let (x, y) = m.to_data(p);
        assert!((x - 2.5).abs() < 1e-4 && (y - -3.0).abs() < 1e-4);
    }

    #[test]
    fn nearest_picks_the_closest_within_the_limit() {
        let pts = [
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(4.0, 0.0),
        ];
        assert_eq!(nearest_point(Point::new(3.0, 0.0), &pts, 8.0), Some(2));
        assert_eq!(nearest_point(Point::new(9.0, 1.0), &pts, 8.0), Some(1));
    }

    #[test]
    fn nearest_ignores_points_beyond_the_limit() {
        let pts = [Point::new(0.0, 0.0)];
        assert_eq!(nearest_point(Point::new(20.0, 0.0), &pts, 8.0), None);
        assert_eq!(nearest_point(Point::new(8.0, 0.0), &pts, 8.0), Some(0));
        assert_eq!(nearest_point(Point::new(0.0, 0.0), &[], 8.0), None);
    }

    #[test]
    fn nearest_ties_go_to_the_lower_index() {
        let pts = [Point::new(-1.0, 0.0), Point::new(1.0, 0.0)];
        assert_eq!(nearest_point(Point::new(0.0, 0.0), &pts, 8.0), Some(0));
    }
}
