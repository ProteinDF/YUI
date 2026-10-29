//! Reader for the plot JSON written by `ProteinDF_pytools` (`pdf-plot-pop.py --json`, UI-free).
//!
//! The format is `pdf-plot/1`. Every point names the ProteinDF atom it belongs to (1-based, in the
//! order of the ProteinDF input), and the file lists those atoms. This module only turns the file
//! into a [`PlotData`] whose [`PlotKey`] is the ProteinDF atom index; mapping that index to an atom
//! of the displayed structure is a separate step, because the two structures can differ.

use std::collections::BTreeSet;
use std::fmt;

use serde::Deserialize;

use crate::plot::{Axis, PlotData, PlotKey, Point, Series};

/// The only schema this reader understands.
pub const SCHEMA: &str = "pdf-plot/1";

/// One atom of the calculation, as listed in the JSON.
#[derive(Debug, Clone, PartialEq)]
pub struct PytoolsAtom {
    /// 1-based index in the ProteinDF input order (the key of the points).
    pub index: u32,
    pub element: String,
    pub label: String,
}

/// A plot read from a pytools JSON file.
#[derive(Debug, Clone, PartialEq)]
pub struct PytoolsPlot {
    pub title: String,
    /// Atoms of the calculation, sorted by `index`.
    pub atoms: Vec<PytoolsAtom>,
    /// Points are keyed by [`PlotKey`] = ProteinDF atom index.
    pub plot: PlotData,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PytoolsPlotError {
    /// Not valid JSON, or a required field is missing or has the wrong type.
    Malformed(String),
    /// `schema` is not [`SCHEMA`].
    UnsupportedSchema(String),
    /// `kind` is not `"scatter"`.
    UnsupportedKind(String),
    /// The content contradicts itself (duplicate atom, point of an unlisted atom, ...).
    Inconsistent(String),
}

impl fmt::Display for PytoolsPlotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(m) => write!(f, "malformed plot JSON: {m}"),
            Self::UnsupportedSchema(s) => {
                write!(
                    f,
                    "unsupported plot schema \"{s}\" (this version reads \"{SCHEMA}\")"
                )
            }
            Self::UnsupportedKind(k) => {
                write!(f, "unsupported plot kind \"{k}\" (only \"scatter\")")
            }
            Self::Inconsistent(m) => write!(f, "inconsistent plot JSON: {m}"),
        }
    }
}

impl std::error::Error for PytoolsPlotError {}

#[derive(Deserialize)]
struct RawHeader {
    schema: String,
    kind: Option<String>,
}

#[derive(Deserialize)]
struct RawPlot {
    kind: String,
    #[serde(default)]
    title: String,
    axes: RawAxes,
    atoms: Vec<RawAtom>,
    series: Vec<RawSeries>,
}

#[derive(Deserialize)]
struct RawAxes {
    x: RawAxis,
    y: RawAxis,
}

#[derive(Deserialize)]
struct RawAxis {
    label: String,
    unit: Option<String>,
}

#[derive(Deserialize)]
struct RawAtom {
    index: u32,
    element: String,
    #[serde(default)]
    label: String,
}

#[derive(Deserialize)]
struct RawSeries {
    name: String,
    points: Vec<RawPoint>,
}

#[derive(Deserialize)]
struct RawPoint {
    x: f64,
    y: f64,
    atom: u32,
}

