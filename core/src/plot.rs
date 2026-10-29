//! Data model for generic plots (UI-free).
//!
//! A plot is a list of series; a series is a list of points. A point carries an opaque
//! [`PlotKey`] that only the code which built the plot interprets (for example, as an atom id).

use serde::{Deserialize, Serialize};

/// Fraction of the data span added on each side when an axis range is derived from the data.
pub const RANGE_MARGIN: f64 = 0.05;

/// Half-width of an axis range derived from data whose values are all equal (or from one point).
const FLAT_HALF_SPAN: f64 = 0.5;

/// Opaque identifier of a point. YUI never interprets the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PlotKey(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
    pub key: PlotKey,
}

/// A named list of points. Points with a non-finite coordinate are dropped and counted.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Series {
    pub name: String,
    pub points: Vec<Point>,
    /// Number of points dropped because `x` or `y` was NaN or infinite.
    pub skipped: usize,
}

impl Series {
    pub fn new(name: impl Into<String>, points: impl IntoIterator<Item = Point>) -> Self {
        let mut series = Self {
            name: name.into(),
            ..Self::default()
        };
        for point in points {
            if point.x.is_finite() && point.y.is_finite() {
                series.points.push(point);
            } else {
                series.skipped += 1;
            }
        }
        series
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Axis {
    pub label: String,
    /// Fixed range `(min, max)`. When `None`, the range is derived from the data.
    pub range: Option<(f64, f64)>,
}

impl Axis {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            range: None,
        }
    }

