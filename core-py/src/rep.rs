use pyo3::prelude::*;
use yui_core::{ColorScheme, Representation, RepresentationKind, Selector};

#[pyclass(name = "Representation", module = "yui", from_py_object)]
#[derive(Clone)]
pub struct PyRepresentation {
    pub(crate) inner: Representation,
}

#[pymethods]
impl PyRepresentation {
    #[new]
    #[pyo3(signature = (id="default".to_string(), kind="ball_and_stick", color="element", opacity=1.0, visible=true))]
    pub fn new(id: String, kind: &str, color: &str, opacity: f32, visible: bool) -> PyResult<Self> {
        let rep_kind = parse_kind(kind)?;
        let color_scheme = parse_color(color, None)?;
        Ok(Self {
            inner: Representation {
                id,
                selection: Selector::All,
                kind: rep_kind,
                color: color_scheme,
                opacity: opacity.clamp(0.0, 1.0),
                visible,
                interaction_kinds: Vec::new(),
                hides_atoms: false,
            },
        })
    }

    #[getter]
    pub fn id(&self) -> String {
        self.inner.id.clone()
    }

    #[getter]
    pub fn kind(&self) -> String {
        format!("{:?}", self.inner.kind).to_lowercase()
    }

    #[getter]
    pub fn color(&self) -> String {
        match self.inner.color {
            ColorScheme::Element => "element".to_string(),
            ColorScheme::Chain => "chain".to_string(),
            ColorScheme::Residue => "residue".to_string(),
            ColorScheme::SecondaryStructure => "secondary_structure".to_string(),
            ColorScheme::Uniform(c) => {
                format!("uniform({:.2},{:.2},{:.2},{:.2})", c[0], c[1], c[2], c[3])
            }
        }
    }

    #[getter]
    pub fn opacity(&self) -> f32 {
        self.inner.opacity
    }

    #[getter]
    pub fn visible(&self) -> bool {
        self.inner.visible
    }

    fn __repr__(&self) -> String {
        format!(
            "Representation(id='{}', kind='{}', color='{}', opacity={:.2}, visible={})",
            self.inner.id,
            self.kind(),
            self.color(),
            self.inner.opacity,
            self.inner.visible
        )
    }
}

pub(crate) fn parse_kind(kind: &str) -> PyResult<RepresentationKind> {
    match kind.to_lowercase().as_str() {
        "ball_and_stick" | "ballandstick" | "stick" => Ok(RepresentationKind::BallAndStick),
        "cartoon" | "ribbon" => Ok(RepresentationKind::Cartoon),
        "surface" => Ok(RepresentationKind::Surface),
        "licorice" => Ok(RepresentationKind::Licorice),
        "cpk" | "sphere" => Ok(RepresentationKind::Cpk),
        "line" | "wireframe" => Ok(RepresentationKind::Line),
        "interaction" | "interactions" => Ok(RepresentationKind::Interaction),
        other => Err(pyo3::exceptions::PyValueError::new_err(format!(
            "Unknown representation kind '{}'. Valid options: ball_and_stick, cartoon, surface, licorice, cpk, line, interaction",
            other
        ))),
    }
}

pub(crate) fn parse_color(
    scheme: &str,
    uniform_rgba: Option<(f32, f32, f32, f32)>,
) -> PyResult<ColorScheme> {
    match scheme.to_lowercase().as_str() {
        "element" => Ok(ColorScheme::Element),
        "chain" => Ok(ColorScheme::Chain),
        "residue" => Ok(ColorScheme::Residue),
        "secondary_structure" | "ss" => Ok(ColorScheme::SecondaryStructure),
        "uniform" => {
            let rgba = uniform_rgba.unwrap_or((0.7, 0.7, 0.7, 1.0));
            Ok(ColorScheme::Uniform([rgba.0, rgba.1, rgba.2, rgba.3]))
        }
        other => Err(pyo3::exceptions::PyValueError::new_err(format!(
            "Unknown color scheme '{}'. Valid options: element, chain, residue, secondary_structure, uniform",
            other
        ))),
    }
}
