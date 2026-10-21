//! 選択の状態と操作（architecture.md 8.9節）。
//!
//! 「今どの原子が選ばれているか」を、Representation とは別の状態として持つ。選択は利用者ごとのもので、
//! ワークスペース（`AppState`、YAML）にも協調編集にも入れない。残したい選択は名前付き選択に保存する。
//! ここには、画面や GPU を使わずに試せる処理（クリックの判定、修飾キーごとの選択の変化、残基の原子）を置く。

use std::collections::BTreeSet;

use yui_core::state::Atom;

/// マウスを押してから離すまでに動いてよい距離（画面上のピクセル）。これ以下ならクリック、超えればドラッグ（回転）。
pub const CLICK_MAX_DISTANCE: f32 = 4.0;
/// 2 回のクリックをダブルクリックとみなす間隔（ミリ秒）
pub const DOUBLE_CLICK_MS: u128 = 400;
/// 2 回のクリックをダブルクリックとみなす位置の差（ピクセル）
pub const DOUBLE_CLICK_DISTANCE: f32 = 6.0;

/// 選択の強調の方法（利用者ごとの設定）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HighlightStyle {
    /// にじむ光（既定）
    #[default]
    Glow,
    /// 輪郭の輪
    Halo,
    /// 強調しない（選択の状態は保つ）
    Off,
}

impl HighlightStyle {
    /// 設定ファイルに書く名前
    pub fn as_str(self) -> &'static str {
        match self {
            HighlightStyle::Glow => "glow",
            HighlightStyle::Halo => "halo",
            HighlightStyle::Off => "off",
        }
    }

    /// 設定ファイルの名前から求める（大文字・小文字と前後の空白は区別しない）
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "glow" => Some(HighlightStyle::Glow),
            "halo" => Some(HighlightStyle::Halo),
            "off" => Some(HighlightStyle::Off),
            _ => None,
        }
    }

    /// 画面に出す名前
    pub fn label(self) -> &'static str {
        match self {
            HighlightStyle::Glow => "Glow",
            HighlightStyle::Halo => "Halo",
            HighlightStyle::Off => "Off",
        }
    }
}

/// 分子図の左クリックで当たった原子
#[derive(Debug, Clone, PartialEq)]
pub struct ClickedAtom {
    pub atom_id: u32,
    pub chain_id: String,
    pub residue_index: i32,
}

/// 分子図の左クリック（ほとんど動かさずに離した）。アプリが選択を変える。
#[derive(Debug, Clone, PartialEq)]
pub struct ViewportClick {
    /// 当たった原子（原子のないところなら `None`）
    pub atom: Option<ClickedAtom>,
    pub shift: bool,
    /// Ctrl（macOS は Cmd）
    pub command: bool,
    /// ダブルクリック（残基単位で選ぶ）
    pub double: bool,
}

/// 選択の変え方
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectOp {
    /// 今の選択を捨てて、クリックしたものだけにする
    Replace,
    /// 今の選択に足す
    Add,
    /// 選ばれていれば外し、選ばれていなければ足す
    Toggle,
}

/// 修飾キーから選択の変え方を決める。Ctrl（macOS は Cmd）が切り替え、Shift が追加。両方なら切り替え。
pub fn op_from_modifiers(shift: bool, command: bool) -> SelectOp {
    if command {
        SelectOp::Toggle
    } else if shift {
        SelectOp::Add
    } else {
        SelectOp::Replace
    }
}

/// 選択を変える。`hit` はクリックしたものの原子（原子のないところなら空）。
///
/// - 置き換え: `hit` だけにする。空なら、選択を外す。
/// - 追加: `hit` を足す。空なら何もしない（誤って外さない）。
/// - 切り替え: `hit` のすべてが選ばれていれば外し、そうでなければ足す。空なら何もしない。
pub fn apply(selection: &mut BTreeSet<u32>, op: SelectOp, hit: &[u32]) {
    match op {
        SelectOp::Replace => {
            selection.clear();
            selection.extend(hit.iter().copied());
        }
        SelectOp::Add => selection.extend(hit.iter().copied()),
        SelectOp::Toggle => {
            if hit.is_empty() {
                return;
            }
            if hit.iter().all(|id| selection.contains(id)) {
                for id in hit {
                    selection.remove(id);
                }
            } else {
                selection.extend(hit.iter().copied());
            }
        }
    }
}

/// 押した位置と離した位置から、クリック（ほとんど動いていない）かを判定する。
pub fn is_click(down: [f32; 2], up: [f32; 2]) -> bool {
    let (dx, dy) = (up[0] - down[0], up[1] - down[1]);
    (dx * dx + dy * dy).sqrt() <= CLICK_MAX_DISTANCE
}

/// 前のクリックからの経過時間と位置の差から、ダブルクリックかを判定する。
pub fn is_double_click(elapsed_ms: u128, previous: [f32; 2], current: [f32; 2]) -> bool {
    let (dx, dy) = (current[0] - previous[0], current[1] - previous[1]);
    elapsed_ms <= DOUBLE_CLICK_MS && (dx * dx + dy * dy).sqrt() <= DOUBLE_CLICK_DISTANCE
}

/// ある残基（鎖と残基番号）の原子の ID。鎖の名前が空なら、鎖は区別しない。
pub fn residue_atom_ids(atoms: &[Atom], chain_id: &str, residue_index: i32) -> Vec<u32> {
    atoms
        .iter()
        .filter(|a| {
            a.residue_index == residue_index && (chain_id.is_empty() || a.chain_id == chain_id)
        })
        .map(|a| a.id)
        .collect()
}

