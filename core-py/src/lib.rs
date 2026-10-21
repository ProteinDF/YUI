use pyo3::prelude::*;
use yui_core::{ColorScheme, RepresentationKind, Selector};

mod atom_group;
mod rep;
mod selector;
mod state;

use atom_group::PyAtomGroup;
use rep::PyRepresentation;
use selector::{PySelector, SelectionBuilder, extract_selector};
use state::PyAppState;

/// Python から `yui.select(...)` でセレクタビルダーを開始する。
/// 文字列（PathWildcard）、`PySelector`、または `SelectionBuilder` を受け付けます。
#[pyfunction]
#[pyo3(signature = (query))]
fn select(query: &Bound<'_, PyAny>) -> PyResult<SelectionBuilder> {
    let sel = extract_selector(query)?;
    Ok(SelectionBuilder {
        selector: sel,
        kind: RepresentationKind::BallAndStick,
        color: ColorScheme::Element,
        opacity: 1.0,
        visible: true,
        rep_id: "rep_custom".to_string(),
        app: None,
    })
}

/// 元素記号による選択 (例: yui.symbol("C", "N", "O"))
#[pyfunction]
#[pyo3(signature = (*elements))]
fn symbol(elements: Vec<String>) -> PySelector {
    PySelector {
        inner: Selector::Symbol(elements),
    }
}

/// 原子名による選択 (例: yui.name("CA", "CB"))
#[pyfunction]
#[pyo3(signature = (*names))]
fn name(names: Vec<String>) -> PySelector {
    PySelector {
        inner: Selector::Name(names),
    }
}

/// 残基名による選択 (例: yui.residue_name("ALA", "GLY"))
#[pyfunction]
#[pyo3(signature = (*names))]
fn residue_name(names: Vec<String>) -> PySelector {
    PySelector {
        inner: Selector::ResidueName(names),
    }
}

/// 残基番号範囲による選択 (例: yui.residue_range(1, 10, chain="A"))
#[pyfunction]
#[pyo3(signature = (start, end, chain=None))]
fn residue_range(start: i32, end: i32, chain: Option<String>) -> PySelector {
    PySelector {
        inner: Selector::ResidueRange { chain, start, end },
    }
}

/// 原子IDによる選択 (例: yui.atom_ids(1, 2, 3))
#[pyfunction]
#[pyo3(signature = (*ids))]
fn atom_ids(ids: Vec<u32>) -> PySelector {
    PySelector {
        inner: Selector::AtomIds(ids),
    }
}

/// 正規表現パスによる選択 (例: yui.path_regex(r"^/A/[0-9]+$"))
#[pyfunction]
#[pyo3(signature = (pattern))]
fn path_regex(pattern: String) -> PySelector {
    PySelector {
        inner: Selector::PathRegex(pattern),
    }
}

/// テスト用 Ala-Gly ペプチド構造を作成する
#[pyfunction]
fn create_test_peptide() -> PyAtomGroup {
    PyAtomGroup::create_test_peptide()
}

/// 複数のセレクタの AND 結合（すべてに合致）
#[pyfunction]
#[pyo3(signature = (*selectors))]
fn and_(selectors: Vec<Bound<'_, PyAny>>) -> PyResult<PySelector> {
    let mut inner_selectors = Vec::new();
    for item in selectors {
        inner_selectors.push(extract_selector(&item)?);
    }
    Ok(PySelector {
        inner: Selector::And(inner_selectors),
    })
}

/// 複数のセレクタの OR 結合（いずれかに合致）
#[pyfunction]
#[pyo3(signature = (*selectors))]
fn or_(selectors: Vec<Bound<'_, PyAny>>) -> PyResult<PySelector> {
    let mut inner_selectors = Vec::new();
    for item in selectors {
        inner_selectors.push(extract_selector(&item)?);
    }
    Ok(PySelector {
        inner: Selector::Or(inner_selectors),
    })
}

/// セレクタの NOT 反転（合致しない）
#[pyfunction]
#[pyo3(signature = (selector))]
fn not_(selector: &Bound<'_, PyAny>) -> PyResult<PySelector> {
    let sub = extract_selector(selector)?;
    Ok(PySelector {
        inner: Selector::Not {
            inner: Box::new(sub),
        },
    })
}

/// Python モジュール `yui`
#[pymodule]
pub fn yui(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyAppState>()?;
    m.add_class::<PyAtomGroup>()?;
    m.add_class::<PySelector>()?;
    m.add_class::<PyRepresentation>()?;
    m.add_class::<SelectionBuilder>()?;

    m.add_function(wrap_pyfunction!(select, m)?)?;
    m.add_function(wrap_pyfunction!(create_test_peptide, m)?)?;
    m.add_function(wrap_pyfunction!(symbol, m)?)?;
    m.add_function(wrap_pyfunction!(name, m)?)?;
    m.add_function(wrap_pyfunction!(residue_name, m)?)?;
    m.add_function(wrap_pyfunction!(residue_range, m)?)?;
    m.add_function(wrap_pyfunction!(atom_ids, m)?)?;
    m.add_function(wrap_pyfunction!(path_regex, m)?)?;
    m.add_function(wrap_pyfunction!(and_, m)?)?;
    m.add_function(wrap_pyfunction!(or_, m)?)?;
    m.add_function(wrap_pyfunction!(not_, m)?)?;

    Ok(())
}
