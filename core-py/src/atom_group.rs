use pyo3::prelude::*;
use yui_core::{AtomGroup, AtomGroupExt};

#[pyclass(name = "AtomGroup", module = "yui", from_py_object)]
#[derive(Clone)]
pub struct PyAtomGroup {
    pub(crate) inner: AtomGroup,
}

#[pymethods]
impl PyAtomGroup {
    #[new]
    pub fn new() -> Self {
        Self {
            inner: AtomGroup::with_name("empty"),
        }
    }

    /// Ala-Gly テストペプチドフィクスチャを作成する
    #[staticmethod]
    pub fn create_test_peptide() -> Self {
        let (ag, _sec_map) = AtomGroup::create_test_peptide();
        Self { inner: ag }
    }

    /// 平坦化キャッシュに変換した際の原子数と結合数を返す: (atom_count, bond_count)
    pub fn flat_counts(&self) -> (usize, usize) {
        let mol = self.inner.to_flat_molecule();
        (mol.atoms.len(), mol.bonds.len())
    }

    /// モデル数
    #[getter]
    pub fn model_count(&self) -> usize {
        self.inner.get_number_of_groups()
    }

    /// 全原子数
    #[getter]
    pub fn atom_count(&self) -> usize {
        self.inner.atom_count()
    }

    fn __repr__(&self) -> String {
        format!(
            "AtomGroup(models={}, total_atoms={})",
            self.model_count(),
            self.atom_count()
        )
    }
}
