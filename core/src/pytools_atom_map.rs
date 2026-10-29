//! Map the atoms of a pytools plot (ProteinDF order) onto the atoms of a displayed structure.
//!
//! `pdf-plot/1` identifies an atom only by its 1-based index in the ProteinDF input, so the only
//! evidence that a displayed structure is the calculated one is that it has the same number of
//! atoms and the same element at every position. The map is built only when both hold; otherwise
//! it is refused, because selecting the wrong atoms would be worse than selecting none. (The
//! displayed structure can differ from the calculated one: added hydrogens, caps, dummy atoms.)

use std::collections::BTreeMap;
use std::fmt;

use crate::atom_group::{iter_flat_atoms, iter_models};
use crate::pytools_plot::PytoolsAtom;
use proteindf_bridge::AtomGroup;

/// How many mismatching atoms [`fmt::Display`] names before saying "and N more".
const SHOWN_MISMATCHES: usize = 3;

/// One position where the plot and the structure disagree on the element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementMismatch {
    /// ProteinDF atom index (1-based).
    pub index: u32,
    pub plot_element: String,
    pub structure_element: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AtomMapError {
    /// The structure has no such model (or no model at all).
    NoModel,
    /// The atom indices of the plot are not exactly `1..=n`.
    IndicesNotContiguous,
    CountMismatch {
        plot: usize,
        structure: usize,
    },
    ElementMismatch(Vec<ElementMismatch>),
}

impl fmt::Display for AtomMapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoModel => write!(f, "the structure has no model to link with"),
            Self::IndicesNotContiguous => {
                write!(f, "the atom indices of the plot are not 1..n without gaps")
            }
            Self::CountMismatch { plot, structure } => write!(
                f,
                "the plot has {plot} atoms but the structure has {structure}"
            ),
            Self::ElementMismatch(list) => {
                write!(f, "{} atom(s) differ in element: ", list.len())?;
                for (i, m) in list.iter().take(SHOWN_MISMATCHES).enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(
                        f,
                        "#{} is {} in the plot and {} in the structure",
                        m.index, m.plot_element, m.structure_element
                    )?;
                }
                if list.len() > SHOWN_MISMATCHES {
                    write!(f, ", and {} more", list.len() - SHOWN_MISMATCHES)?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for AtomMapError {}

/// One-to-one map between ProteinDF atom indices and YUI atom ids.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AtomMap {
    to_yui: BTreeMap<u32, u32>,
    to_plot: BTreeMap<u32, u32>,
}

impl AtomMap {
    /// YUI atom id of a ProteinDF atom index.
    pub fn yui_id(&self, plot_index: u32) -> Option<u32> {
        self.to_yui.get(&plot_index).copied()
    }

    /// ProteinDF atom index of a YUI atom id (`None` for atoms outside the mapped model).
    pub fn plot_index(&self, yui_id: u32) -> Option<u32> {
        self.to_plot.get(&yui_id).copied()
    }

    pub fn len(&self) -> usize {
        self.to_yui.len()
    }

    pub fn is_empty(&self) -> bool {
        self.to_yui.is_empty()
    }
}

