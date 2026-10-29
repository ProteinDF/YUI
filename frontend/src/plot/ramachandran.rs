//! Ramachandran プロットのアダプタ。点は残基 1 つに対応し、鍵は残基の最初の原子の ID。
//!
//! 点の作り方（φ/ψ の軸、Gly・Pro・その他の系列）は `yui_core::ramachandran` にある。
//! ここは、クリックされた点から残基の全原子を選び、選ばれている原子から点を強調する対応だけを持つ。

use std::collections::BTreeSet;
use std::rc::Rc;

use super::PlotAdapter;
use yui_core::plot::PlotKey;
use yui_core::ramachandran::{RamachandranPoint, plot_key};

pub struct RamachandranAdapter {
    points: Rc<Vec<RamachandranPoint>>,
}

impl RamachandranAdapter {
    pub fn new(points: Rc<Vec<RamachandranPoint>>) -> Self {
        Self { points }
    }
}

impl PlotAdapter for RamachandranAdapter {
    /// 残基の全原子（側鎖を含む）。3D で残基ごと選ばれる。
    fn selection_for(&self, key: PlotKey) -> Vec<u32> {
        self.points
            .iter()
            .find(|p| plot_key(p) == Some(key))
            .map(|p| p.atom_ids.clone())
            .unwrap_or_default()
    }

    /// 原子が 1 つでも選ばれている残基の点（残基の一部だけが選ばれていても強調する）。
    fn keys_for(&self, selection: &BTreeSet<u32>) -> BTreeSet<PlotKey> {
        self.points
            .iter()
            .filter(|p| p.atom_ids.iter().any(|id| selection.contains(id)))
            .filter_map(plot_key)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(key: &str, ids: &[u32]) -> RamachandranPoint {
        RamachandranPoint {
            model_key: "model_1".into(),
            chain_id: "A".into(),
            residue_key: key.into(),
            residue_index: key.parse().unwrap_or(0),
            residue_name: "ALA".into(),
            phi: Some(-60.0),
            psi: Some(-45.0),
            atom_ids: ids.to_vec(),
        }
    }

    fn adapter() -> RamachandranAdapter {
        RamachandranAdapter::new(Rc::new(vec![
            point("1", &[1, 2, 3]),
            point("2", &[4, 5, 6, 7]),
            point("3", &[]),
        ]))
    }

    #[test]
    fn a_click_selects_every_atom_of_the_residue() {
        assert_eq!(adapter().selection_for(PlotKey(4)), vec![4, 5, 6, 7]);
        assert_eq!(adapter().selection_for(PlotKey(1)), vec![1, 2, 3]);
    }

    #[test]
    fn an_unknown_key_selects_nothing() {
        assert!(adapter().selection_for(PlotKey(5)).is_empty());
        assert!(adapter().selection_for(PlotKey(99)).is_empty());
    }

    #[test]
    fn any_selected_atom_highlights_its_residue() {
        let keys = adapter().keys_for(&[6].into());
        assert_eq!(keys, [PlotKey(4)].into());
        let keys = adapter().keys_for(&[1, 7].into());
        assert_eq!(keys, [PlotKey(1), PlotKey(4)].into());
        assert!(adapter().keys_for(&[100].into()).is_empty());
        assert!(adapter().keys_for(&BTreeSet::new()).is_empty());
    }

    #[test]
    fn clicking_a_point_then_highlighting_returns_the_same_point() {
        let a = adapter();
        for key in [PlotKey(1), PlotKey(4)] {
            let selection: BTreeSet<u32> = a.selection_for(key).into_iter().collect();
            assert_eq!(a.keys_for(&selection), [key].into());
        }
    }
}