    pub fn with_range(mut self, min: f64, max: f64) -> Self {
        self.range = Some((min, max));
        self
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PlotData {
    pub x_axis: Axis,
    pub y_axis: Axis,
    pub series: Vec<Series>,
}

impl PlotData {
    pub fn new(x_axis: Axis, y_axis: Axis) -> Self {
        Self {
            x_axis,
            y_axis,
            series: Vec::new(),
        }
    }

    pub fn point_count(&self) -> usize {
        self.series.iter().map(|s| s.points.len()).sum()
    }

    /// Number of points dropped for a non-finite coordinate, over all series.
    pub fn skipped_count(&self) -> usize {
        self.series.iter().map(|s| s.skipped).sum()
    }

    /// Range of the x axis: the fixed range if set, otherwise the data span with a margin.
    pub fn x_range(&self) -> (f64, f64) {
        resolve_range(&self.x_axis, self.points().map(|p| p.x))
    }

    /// Range of the y axis (see [`PlotData::x_range`]).
    pub fn y_range(&self) -> (f64, f64) {
        resolve_range(&self.y_axis, self.points().map(|p| p.y))
    }

    fn points(&self) -> impl Iterator<Item = &Point> {
        self.series.iter().flat_map(|s| s.points.iter())
    }
}

fn resolve_range(axis: &Axis, values: impl Iterator<Item = f64>) -> (f64, f64) {
    if let Some((a, b)) = axis.range
        && a.is_finite()
        && b.is_finite()
        && a != b
    {
        return if a < b { (a, b) } else { (b, a) };
    }
    let Some((min, max)) = values.fold(None, |acc: Option<(f64, f64)>, v| match acc {
        None => Some((v, v)),
        Some((lo, hi)) => Some((lo.min(v), hi.max(v))),
    }) else {
        return (0.0, 1.0);
    };
    if min == max {
        return (min - FLAT_HALF_SPAN, max + FLAT_HALF_SPAN);
    }
    let margin = (max - min) * RANGE_MARGIN;
    (min - margin, max + margin)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: f64, y: f64, key: u64) -> Point {
        Point {
            x,
            y,
            key: PlotKey(key),
        }
    }

    fn plot(axis_x: Axis, axis_y: Axis, points: Vec<Point>) -> PlotData {
        let mut data = PlotData::new(axis_x, axis_y);
        data.series.push(Series::new("s", points));
        data
    }

    #[test]
    fn series_drops_and_counts_non_finite_points() {
        let series = Series::new(
            "s",
            [
                pt(1.0, 2.0, 1),
                pt(f64::NAN, 2.0, 2),
                pt(1.0, f64::INFINITY, 3),
                pt(f64::NEG_INFINITY, 0.0, 4),
                pt(3.0, 4.0, 5),
            ],
        );
        let keys: Vec<u64> = series.points.iter().map(|p| p.key.0).collect();
        assert_eq!(keys, vec![1, 5]);
        assert_eq!(series.skipped, 3);
    }

    #[test]
    fn counts_add_up_over_series() {
        let mut data = PlotData::default();
        data.series
            .push(Series::new("a", [pt(0.0, 0.0, 1), pt(f64::NAN, 0.0, 2)]));
        data.series.push(Series::new("b", [pt(1.0, 1.0, 3)]));
        data.series.push(Series::new("empty", []));
        assert_eq!(data.point_count(), 2);
        assert_eq!(data.skipped_count(), 1);
    }

    #[test]
    fn range_from_data_has_a_margin() {
        let data = plot(
            Axis::new("x"),
            Axis::new("y"),
            vec![pt(0.0, -10.0, 1), pt(10.0, 10.0, 2)],
        );
        let (x0, x1) = data.x_range();
        assert!((x0 - -0.5).abs() < 1e-12 && (x1 - 10.5).abs() < 1e-12);
        let (y0, y1) = data.y_range();
        assert!((y0 - -11.0).abs() < 1e-12 && (y1 - 11.0).abs() < 1e-12);
    }

    #[test]
    fn range_spans_all_series() {
        let mut data = PlotData::default();
        data.series.push(Series::new("a", [pt(0.0, 0.0, 1)]));
        data.series.push(Series::new("b", [pt(10.0, 0.0, 2)]));
        let (x0, x1) = data.x_range();
        assert!(x0 < 0.0 && x1 > 10.0);
    }

    #[test]
    fn fixed_range_wins_over_data() {
        let data = plot(
            Axis::new("x").with_range(-180.0, 180.0),
            Axis::new("y"),
            vec![pt(0.0, 0.0, 1), pt(1000.0, 0.0, 2)],
        );
        assert_eq!(data.x_range(), (-180.0, 180.0));
    }

    #[test]
    fn reversed_fixed_range_is_ordered() {
        let data = plot(Axis::new("x").with_range(5.0, 1.0), Axis::new("y"), vec![]);
        assert_eq!(data.x_range(), (1.0, 5.0));
    }

    #[test]
    fn unusable_fixed_range_falls_back_to_data() {
        for range in [(1.0, 1.0), (f64::NAN, 2.0), (0.0, f64::INFINITY)] {
            let data = plot(
                Axis::new("x").with_range(range.0, range.1),
                Axis::new("y"),
                vec![pt(0.0, 0.0, 1), pt(10.0, 0.0, 2)],
            );
            let (x0, x1) = data.x_range();
            assert!(x0 < 0.0 && x1 > 10.0, "{range:?}");
        }
    }

    #[test]
    fn flat_data_still_gets_a_nonempty_range() {
        let data = plot(Axis::new("x"), Axis::new("y"), vec![pt(3.0, 7.0, 1)]);
        assert_eq!(data.x_range(), (2.5, 3.5));
        assert_eq!(data.y_range(), (6.5, 7.5));
    }

    #[test]
    fn empty_plot_has_a_default_range() {
        let data = PlotData::default();
        assert_eq!(data.x_range(), (0.0, 1.0));
        assert_eq!(data.y_range(), (0.0, 1.0));
    }

    #[test]
    fn dropped_points_do_not_affect_the_range() {
        let data = plot(
            Axis::new("x"),
            Axis::new("y"),
            vec![pt(0.0, 0.0, 1), pt(1e300, f64::NAN, 2), pt(10.0, 0.0, 3)],
        );
        let (x0, x1) = data.x_range();
        assert!(x0 < 0.0 && x1 > 10.0 && x1 < 11.0);
    }

    #[test]
    fn keys_are_kept_as_given() {
        let series = Series::new("s", [pt(0.0, 0.0, u64::MAX)]);
        assert_eq!(series.points[0].key, PlotKey(u64::MAX));
    }
}
