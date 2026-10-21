use pyo3::prelude::*;
use std::sync::{Arc, Mutex};
use yui_core::{AppState, AtomGroup, AtomGroupExt, ColorScheme, Compression, RepresentationKind};

use crate::atom_group::PyAtomGroup;
use crate::rep::PyRepresentation;
use crate::selector::{SelectionBuilder, extract_selector};

#[pyclass(name = "AppState", module = "yui", from_py_object)]
#[derive(Clone)]
pub struct PyAppState {
    pub(crate) inner: Arc<Mutex<AppState>>,
}

#[pymethods]
impl PyAppState {
    #[new]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(AppState::default())),
        }
    }

    /// テスト用の Ala-Gly ペプチド構造をロードし、平坦化キャッシュと同期する
    pub fn load_test_peptide(&self) {
        let mut state = self.inner.lock().unwrap();
        let (ag, sec_map) = AtomGroup::create_test_peptide();
        state.atom_group = Some(ag);
        state.secondary_structure_map = sec_map;
        state.sync_molecule_from_atom_group();
    }

    /// セレクタを指定して表現構築ビルダー（SelectionBuilder）を開始する。
    /// 返されたビルダーで .show(...) や .color_by(...) を呼ぶと、自動的にこの AppState に反映される。
    pub fn select(&self, query: &Bound<'_, PyAny>) -> PyResult<SelectionBuilder> {
        let sel = extract_selector(query)?;
        let rep_id = {
            let state = self.inner.lock().unwrap();
            format!("rep_{}", state.representations.len() + 1)
        };
        Ok(SelectionBuilder {
            selector: sel,
            kind: RepresentationKind::BallAndStick,
            color: ColorScheme::Element,
            opacity: 1.0,
            visible: true,
            rep_id,
            app: Some(self.clone()),
        })
    }

    /// 表現（Representation）を明示的に追加する
    pub fn add_representation(&self, rep: &PyRepresentation) {
        let mut state = self.inner.lock().unwrap();
        state.representations.push(rep.inner.clone());
    }

    /// 登録済みの全表現リストを取得する
    #[getter]
    pub fn representations(&self) -> Vec<PyRepresentation> {
        let state = self.inner.lock().unwrap();
        state
            .representations
            .iter()
            .map(|r| PyRepresentation { inner: r.clone() })
            .collect()
    }

    /// 登録済みの表現を全消去する
    pub fn clear_representations(&self) {
        let mut state = self.inner.lock().unwrap();
        state.representations.clear();
    }

    /// 現在保持している `AtomGroup` を取得する（存在しない場合は None）
    #[getter]
    pub fn atom_group(&self) -> Option<PyAtomGroup> {
        let state = self.inner.lock().unwrap();
        state.atom_group.clone().map(|ag| PyAtomGroup { inner: ag })
    }

    /// 平坦化キャッシュ内の原子数・結合数を返す: (atom_count, bond_count)
    pub fn molecule_counts(&self) -> (usize, usize) {
        let state = self.inner.lock().unwrap();
        (state.molecule.atoms.len(), state.molecule.bonds.len())
    }

    /// `atom_group` から平坦化 `molecule` キャッシュを同期・再生成する
    pub fn sync_molecule(&self) {
        let mut state = self.inner.lock().unwrap();
        state.sync_molecule_from_atom_group();
    }

    /// YAML 文字列へエクスポートする
    pub fn to_yaml(&self) -> PyResult<String> {
        let state = self.inner.lock().unwrap();
        serde_yaml::to_string(&*state).map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Failed to export YAML: {}", e))
        })
    }

    /// YAML 文字列から状態をロードする
    #[allow(clippy::wrong_self_convention)] // Python APIとしてインスタンスへデシリアライズ結果を上書き反映するため&selfを保持
    pub fn from_yaml(&self, yaml_str: &str) -> PyResult<()> {
        let loaded: AppState = yui_core::import_app_state_from_yaml_str(yaml_str).map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Failed to parse YAML: {}", e))
        })?;
        let mut state = self.inner.lock().unwrap();
        *state = loaded;
        Ok(())
    }

    /// MessagePack バイナリ (バイト列) にシリアライズする
    pub fn to_msgpack(&self) -> PyResult<Vec<u8>> {
        let state = self.inner.lock().unwrap();
        yui_core::pack_app_state(&state, state.version as u8, Compression::None).map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Failed to pack MsgPack: {}", e))
        })
    }

    /// MessagePack バイナリから状態を復元する
    #[allow(clippy::wrong_self_convention)] // Python APIとしてインスタンスへデシリアライズ結果を上書き反映するため&selfを保持
    pub fn from_msgpack(&self, data: &[u8]) -> PyResult<()> {
        let (_ver, _comp, loaded) = yui_core::unpack_app_state(data).map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Failed to unpack MsgPack: {}", e))
        })?;
        let mut state = self.inner.lock().unwrap();
        *state = loaded;
        Ok(())
    }

    #[getter]
    pub fn version(&self) -> u32 {
        let state = self.inner.lock().unwrap();
        state.version
    }

    fn __repr__(&self) -> String {
        let state = self.inner.lock().unwrap();
        format!(
            "AppState(version={}, reps={}, has_atom_group={})",
            state.version,
            state.representations.len(),
            state.atom_group.is_some()
        )
    }
}