/// 選ばれている原子の座標（Zoom to Selection で、合わせる対象）
pub fn positions_of(atoms: &[Atom], selection: &BTreeSet<u32>) -> Vec<[f32; 3]> {
    atoms
        .iter()
        .filter(|a| selection.contains(&a.id))
        .map(|a| a.position)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atom(id: u32, chain: &str, residue: i32) -> Atom {
        Atom {
            id,
            element: "C".into(),
            position: [0.0; 3],
            radius: 1.7,
            color: [1.0; 4],
            residue_name: "ALA".into(),
            residue_index: residue,
            chain_id: chain.into(),
        }
    }

    fn set(ids: &[u32]) -> BTreeSet<u32> {
        ids.iter().copied().collect()
    }

    #[test]
    fn click_versus_drag_threshold() {
        assert!(is_click([10.0, 10.0], [10.0, 10.0]));
        assert!(is_click([10.0, 10.0], [13.0, 12.0])); // 距離 3.6
        assert!(is_click([0.0, 0.0], [CLICK_MAX_DISTANCE, 0.0])); // ちょうどしきい値
        assert!(!is_click([0.0, 0.0], [CLICK_MAX_DISTANCE + 0.1, 0.0]));
        assert!(!is_click([0.0, 0.0], [30.0, 40.0]));
    }

    #[test]
    fn double_click_needs_time_and_place() {
        assert!(is_double_click(200, [5.0, 5.0], [6.0, 6.0]));
        assert!(is_double_click(DOUBLE_CLICK_MS, [5.0, 5.0], [5.0, 5.0]));
        assert!(!is_double_click(
            DOUBLE_CLICK_MS + 1,
            [5.0, 5.0],
            [5.0, 5.0]
        ));
        assert!(!is_double_click(100, [0.0, 0.0], [50.0, 0.0]));
    }

    #[test]
    fn modifiers_choose_the_operation() {
        assert_eq!(op_from_modifiers(false, false), SelectOp::Replace);
        assert_eq!(op_from_modifiers(true, false), SelectOp::Add);
        assert_eq!(op_from_modifiers(false, true), SelectOp::Toggle);
        assert_eq!(op_from_modifiers(true, true), SelectOp::Toggle);
    }

    #[test]
    fn replace_add_and_toggle() {
        let mut s = set(&[1, 2]);
        apply(&mut s, SelectOp::Replace, &[5]);
        assert_eq!(s, set(&[5]));

        apply(&mut s, SelectOp::Add, &[7, 5]);
        assert_eq!(s, set(&[5, 7]));

        apply(&mut s, SelectOp::Toggle, &[7]);
        assert_eq!(s, set(&[5]));
        apply(&mut s, SelectOp::Toggle, &[7]);
        assert_eq!(s, set(&[5, 7]));

        // 残基など複数の原子: すべて選ばれていれば外し、一部だけなら足す
        let mut s = set(&[1, 2, 3]);
        apply(&mut s, SelectOp::Toggle, &[1, 2]);
        assert_eq!(s, set(&[3]));
        apply(&mut s, SelectOp::Toggle, &[3, 4]);
        assert_eq!(s, set(&[3, 4]));
    }

    #[test]
    fn clicking_empty_space_clears_only_with_replace() {
        let mut s = set(&[1, 2]);
        apply(&mut s, SelectOp::Add, &[]);
        apply(&mut s, SelectOp::Toggle, &[]);
        assert_eq!(s, set(&[1, 2]));
        apply(&mut s, SelectOp::Replace, &[]);
        assert!(s.is_empty());
    }

    #[test]
    fn residue_selection_takes_all_atoms_of_that_residue() {
        let atoms = vec![
            atom(1, "A", 15),
            atom(2, "A", 15),
            atom(3, "A", 16),
            atom(4, "B", 15),
        ];
        assert_eq!(residue_atom_ids(&atoms, "A", 15), vec![1, 2]);
        assert_eq!(residue_atom_ids(&atoms, "B", 15), vec![4]);
        assert_eq!(residue_atom_ids(&atoms, "A", 99), Vec::<u32>::new());
        // 鎖の名前が空なら、鎖は区別しない
        assert_eq!(residue_atom_ids(&atoms, "", 15), vec![1, 2, 4]);
    }

    #[test]
    fn highlight_style_names_round_trip() {
        for style in [
            HighlightStyle::Glow,
            HighlightStyle::Halo,
            HighlightStyle::Off,
        ] {
            assert_eq!(HighlightStyle::parse(style.as_str()), Some(style));
        }
        assert_eq!(HighlightStyle::parse(" Halo "), Some(HighlightStyle::Halo));
        assert_eq!(HighlightStyle::parse("blur"), None);
        assert_eq!(HighlightStyle::default(), HighlightStyle::Glow);
    }

    #[test]
    fn test_positions_of_returns_only_the_selected_atoms() {
        let mk = |id: u32, x: f32| Atom {
            position: [x, 0.0, 0.0],
            ..atom(id, "A", 1)
        };
        let atoms = vec![mk(1, 1.0), mk(2, 2.0), mk(3, 3.0)];
        let sel: BTreeSet<u32> = [1, 3, 99].into_iter().collect();
        assert_eq!(
            positions_of(&atoms, &sel),
            vec![[1.0, 0.0, 0.0], [3.0, 0.0, 0.0]]
        );
        assert!(positions_of(&atoms, &BTreeSet::new()).is_empty());
    }
}
