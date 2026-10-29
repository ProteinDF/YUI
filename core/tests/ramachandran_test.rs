use yui_core::atom_group::{AtomGroupExt, iter_flat_atoms};
use yui_core::ramachandran::ramachandran_points;
use yui_core::structure_loader::parse_structure_bytes;

fn load_1hls() -> yui_core::atom_group::AtomGroup {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/1hls.pdb");
    let bytes = std::fs::read(path).expect("read 1hls.pdb");
    parse_structure_bytes(&bytes, "1hls.pdb")
        .expect("parse 1hls.pdb")
        .0
}

#[test]
fn real_structure_gives_plausible_angles_and_valid_atom_ids() {
    let root = load_1hls();
    let points = ramachandran_points(&root, None);
    assert!(points.len() > 40, "{} residues", points.len());

    // the angles are in range, and the ends of the chains are open
    let angles = points.iter().filter_map(|p| p.phi.zip(p.psi));
    let mut total = 0;
    let mut helical = 0;
    for (phi, psi) in angles {
        assert!((-180.0..=180.0).contains(&phi) && (-180.0..=180.0).contains(&psi));
        total += 1;
        if (-100.0..=-30.0).contains(&phi) && (-80.0..=-5.0).contains(&psi) {
            helical += 1;
        }
    }
    assert!(total > 40);
    // 1HLS is insulin (chains A and B, helices in both): the right-handed helix region is crowded
    assert!(helical * 3 > total, "{helical} of {total} are helical");
    let chains: std::collections::BTreeSet<_> =
        points.iter().map(|p| p.chain_id.as_str()).collect();
    assert!(chains.len() >= 2, "{chains:?}");
    assert!(points.iter().any(|p| p.phi.is_none()));
    assert!(points.iter().any(|p| p.psi.is_none()));

    // every point's atoms exist, belong to its residue, and no atom is in two points
    let flat: Vec<_> = iter_flat_atoms(&root).collect();
    let mut seen = std::collections::HashSet::new();
    for p in &points {
        assert!(!p.atom_ids.is_empty(), "{}", p.residue_key);
        for id in &p.atom_ids {
            assert!(seen.insert(*id), "atom {id} is in two points");
            let atom = &flat[*id as usize - 1];
            assert_eq!(atom.id, *id);
            assert_eq!(atom.residue_key, p.residue_key);
            assert_eq!(atom.chain_key, p.chain_id);
        }
    }
    assert_eq!(root.atom_count(), flat.len());
}
