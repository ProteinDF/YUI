use pyo3::prelude::*;
use yui_core::{ColorScheme, Representation, RepresentationKind, Selector};

use crate::atom_group::PyAtomGroup;
use crate::rep::{PyRepresentation, parse_color, parse_kind};
use crate::state::PyAppState;

#[pyclass(name = "Selector", module = "yui", from_py_object)]
#[derive(Clone)]
pub struct PySelector {
    pub(crate) inner: Selector,
}

#[pymethods]
impl PySelector {
    #[new]
    pub fn new(query: &Bound<'_, PyAny>) -> PyResult<Self> {
        let sel = extract_selector(query)?;
        Ok(Self { inner: sel })
    }

    /// 対象 AtomGroup に対して合致する全原子のIDリストを返す
    pub fn select_atom_ids(&self, atom_group: &PyAtomGroup) -> Vec<u32> {
        self.inner.select_atom_ids(&atom_group.inner)
    }

    fn __repr__(&self) -> String {
        format!("Selector({:?})", self.inner)
    }
}

/// メソッドチェーンで表現（Representation）を構築・適用するためのビルダー
#[pyclass(name = "SelectionBuilder", module = "yui", from_py_object)]
#[derive(Clone)]
pub struct SelectionBuilder {
    pub(crate) selector: Selector,
    pub(crate) kind: RepresentationKind,
    pub(crate) color: ColorScheme,
    pub(crate) opacity: f32,
    pub(crate) visible: bool,
    pub(crate) rep_id: String,
    pub(crate) app: Option<PyAppState>,
}

#[pymethods]
impl SelectionBuilder {
    /// 表現種別（cartoon, ball_and_stick, surface, licorice, cpk, line）を設定する
    pub fn show<'py>(slf: Bound<'py, Self>, kind: &str) -> PyResult<Bound<'py, Self>> {
        {
            let mut borrowed = slf.borrow_mut();
            borrowed.kind = parse_kind(kind)?;
            borrowed.sync_to_app()?;
        }
        Ok(slf)
    }

    /// 配色方式（element, chain, residue, secondary_structure, uniform）を設定する
    #[pyo3(signature = (scheme, color=None))]
    pub fn color_by<'py>(
        slf: Bound<'py, Self>,
        scheme: &str,
        color: Option<(f32, f32, f32, f32)>,
    ) -> PyResult<Bound<'py, Self>> {
        {
            let mut borrowed = slf.borrow_mut();
            borrowed.color = parse_color(scheme, color)?;
            borrowed.sync_to_app()?;
        }
        Ok(slf)
    }

    /// 不透明度 (0.0〜1.0) を設定する
    pub fn opacity<'py>(slf: Bound<'py, Self>, opacity: f32) -> PyResult<Bound<'py, Self>> {
        {
            let mut borrowed = slf.borrow_mut();
            borrowed.opacity = opacity.clamp(0.0, 1.0);
            borrowed.sync_to_app()?;
        }
        Ok(slf)
    }

    /// 可視状態 (True / False) を設定する
    pub fn visible<'py>(slf: Bound<'py, Self>, visible: bool) -> PyResult<Bound<'py, Self>> {
        {
            let mut borrowed = slf.borrow_mut();
            borrowed.visible = visible;
            borrowed.sync_to_app()?;
        }
        Ok(slf)
    }

    /// 表現のカスタムIDを設定する
    pub fn id<'py>(slf: Bound<'py, Self>, id: &str) -> PyResult<Bound<'py, Self>> {
        {
            let mut borrowed = slf.borrow_mut();
            borrowed.rep_id = id.to_string();
            borrowed.sync_to_app()?;
        }
        Ok(slf)
    }

    /// 対象の `AtomGroup` に対して合致する原子IDのリストを取得する
    pub fn select_atom_ids(&self, atom_group: &PyAtomGroup) -> Vec<u32> {
        self.selector.select_atom_ids(&atom_group.inner)
    }

    /// 内部で保持している `Selector` を取得する
    #[getter]
    pub fn selector(&self) -> PySelector {
        PySelector {
            inner: self.selector.clone(),
        }
    }

    /// 現在の設定から独立した `Representation` オブジェクトを生成する
    pub fn to_representation(&self) -> PyRepresentation {
        PyRepresentation {
            inner: Representation {
                id: self.rep_id.clone(),
                selection: self.selector.clone(),
                kind: self.kind,
                color: self.color.clone(),
                opacity: self.opacity,
                visible: self.visible,
                interaction_kinds: Vec::new(),
                hides_atoms: false,
            },
        }
    }

    /// 指定した `AppState` にこの表現を適用・追加する
    pub fn apply(&self, app: &Bound<'_, PyAppState>) -> PyResult<()> {
        let rep = self.to_representation();
        app.borrow().add_representation(&rep);
        Ok(())
    }

    fn __repr__(&self) -> String {
        format!(
            "SelectionBuilder(id='{}', kind={:?}, color={:?}, opacity={:.2})",
            self.rep_id, self.kind, self.color, self.opacity
        )
    }
}

impl SelectionBuilder {
    /// 紐づく `AppState` が存在する場合、その representations 内の対応エントリを更新または追加する
    fn sync_to_app(&self) -> PyResult<()> {
        if let Some(ref app) = self.app {
            let rep = Representation {
                id: self.rep_id.clone(),
                selection: self.selector.clone(),
                kind: self.kind,
                color: self.color.clone(),
                opacity: self.opacity,
                visible: self.visible,
                interaction_kinds: Vec::new(),
                hides_atoms: false,
            };
            let mut inner = app.inner.lock().unwrap();
            if let Some(existing) = inner
                .representations
                .iter_mut()
                .find(|r| r.id == self.rep_id)
            {
                *existing = rep;
            } else {
                inner.representations.push(rep);
            }
        }
        Ok(())
    }
}

/// クエリ文字列から `Selector` をパースする。
///
/// NOTE: `architecture.md` 6.2節に準拠し、コロン接頭辞等の独自ミニ言語は持たず、
/// 裸の文字列は常に `Selector::PathWildcard`（QCLObot `brd_select` 規約と整合）として扱います。
pub fn parse_selector_query(query: &str) -> PyResult<Selector> {
    let trimmed = query.trim();
    if trimmed.eq_ignore_ascii_case("all") || trimmed == "*" {
        Ok(Selector::All)
    } else if trimmed.eq_ignore_ascii_case("none") {
        Ok(Selector::None)
    } else {
        Ok(Selector::PathWildcard(trimmed.to_string()))
    }
}

/// `PyAny` オブジェクトから `Selector` を抽出・解決する共通ヘルパー。
/// `PySelector`, `SelectionBuilder`, または文字列（`parse_selector_query` 経由）を受け付けます。
pub fn extract_selector(item: &Bound<'_, PyAny>) -> PyResult<Selector> {
    if let Ok(sel) = item.extract::<PySelector>() {
        Ok(sel.inner)
    } else if let Ok(builder) = item.extract::<SelectionBuilder>() {
        Ok(builder.selector)
    } else if let Ok(s) = item.extract::<String>() {
        parse_selector_query(&s)
    } else {
        Err(pyo3::exceptions::PyTypeError::new_err(
            "Expected a Selector, SelectionBuilder, or string path wildcard query",
        ))
    }
}