/// Map `atoms` onto the atoms of one model, in order: `model_key`, or the first model when `None`.
///
/// The i-th atom of the model (in the order of [`iter_flat_atoms`]) is ProteinDF atom `i`.
pub fn map_pytools_atoms(
    root: &AtomGroup,
    model_key: Option<&str>,
    atoms: &[PytoolsAtom],
) -> Result<AtomMap, AtomMapError> {
    let (model_key, _) = iter_models(root)
        .find(|(key, _)| model_key.is_none_or(|wanted| wanted == *key))
        .ok_or(AtomMapError::NoModel)?;

    let mut plot_atoms: Vec<&PytoolsAtom> = atoms.iter().collect();
    plot_atoms.sort_by_key(|a| a.index);
    if plot_atoms
        .iter()
        .enumerate()
        .any(|(i, a)| a.index as usize != i + 1)
    {
        return Err(AtomMapError::IndicesNotContiguous);
    }

    let structure: Vec<_> = iter_flat_atoms(root)
        .filter(|a| a.model_key == model_key)
        .collect();
    if structure.len() != plot_atoms.len() {
        return Err(AtomMapError::CountMismatch {
            plot: plot_atoms.len(),
            structure: structure.len(),
        });
    }

    let mut mismatches = Vec::new();
    let mut map = AtomMap::default();
    for (plot_atom, flat) in plot_atoms.iter().zip(&structure) {
        // An element the table does not know is shown as a dummy atom, like elsewhere in YUI.
        let element = flat.atom.symbol().unwrap_or("X");
        if !plot_atom.element.eq_ignore_ascii_case(element) {
            mismatches.push(ElementMismatch {
                index: plot_atom.index,
                plot_element: plot_atom.element.clone(),
                structure_element: element.to_string(),
            });
        }
        map.to_yui.insert(plot_atom.index, flat.id);
        map.to_plot.insert(flat.id, plot_atom.index);
    }
    if mismatches.is_empty() {
        Ok(map)
    } else {
        Err(AtomMapError::ElementMismatch(mismatches))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structure_loader::parse_structure_bytes;

    const WATER: &str = "\
HETATM    1  O   HOH A   1       0.000   0.000   0.000  1.00  0.00           O
HETATM    2  H1  HOH A   1       0.960   0.000   0.000  1.00  0.00           H
HETATM    3  H2  HOH A   1      -0.240   0.930   0.000  1.00  0.00           H
END
";

    fn water() -> AtomGroup {
        parse_structure_bytes(WATER.as_bytes(), "water.pdb")
            .unwrap()
            .0
            .0
    }

    fn atom(index: u32, element: &str) -> PytoolsAtom {
        PytoolsAtom {
            index,
            element: element.into(),
            label: String::new(),
        }
    }

    fn h2o() -> Vec<PytoolsAtom> {
        vec![atom(1, "O"), atom(2, "H"), atom(3, "H")]
    }

    #[test]
    fn maps_by_order_in_both_directions() {
        let map = map_pytools_atoms(&water(), None, &h2o()).unwrap();
        assert_eq!(map.len(), 3);
        let ids: Vec<u32> = (1..=3).map(|i| map.yui_id(i).unwrap()).collect();
        let flat: Vec<u32> = iter_flat_atoms(&water()).map(|a| a.id).collect();
        assert_eq!(ids, flat);
        for i in 1..=3 {
            assert_eq!(map.plot_index(map.yui_id(i).unwrap()), Some(i));
        }
        assert_eq!(map.yui_id(4), None);
        assert_eq!(map.plot_index(99), None);
    }

    #[test]
    fn element_case_is_ignored_and_input_order_does_not_matter() {
        let atoms = vec![atom(3, "h"), atom(1, "o"), atom(2, "H")];
        assert!(map_pytools_atoms(&water(), None, &atoms).is_ok());
    }

    #[test]
    fn refuses_a_different_atom_count() {
        // For example the structure got hydrogens (or lost a dummy atom) after the calculation.
        let two = &h2o()[..2];
        assert_eq!(
            map_pytools_atoms(&water(), None, two),
            Err(AtomMapError::CountMismatch {
                plot: 2,
                structure: 3
            })
        );
    }

    #[test]
    fn names_the_atoms_whose_element_differs() {
        let atoms = vec![atom(1, "O"), atom(2, "C"), atom(3, "N")];
        let Err(AtomMapError::ElementMismatch(list)) = map_pytools_atoms(&water(), None, &atoms)
        else {
            panic!("expected an element mismatch");
        };
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].index, 2);
        assert_eq!(list[0].plot_element, "C");
        assert_eq!(list[0].structure_element, "H");
        let text = AtomMapError::ElementMismatch(list).to_string();
        assert!(
            text.contains("#2 is C in the plot and H in the structure"),
            "{text}"
        );
    }

    #[test]
    fn a_swapped_pair_is_not_mapped() {
        // Same count and same element set, but a different order: must not link.
        let atoms = vec![atom(1, "H"), atom(2, "O"), atom(3, "H")];
        assert!(matches!(
            map_pytools_atoms(&water(), None, &atoms),
            Err(AtomMapError::ElementMismatch(_))
        ));
    }

    #[test]
    fn refuses_gaps_and_duplicates_in_the_indices() {
        let gap = vec![atom(1, "O"), atom(2, "H"), atom(4, "H")];
        assert_eq!(
            map_pytools_atoms(&water(), None, &gap),
            Err(AtomMapError::IndicesNotContiguous)
        );
        let from_zero = vec![atom(0, "O"), atom(1, "H"), atom(2, "H")];
        assert_eq!(
            map_pytools_atoms(&water(), None, &from_zero),
            Err(AtomMapError::IndicesNotContiguous)
        );
    }

    #[test]
    fn unknown_model_and_empty_structure() {
        assert_eq!(
            map_pytools_atoms(&water(), Some("nope"), &h2o()),
            Err(AtomMapError::NoModel)
        );
        assert_eq!(
            map_pytools_atoms(&AtomGroup::default(), None, &h2o()),
            Err(AtomMapError::NoModel)
        );
    }

    #[test]
    fn long_mismatch_lists_are_shortened() {
        let list: Vec<ElementMismatch> = (1..=5)
            .map(|i| ElementMismatch {
                index: i,
                plot_element: "C".into(),
                structure_element: "H".into(),
            })
            .collect();
        let text = AtomMapError::ElementMismatch(list).to_string();
        assert!(text.starts_with("5 atom(s)"), "{text}");
        assert!(text.ends_with("and 2 more"), "{text}");
    }
}
