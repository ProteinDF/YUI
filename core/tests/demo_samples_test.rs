// SPDX-FileCopyrightText: The Yui development team
// SPDX-License-Identifier: GPL-3.0-or-later

//! `examples/structures/` の展示用タンパク質が、すべて読み込めることを確かめる。

use yui_core::atom_group::AtomGroupExt;
use yui_core::structure_loader::parse_structure_bytes;

/// (ファイル名, 読み込まれる原子数)
const SAMPLES: [(&str, usize); 5] = [
    ("1ema.pdb", 1866),
    ("2hhb.pdb", 4779),
    ("1zni.pdb", 915),
    ("1bl8.pdb", 2824),
    ("1ubq.pdb", 660),
];

#[test]
fn demo_proteins_load() {
    for (name, expected) in SAMPLES {
        let path = format!(
            "{}/../examples/structures/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        let (group, _, _) =
            parse_structure_bytes(&bytes, name).unwrap_or_else(|e| panic!("{name}: {e}"));
        let n = group.atom_count();
        println!("{name}: {n} atoms");
        assert_eq!(n, expected, "{name}");
    }
}
