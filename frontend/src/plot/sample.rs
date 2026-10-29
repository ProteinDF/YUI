//! 開発用のサンプルのプロット: 原子の座標（x と y）。鎖ごとに 1 つの系列で、点は原子 1 つに対応する。
//!
//! タブ、メニュー、選択の連動を実際に通して確かめるためのもの。ProteinDF の結果などの本物のプロットは、
//! ここと同じ形のアダプタとして足す（ROADMAP 6l-ii、6l-iii）。

use std::collections::{BTreeMap, BTreeSet};

use super::PlotAdapter;
use yui_core::plot::{Axis, PlotData, PlotKey, Point, Series};
use yui_core::state::Atom;

/// 原子の座標の散布図
pub fn atom_positions(atoms: &[Atom]) -> PlotData {
    let mut by_chain: BTreeMap<&str, Vec<Point>> = BTreeMap::new();
    for atom in atoms {
        by_chain.entry(&atom.chain_id).or_default().push(Point {
            x: f64::from(atom.position[0]),
            y: f64::from(atom.position[1]),
            key: PlotKey(u64::from(atom.id)),
        });
    }
    let mut data = PlotData::new(Axis::new("x (Å)"), Axis::new("y (Å)"));
    for (chain, points) in by_chain {
        let name = if chain.is_empty() {
            "(no chain)".to_string()
        } else {
            format!("Chain {chain}")
        };
        data.series.push(Series::new(name, points));
    }
    data
}

/// 点の鍵が原子の ID そのもの
pub struct AtomAdapter;

impl PlotAdapter for AtomAdapter {
    fn selection_for(&self, key: PlotKey) -> Vec<u32> {
        u32::try_from(key.0).into_iter().collect()
    }

    fn keys_for(&self, selection: &BTreeSet<u32>) -> BTreeSet<PlotKey> {
        selection.iter().map(|id| PlotKey(u64::from(*id))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atom(id: u32, chain: &str, x: f32, y: f32) -> Atom {
        Atom {
            id,
            element: "C".into(),
            position: [x, y, 0.0],
            radius: 1.0,
            color: [1.0; 4],
            residue_name: "ALA".into(),
            residue_index: 1,
            chain_id: chain.into(),
        }
    }

    #[test]
    fn one_series_per_chain_and_one_point_per_atom() {
        let atoms = [
            atom(1, "A", 0.0, 1.0),
            atom(2, "B", 2.0, 3.0),
            atom(3, "A", 4.0, 5.0),
        ];
        let data = atom_positions(&atoms);
        assert_eq!(data.series.len(), 2);
        assert_eq!(data.series[0].name, "Chain A");
        assert_eq!(data.series[0].points.len(), 2);
        assert_eq!(data.series[1].points[0].key, PlotKey(2));
        assert_eq!(
            (data.series[1].points[0].x, data.series[1].points[0].y),
            (2.0, 3.0)
        );
    }

    #[test]
    fn no_atoms_give_an_empty_plot() {
        assert_eq!(atom_positions(&[]).point_count(), 0);
    }

    #[test]
    fn adapter_maps_both_ways() {
        assert_eq!(AtomAdapter.selection_for(PlotKey(7)), vec![7]);
        assert!(AtomAdapter.selection_for(PlotKey(u64::MAX)).is_empty());
        let keys = AtomAdapter.keys_for(&[1, 5].into());
        assert_eq!(keys, [1, 5].map(PlotKey).into());
    }
}