/// Parse a pytools plot JSON.
///
/// Unknown fields are ignored. The schema is checked first, so a file of another version is
/// reported as such instead of as a missing field.
pub fn parse_pytools_plot(json: &str) -> Result<PytoolsPlot, PytoolsPlotError> {
    let header: RawHeader =
        serde_json::from_str(json).map_err(|e| PytoolsPlotError::Malformed(e.to_string()))?;
    if header.schema != SCHEMA {
        return Err(PytoolsPlotError::UnsupportedSchema(header.schema));
    }
    if let Some(kind) = header.kind.filter(|k| k != "scatter") {
        return Err(PytoolsPlotError::UnsupportedKind(kind));
    }
    let raw: RawPlot =
        serde_json::from_str(json).map_err(|e| PytoolsPlotError::Malformed(e.to_string()))?;
    if raw.kind != "scatter" {
        return Err(PytoolsPlotError::UnsupportedKind(raw.kind));
    }

    let mut seen = BTreeSet::new();
    for atom in &raw.atoms {
        if atom.index == 0 {
            return Err(PytoolsPlotError::Inconsistent(
                "atom index starts at 1, found 0".into(),
            ));
        }
        if !seen.insert(atom.index) {
            return Err(PytoolsPlotError::Inconsistent(format!(
                "atom {} is listed twice",
                atom.index
            )));
        }
    }

    let mut plot = PlotData::new(axis(raw.axes.x), axis(raw.axes.y));
    for series in raw.series {
        let mut points = Vec::with_capacity(series.points.len());
        for p in series.points {
            if !seen.contains(&p.atom) {
                return Err(PytoolsPlotError::Inconsistent(format!(
                    "series \"{}\" has a point of atom {}, which is not listed in atoms",
                    series.name, p.atom
                )));
            }
            points.push(Point {
                x: p.x,
                y: p.y,
                key: PlotKey(u64::from(p.atom)),
            });
        }
        plot.series.push(Series::new(series.name, points));
    }

    let mut atoms: Vec<PytoolsAtom> = raw
        .atoms
        .into_iter()
        .map(|a| PytoolsAtom {
            index: a.index,
            element: a.element,
            label: a.label,
        })
        .collect();
    atoms.sort_by_key(|a| a.index);

    Ok(PytoolsPlot {
        title: raw.title,
        atoms,
        plot,
    })
}

