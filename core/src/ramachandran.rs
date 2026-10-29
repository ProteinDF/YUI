//! Ramachandran points (backbone dihedral angles φ / ψ) of a structure (UI-free).
//!
//! The angles are calculated by `proteindf-bridge` (`calc_phi_psi`); this module only walks the
//! structure tree, asks the bridge for each chain, and attaches to every residue the ids of its
//! atoms (the same ids as `iter_flat_atoms` / `Molecule::atoms`), so a caller can select a residue
//! without matching residue numbers again.

use std::collections::HashMap;

use crate::atom_group::{iter_chains, iter_flat_atoms, iter_models, parse_residue_key};
use crate::plot::{Axis, PlotData, PlotKey, Point, Series};
use proteindf_bridge::{AtomGroup, calc_phi_psi};

/// One residue with its backbone dihedral angles, in degrees.
#[derive(Debug, Clone, PartialEq)]
pub struct RamachandranPoint {
    pub model_key: String,
    /// Key of the chain in the tree (equal to `Atom::chain_id`).
    pub chain_id: String,
    /// Key of the residue in the chain, for example `"42"` or `"52A"`.
    pub residue_key: String,
    /// Number part of `residue_key` (equal to `Atom::residue_index`).
    pub residue_index: i32,
    pub residue_name: String,
    /// `None` for the first residue of a segment (or after a chain break).
    pub phi: Option<f64>,
    /// `None` for the last residue of a segment (or before a chain break).
    pub psi: Option<f64>,
    /// Ids of all atoms of this residue, in the order of `iter_flat_atoms`.
    pub atom_ids: Vec<u32>,
}

/// Ramachandran points of one model: `model_key`, or the first model when it is `None`.
///
/// Residues are listed chain by chain, in the order `calc_phi_psi` returns them. Residues that lack
/// `N`, `CA` or `C` have no point. An unknown `model_key` gives an empty list.
pub fn ramachandran_points(root: &AtomGroup, model_key: Option<&str>) -> Vec<RamachandranPoint> {
    let Some((model_key, model)) =
        iter_models(root).find(|(key, _)| model_key.is_none_or(|wanted| wanted == *key))
    else {
        return Vec::new();
    };

    let mut atom_ids: HashMap<(String, String), Vec<u32>> = HashMap::new();
    for flat in iter_flat_atoms(root).filter(|f| f.model_key == model_key) {
        atom_ids
            .entry((flat.chain_key.to_string(), flat.residue_key.to_string()))
            .or_default()
            .push(flat.id);
    }

    let mut points = Vec::new();
    for (chain_key, chain) in iter_chains(model) {
        for angle in calc_phi_psi(chain) {
            points.push(RamachandranPoint {
                model_key: model_key.to_string(),
                chain_id: chain_key.to_string(),
                residue_index: parse_residue_key(&angle.residue_key).0,
                atom_ids: atom_ids
                    .remove(&(chain_key.to_string(), angle.residue_key.clone()))
                    .unwrap_or_default(),
                residue_key: angle.residue_key,
                residue_name: angle.residue_name,
                phi: angle.phi,
                psi: angle.psi,
            });
        }
    }
    points
}

/// Key of the point of a residue in the plot: the id of its first atom (unique among residues,
/// and the same on every redraw). A residue without atoms has no key and is not plotted.
pub fn plot_key(point: &RamachandranPoint) -> Option<PlotKey> {
    point.atom_ids.first().map(|id| PlotKey(u64::from(*id)))
}

/// A Ramachandran plot and the number of residues that could not be plotted.
#[derive(Debug, Clone, PartialEq)]
pub struct RamachandranPlot {
    pub data: PlotData,
    /// Residues with a missing φ or ψ (segment ends, chain breaks) or without atoms.
    pub unplotted: usize,
}

/// Series names, in the order they are drawn.
pub const SERIES_GLY: &str = "Gly";
pub const SERIES_PRO: &str = "Pro";
pub const SERIES_OTHER: &str = "Other";

