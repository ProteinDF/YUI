//! pytools のグラフのアダプタ。点は ProteinDF の原子 1 つに対応し、鍵はその原子番号。
//!
//! 原子番号と表示中の構造の原子 ID の対応（`AtomMap`）は `yui_core::pytools_atom_map` が作る。
//! 対応が作れなかったとき（原子の数や元素が合わない）は、どの点も原子に結びつけない。

use std::collections::BTreeSet;
use std::rc::Rc;

use super::PlotAdapter;
use yui_core::plot::PlotKey;
use yui_core::pytools_atom_map::{AtomMap, AtomMapError};

pub struct PytoolsAdapter {
    map: Rc<Result<AtomMap, AtomMapError>>,
}

impl PytoolsAdapter {
    pub fn new(map: Rc<Result<AtomMap, AtomMapError>>) -> Self {
        Self { map }
    }
}

impl PlotAdapter for PytoolsAdapter {
    /// 点の原子（対応がなければ何も選ばない）
    fn selection_for(&self, key: PlotKey) -> Vec<u32> {
        let Ok(map) = self.map.as_ref() else {
            return Vec::new();
        };
        u32::try_from(key.0)
            .ok()
            .and_then(|index| map.yui_id(index))
            .into_iter()
            .collect()
    }

    /// 選ばれている原子の点（対応のない原子は無視する）
    fn keys_for(&self, selection: &BTreeSet<u32>) -> BTreeSet<PlotKey> {
        let Ok(map) = self.map.as_ref() else {
            return BTreeSet::new();
        };
        selection
            .iter()
            .filter_map(|id| map.plot_index(*id))
            .map(|index| PlotKey(u64::from(index)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yui_core::pytools_atom_map::map_pytools_atoms;
    use yui_core::pytools_plot::PytoolsAtom;
    use yui_core::structure_loader::parse_structure_bytes;

    const WATER: &str = "\
HETATM    1  O   HOH A   1       0.000   0.000   0.000  1.00  0.00           O
HETATM    2  H1  HOH A   1       0.960   0.000   0.000  1.00  0.00           H
HETATM    3  H2  HOH A   1      -0.240   0.930   0.000  1.00  0.00           H
END
";

    fn atoms(elements: &[&str]) -> Vec<PytoolsAtom> {
        elements
            .iter()
            .enumerate()
            .map(|(i, e)| PytoolsAtom {
                index: i as u32 + 1,
                element: (*e).into(),
                label: String::new(),
            })
            .collect()
    }

    fn adapter(elements: &[&str]) -> PytoolsAdapter {
        let root = parse_structure_bytes(WATER.as_bytes(), "water.pdb")
            .unwrap()
            .0
            .0;
        PytoolsAdapter::new(Rc::new(map_pytools_atoms(&root, None, &atoms(elements))))
    }

    #[test]
    fn a_point_selects_its_atom_and_a_selected_atom_lights_its_point() {
        let a = adapter(&["O", "H", "H"]);
        assert_eq!(a.selection_for(PlotKey(2)), vec![2]);
        assert_eq!(a.selection_for(PlotKey(9)), Vec::<u32>::new());
        let selection: BTreeSet<u32> = [1, 3].into();
        assert_eq!(a.keys_for(&selection), [PlotKey(1), PlotKey(3)].into());
        let unknown: BTreeSet<u32> = [42].into();
        assert!(a.keys_for(&unknown).is_empty());
    }

    #[test]
    fn without_a_map_nothing_is_linked() {
        let a = adapter(&["O", "H", "C"]);
        assert!(a.selection_for(PlotKey(1)).is_empty());
        assert!(a.keys_for(&[1].into()).is_empty());
    }
}