/// Axis label with the unit appended: `"charge"` + `"e"` gives `"charge [e]"`.
fn axis(raw: RawAxis) -> Axis {
    match raw.unit.filter(|u| !u.is_empty()) {
        Some(unit) => Axis::new(format!("{} [{unit}]", raw.label)),
        None => Axis::new(raw.label),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file shaped like `pdf-plot-pop.py --json` output (H2O plus a dummy atom).
    const SAMPLE: &str = r#"{
      "schema": "pdf-plot/1",
      "kind": "scatter",
      "title": "Mulliken charge difference",
      "source": {"tool": "pdf-plot-pop.py", "result": "pdfresults.h5",
                 "run_type": "rks", "iteration": 5, "reference_iteration": 1},
      "axes": {"x": {"label": "atom index", "unit": null},
               "y": {"label": "Mulliken charge difference", "unit": "e"}},
      "atoms": [
        {"index": 1, "element": "O", "label": "", "chain": null, "residue_number": null,
         "residue_name": null, "atom_name": null},
        {"index": 2, "element": "H", "label": "", "chain": null, "residue_number": null,
         "residue_name": null, "atom_name": null},
        {"index": 3, "element": "H", "label": "", "chain": null, "residue_number": null,
         "residue_name": null, "atom_name": null}
      ],
      "series": [{"name": "Mulliken (diff)", "points": [
        {"x": 1, "y": -0.31, "atom": 1},
        {"x": 2, "y": 0.155, "atom": 2},
        {"x": 3, "y": 0.155, "atom": 3}
      ]}]
    }"#;

    fn replace(from: &str, to: &str) -> String {
        assert!(SAMPLE.contains(from), "sample lacks {from}");
        SAMPLE.replacen(from, to, 1)
    }

    #[test]
    fn reads_the_sample() {
        let plot = parse_pytools_plot(SAMPLE).unwrap();
        assert_eq!(plot.title, "Mulliken charge difference");
        assert_eq!(plot.atoms.len(), 3);
        assert_eq!(plot.atoms[0].element, "O");
        assert_eq!(plot.plot.x_axis.label, "atom index");
        assert_eq!(plot.plot.y_axis.label, "Mulliken charge difference [e]");
        assert_eq!(plot.plot.series.len(), 1);
        let points = &plot.plot.series[0].points;
        assert_eq!(points.len(), 3);
        assert_eq!(points[0].key, PlotKey(1));
        assert_eq!(points[0].y, -0.31);
        assert_eq!(points[2].key, PlotKey(3));
    }

    #[test]
    fn rejects_another_schema() {
        let err = parse_pytools_plot(&replace("pdf-plot/1", "pdf-plot/2")).unwrap_err();
        assert_eq!(
            err,
            PytoolsPlotError::UnsupportedSchema("pdf-plot/2".into())
        );
        assert!(err.to_string().contains("pdf-plot/2"));
    }

    #[test]
    fn reports_the_schema_before_other_problems() {
        // A future version may change every other field.
        let err = parse_pytools_plot(r#"{"schema": "pdf-plot/9", "shape": 1}"#).unwrap_err();
        assert!(matches!(err, PytoolsPlotError::UnsupportedSchema(_)));
    }

    #[test]
    fn rejects_other_kinds() {
        let err = parse_pytools_plot(&replace("\"scatter\"", "\"bar\"")).unwrap_err();
        assert_eq!(err, PytoolsPlotError::UnsupportedKind("bar".into()));
    }

    #[test]
    fn rejects_non_json_and_missing_fields() {
        assert!(matches!(
            parse_pytools_plot("not json"),
            Err(PytoolsPlotError::Malformed(_))
        ));
        assert!(matches!(
            parse_pytools_plot(r#"{"kind": "scatter"}"#),
            Err(PytoolsPlotError::Malformed(_))
        ));
        let no_series = SAMPLE.replace("\"series\"", "\"serie\"");
        assert!(matches!(
            parse_pytools_plot(&no_series),
            Err(PytoolsPlotError::Malformed(_))
        ));
    }

    #[test]
    fn rejects_a_point_of_an_unlisted_atom() {
        let err = parse_pytools_plot(&replace("\"atom\": 3", "\"atom\": 9")).unwrap_err();
        assert!(matches!(err, PytoolsPlotError::Inconsistent(m) if m.contains("atom 9")));
    }

    #[test]
    fn rejects_duplicate_and_zero_atom_index() {
        let dup = replace("\"index\": 3", "\"index\": 2");
        assert!(matches!(
            parse_pytools_plot(&dup),
            Err(PytoolsPlotError::Inconsistent(m)) if m.contains("twice")
        ));
        let zero = replace("\"index\": 1", "\"index\": 0");
        assert!(matches!(
            parse_pytools_plot(&zero),
            Err(PytoolsPlotError::Inconsistent(_))
        ));
    }

    #[test]
    fn ignores_unknown_fields_and_sorts_atoms() {
        let json = replace("\"atoms\": [", "\"future\": {\"a\": 1},\n \"atoms\": [")
            .replace("\"index\": 1", "\"index\": 7");
        // Atom 1 is gone, so its point must now be rejected; fix the point too.
        let json = json.replace("\"atom\": 1", "\"atom\": 7");
        let plot = parse_pytools_plot(&json).unwrap();
        let indices: Vec<u32> = plot.atoms.iter().map(|a| a.index).collect();
        assert_eq!(indices, vec![2, 3, 7]);
    }

    #[test]
    fn empty_series_is_allowed() {
        let json = r#"{"schema": "pdf-plot/1", "kind": "scatter",
          "axes": {"x": {"label": "x", "unit": null}, "y": {"label": "y", "unit": ""}},
          "atoms": [], "series": []}"#;
        let plot = parse_pytools_plot(json).unwrap();
        assert_eq!(plot.plot.point_count(), 0);
        assert_eq!(plot.plot.y_axis.label, "y");
        assert_eq!(plot.title, "");
    }
}