/// The plot: φ on x and ψ on y, both fixed to -180°..180°, one series each for Gly, Pro and the
/// other residues. A residue becomes a point only when it has both angles.
pub fn ramachandran_plot(points: &[RamachandranPoint]) -> RamachandranPlot {
    let mut gly = Vec::new();
    let mut pro = Vec::new();
    let mut other = Vec::new();
    let mut unplotted = 0;
    for p in points {
        let (Some(phi), Some(psi), Some(key)) = (p.phi, p.psi, plot_key(p)) else {
            unplotted += 1;
            continue;
        };
        let point = Point {
            x: phi,
            y: psi,
            key,
        };
        match p.residue_name.trim().to_ascii_uppercase().as_str() {
            "GLY" => gly.push(point),
            "PRO" => pro.push(point),
            _ => other.push(point),
        }
    }
    let mut data = PlotData::new(
        Axis::new("φ (deg)").with_range(-180.0, 180.0),
        Axis::new("ψ (deg)").with_range(-180.0, 180.0),
    );
    data.series.push(Series::new(SERIES_OTHER, other));
    data.series.push(Series::new(SERIES_GLY, gly));
    data.series.push(Series::new(SERIES_PRO, pro));
    RamachandranPlot { data, unplotted }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proteindf_bridge::{Atom, Position};

    type Xyz = [f64; 3];
    /// (residue key, residue name, N / CA / C positions)
    type ResidueSpec<'a> = (&'a str, &'a str, [Xyz; 3]);

    fn residue(name: &str, backbone: [Xyz; 3], with_ca: bool) -> AtomGroup {
        let mut res = AtomGroup::with_name(name);
        let atoms = [("N", "N"), ("CA", "C"), ("C", "C")];
        for ((atom_name, symbol), p) in atoms.into_iter().zip(backbone) {
            if atom_name == "CA" && !with_ca {
                continue;
            }
            let mut atom =
                Atom::new_with_pos(symbol, Position::new(p[0], p[1], p[2])).expect("valid symbol");
            atom.name = atom_name.to_string();
            res.set_atom(atom_name, atom);
        }
        res
    }

    /// A planar zigzag backbone along x: every peptide bond is trans, so φ = ψ = ±180°.
    /// `shift` moves the residue along x; `lift` moves its C along z.
    fn planar(shift: f64, lift: f64) -> [Xyz; 3] {
        [
            [shift, 0.0, 0.0],
            [shift + 1.4, 0.8, 0.0],
            [shift + 2.8, 0.0, lift],
        ]
    }

    fn model_of(chains: Vec<(&str, Vec<ResidueSpec>)>) -> AtomGroup {
        let mut model = AtomGroup::with_name("model");
        for (chain_key, residues) in chains {
            let mut chain = AtomGroup::with_name(chain_key);
            for (key, name, backbone) in residues {
                chain.set_group(key, residue(name, backbone, true));
            }
            model.set_group(chain_key, chain);
        }
        model
    }

    fn root_of(models: Vec<(&str, AtomGroup)>) -> AtomGroup {
        let mut root = AtomGroup::with_name("root");
        for (key, model) in models {
            root.set_group(key, model);
        }
        root.set_path("/".to_string());
        root
    }

    /// Three trans residues "1", "2", "3" in chain A, 3.9 Å apart along x.
    fn trio() -> AtomGroup {
        let residues = |lift: f64| {
            vec![
                ("1", "ALA", planar(0.0, 0.0)),
                ("2", "GLY", planar(3.9, lift)),
                ("3", "PRO", planar(7.8, 0.0)),
            ]
        };
        root_of(vec![("model_1", model_of(vec![("A", residues(0.0))]))])
    }

    fn near(value: Option<f64>, expected: f64) -> bool {
        value.is_some_and(|v| (v - expected).abs() < 1e-6)
    }

    fn is_flat(value: Option<f64>) -> bool {
        value.is_some_and(|v| (v.abs() - 180.0).abs() < 1e-6)
    }

    #[test]
    fn trans_backbone_gives_flat_angles_and_open_ends() {
        let points = ramachandran_points(&trio(), None);
        let keys: Vec<&str> = points.iter().map(|p| p.residue_key.as_str()).collect();
        assert_eq!(keys, ["1", "2", "3"]);
        assert_eq!(points[1].residue_name, "GLY");
        assert_eq!(points[1].residue_index, 2);
        assert_eq!(points[1].chain_id, "A");
        assert_eq!(points[1].model_key, "model_1");
        assert!(points[0].phi.is_none() && is_flat(points[0].psi));
        assert!(is_flat(points[1].phi) && is_flat(points[1].psi));
        assert!(is_flat(points[2].phi) && points[2].psi.is_none());
    }

    #[test]
    fn a_bent_backbone_gives_other_angles() {
        let root = root_of(vec![(
            "model_1",
            model_of(vec![(
                "A",
                vec![
                    ("1", "ALA", planar(0.0, 0.0)),
                    ("2", "ALA", planar(3.9, 1.0)),
                    ("3", "ALA", planar(7.8, 0.0)),
                ],
            )]),
        )]);
        let points = ramachandran_points(&root, None);
        let phi = points[1].phi.expect("phi");
        let psi = points[1].psi.expect("psi");
        assert!(phi.abs() < 170.0 && psi.abs() < 170.0, "{phi} {psi}");
        assert!(!near(points[1].phi, 180.0));
    }

    #[test]
    fn a_chain_break_removes_the_angles_across_it() {
        let root = root_of(vec![(
            "model_1",
            model_of(vec![(
                "A",
                vec![
                    ("1", "ALA", planar(0.0, 0.0)),
                    ("2", "ALA", planar(3.9, 0.0)),
                    ("3", "ALA", planar(17.8, 0.0)),
                ],
            )]),
        )]);
        let points = ramachandran_points(&root, None);
        assert!(is_flat(points[0].psi));
        assert!(points[1].psi.is_none(), "C(2) is far from N(3)");
        assert!(points[2].phi.is_none());
    }

    #[test]
    fn a_residue_without_ca_has_no_point_and_cuts_its_neighbours() {
        let mut model = model_of(vec![(
            "A",
            vec![
                ("1", "ALA", planar(0.0, 0.0)),
                ("3", "ALA", planar(7.8, 0.0)),
            ],
        )]);
        let mut chain = model.get_group("A").expect("chain").clone();
        chain.set_group("2", residue("GLY", planar(3.9, 0.0), false));
        model.set_group("A", chain);
        let points = ramachandran_points(&root_of(vec![("model_1", model)]), None);
        let keys: Vec<&str> = points.iter().map(|p| p.residue_key.as_str()).collect();
        assert_eq!(keys, ["1", "3"]);
        assert!(points[0].psi.is_none() && points[1].phi.is_none());
    }

    #[test]
    fn atom_ids_are_those_of_the_flat_molecule() {
        let root = trio();
        let points = ramachandran_points(&root, None);
        // 3 atoms per residue, numbered from 1 in tree order
        assert_eq!(points[0].atom_ids, [1, 2, 3]);
        assert_eq!(points[1].atom_ids, [4, 5, 6]);
        assert_eq!(points[2].atom_ids, [7, 8, 9]);
        for p in &points {
            for flat in iter_flat_atoms(&root).filter(|f| p.atom_ids.contains(&f.id)) {
                assert_eq!(flat.residue_key, p.residue_key);
                assert_eq!(flat.chain_key, p.chain_id);
                assert_eq!(flat.residue_index, p.residue_index);
            }
        }
    }

    #[test]
    fn an_insertion_code_is_a_separate_residue() {
        let root = root_of(vec![(
            "model_1",
            model_of(vec![(
                "A",
                vec![
                    ("52", "ALA", planar(0.0, 0.0)),
                    ("52A", "GLY", planar(3.9, 0.0)),
                    ("53", "ALA", planar(7.8, 0.0)),
                ],
            )]),
        )]);
        let points = ramachandran_points(&root, None);
        let by_key = |k: &str| points.iter().find(|p| p.residue_key == k).expect(k);
        assert_eq!(by_key("52").residue_index, 52);
        assert_eq!(by_key("52A").residue_index, 52);
        assert_ne!(by_key("52").atom_ids, by_key("52A").atom_ids);
        assert_eq!(by_key("52A").atom_ids.len(), 3);
    }

    #[test]
    fn the_model_is_chosen_by_key() {
        let one = model_of(vec![("A", vec![("1", "ALA", planar(0.0, 0.0))])]);
        let two = model_of(vec![(
            "B",
            vec![
                ("7", "GLY", planar(0.0, 0.0)),
                ("8", "GLY", planar(3.9, 0.0)),
            ],
        )]);
        let root = root_of(vec![("model_1", one), ("model_2", two)]);

        let first = ramachandran_points(&root, None);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].model_key, "model_1");

        let second = ramachandran_points(&root, Some("model_2"));
        assert_eq!(second.len(), 2);
        assert!(second.iter().all(|p| p.chain_id == "B"));
        // ids continue after the 3 atoms of model_1
        assert_eq!(second[0].atom_ids, [4, 5, 6]);

        assert!(ramachandran_points(&root, Some("model_9")).is_empty());
    }

    #[test]
    fn chains_are_listed_in_turn() {
        let root = root_of(vec![(
            "model_1",
            model_of(vec![
                ("A", vec![("1", "ALA", planar(0.0, 0.0))]),
                ("B", vec![("1", "ALA", planar(0.0, 0.0))]),
            ]),
        )]);
        let points = ramachandran_points(&root, None);
        let chains: Vec<&str> = points.iter().map(|p| p.chain_id.as_str()).collect();
        assert_eq!(chains, ["A", "B"]);
        assert_ne!(points[0].atom_ids, points[1].atom_ids);
    }

    #[test]
    fn an_empty_structure_gives_no_points() {
        assert!(ramachandran_points(&AtomGroup::new(), None).is_empty());
    }

    fn point(
        key: &str,
        name: &str,
        phi: Option<f64>,
        psi: Option<f64>,
        ids: &[u32],
    ) -> RamachandranPoint {
        RamachandranPoint {
            model_key: "model_1".into(),
            chain_id: "A".into(),
            residue_key: key.into(),
            residue_index: key.parse().unwrap_or(0),
            residue_name: name.into(),
            phi,
            psi,
            atom_ids: ids.to_vec(),
        }
    }

    #[test]
    fn the_plot_has_fixed_angle_axes_and_three_series() {
        let plot = ramachandran_plot(&[]);
        assert_eq!(plot.data.x_range(), (-180.0, 180.0));
        assert_eq!(plot.data.y_range(), (-180.0, 180.0));
        let names: Vec<&str> = plot.data.series.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, [SERIES_OTHER, SERIES_GLY, SERIES_PRO]);
        assert_eq!(plot.unplotted, 0);
    }

    #[test]
    fn residues_are_split_by_kind_and_keyed_by_the_first_atom() {
        let plot = ramachandran_plot(&[
            point("1", "ALA", Some(-60.0), Some(-45.0), &[1, 2, 3]),
            point("2", "GLY", Some(80.0), Some(10.0), &[4, 5, 6]),
            point("3", "pro", Some(-65.0), Some(150.0), &[7, 8, 9]),
            point("4", "TRP", Some(-120.0), Some(130.0), &[10, 11]),
        ]);
        let series = |n: &str| plot.data.series.iter().find(|s| s.name == n).expect(n);
        assert_eq!(series(SERIES_OTHER).points.len(), 2);
        let gly = series(SERIES_GLY).points[0];
        assert_eq!((gly.x, gly.y, gly.key), (80.0, 10.0, PlotKey(4)));
        assert_eq!(series(SERIES_PRO).points[0].key, PlotKey(7));
        assert_eq!(plot.unplotted, 0);
    }

    #[test]
    fn residues_without_both_angles_or_atoms_are_counted_not_plotted() {
        let plot = ramachandran_plot(&[
            point("1", "ALA", None, Some(10.0), &[1]),
            point("2", "ALA", Some(10.0), None, &[2]),
            point("3", "ALA", Some(10.0), Some(10.0), &[]),
            point("4", "ALA", Some(10.0), Some(10.0), &[4]),
        ]);
        assert_eq!(plot.data.point_count(), 1);
        assert_eq!(plot.unplotted, 3);
    }
}
