use proteindf_bridge::Pdb;
use yui_core::atom_group::{
    AtomGroup, AtomGroupExt, iter_atoms, iter_chains, iter_models, iter_residues,
};

#[test]
fn test_real_pdb_1hls_depth_convention_and_flattening() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/1hls.pdb");
    let pdb = Pdb::from_file(path, None).expect("failed to load real PDB file");
    let mut bridge_group = pdb
        .get_atomgroup(None, None)
        .expect("failed to build AtomGroup from PDB");
    bridge_group.setup().expect("bond resolution failed"); // CCDテンプレート + 共有結合半径ヒューリスティック
    let group: AtomGroup = bridge_group.into();

    // 1. 深さ規約の検証: iter_models→iter_chains→iter_residues→iter_atoms の走査で
    //    見つかる原子の総数が get_number_of_all_atoms() と一致すること
    //    （途中に規約外の深さの原子が紛れ込んでいないことの確認）
    let mut traversed_count = 0;
    for (_m, model) in iter_models(&group) {
        for (_c, chain) in iter_chains(model) {
            for (_r, residue) in iter_residues(chain) {
                traversed_count += iter_atoms(residue).count();
            }
        }
    }
    println!(
        "=== Real PDB 1HLS: atom_count = {}, traversed = {} ===",
        group.atom_count(),
        traversed_count
    );
    assert_eq!(traversed_count, group.atom_count());
    assert!(traversed_count > 0, "real PDB file should contain atoms");

    // 2. 平坦化キャッシュへの変換が成功し、原子数が一致すること
    let flat = group.to_flat_molecule();
    println!(
        "=== Real PDB 1HLS: flat.atoms = {}, flat.bonds = {} ===",
        flat.atoms.len(),
        flat.bonds.len()
    );
    assert_eq!(flat.atoms.len(), traversed_count);

    // 3. 結合が生成されていること（実データでの Bond::setup 系パイプラインの疎通確認）
    assert!(
        !flat.bonds.is_empty(),
        "real PDB structure should have detected bonds"
    );
}
