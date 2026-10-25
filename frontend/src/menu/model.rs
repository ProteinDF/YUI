//! メニューモデルの型と、メニューの場所ごとの受け口（`MenuRegistry`）。

use std::collections::BTreeMap;

use super::command::CommandInvocation;
use crate::context_menu::ContextMenuTarget;
use crate::representation_inspector::RepContextTarget;
use crate::structure_tree::TreeContextTarget;

/// メニューの場所（受け口の名前）
pub mod location {
    pub const FILE: &str = "menu/file";
    pub const VIEW: &str = "menu/view";
    pub const HELP: &str = "menu/help";
    pub const CONTEXT_VIEWPORT: &str = "context/viewport";
    pub const CONTEXT_STRUCTURE_TREE: &str = "context/structure_tree";
    pub const CONTEXT_REPRESENTATION: &str = "context/representation";
    pub const CONTEXT_FILE: &str = "context/file";
    /// 受け付ける場所の全部（プラグインが `add_menu_item` で指定した場所の検査に使う）
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub const ALL: [&str; 7] = [
        FILE,
        VIEW,
        HELP,
        CONTEXT_VIEWPORT,
        CONTEXT_STRUCTURE_TREE,
        CONTEXT_REPRESENTATION,
        CONTEXT_FILE,
    ];
}

/// 通常の項目（ボタン）の見た目の指定。メニューごとに文字の大きさ・余白が違うので、モデルに持たせる。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ItemStyle {
    pub size: u16,
    /// `[縦, 横]` の余白
    pub padding: [u16; 2],
    /// 横幅いっぱいに広げるか
    pub fill: bool,
    /// 削除など、注意を促す色にするか
    pub danger: bool,
}

impl ItemStyle {
    /// File / View / Help のドロップダウンの項目
    pub const DROPDOWN: Self = Self::new(12, [4, 12], true, false);
    /// 右クリックのメニューの、操作のボタン（ビューポートのメニュー。文字は 12）
    pub const ACTION_12: Self = Self::new(12, [3, 8], false, false);
    /// 右クリックのメニューの、操作のボタン（構造ツリー・Representation のメニュー。文字は 11）
    pub const ACTION_11: Self = Self::new(11, [3, 8], false, false);
    /// 右クリックのメニューの、横幅いっぱいのボタン
    pub const WIDE_11: Self = Self::new(11, [3, 8], true, false);
    /// 右クリックのメニューの、Show As / Color By の小さなボタン
    pub const CHIP: Self = Self::new(11, [2, 6], false, false);
    /// 右クリックのメニューの、注意を促すボタン（Delete）
    pub const DANGER_11: Self = Self::new(11, [3, 8], false, true);

    const fn new(size: u16, padding: [u16; 2], fill: bool, danger: bool) -> Self {
        Self {
            size,
            padding,
            fill,
            danger,
        }
    }
}

/// コマンドを呼び出す通常の項目
#[derive(Debug, Clone, PartialEq)]
pub struct CommandItem {
    pub label: String,
    pub command: CommandInvocation,
    /// 押せるか
    pub enabled: bool,
    /// チェック状態（`None` はチェックのない項目。`Some` なら、表示名の前に `✓ ` か空白を付ける）
    pub checked: Option<bool>,
    pub style: ItemStyle,
}

impl CommandItem {
    pub fn new(label: impl Into<String>, command: CommandInvocation, style: ItemStyle) -> Self {
        Self {
            label: label.into(),
            command,
            enabled: true,
            checked: None,
            style,
        }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }

    /// 画面に出す表示名（チェックのある項目は `✓ ` か空白 2 つを前に付ける）
    pub fn display_label(&self) -> String {
        match self.checked {
            Some(true) => format!("✓ {}", self.label),
            Some(false) => format!("  {}", self.label),
            None => self.label.clone(),
        }
    }
}

/// 項目のまとめ方
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GroupLayout {
    /// 横に並べる
    Row { spacing: u16 },
    /// 縦に並べる
    Column { spacing: u16 },
}

/// メニューの項目
#[derive(Debug, Clone, PartialEq)]
pub enum MenuItem {
    /// 通常の項目
    Command(CommandItem),
    /// 区切り線
    Separator,
    /// 見出し
    Heading(String),
    /// 右クリックのメニューの先頭の見出し（対象の説明つき）
    Header { title: String, subtitle: String },
    /// 項目のまとまり（横または縦に並べる）
    Group {
        layout: GroupLayout,
        items: Vec<MenuItem>,
    },
    /// 子メニュー（組み込みのメニューにはまだない。プラグインが使えるように用意している）。今は見出しと、その下に項目を並べる形で描く。
    #[allow(dead_code)] // プラグインの受け口として用意している（テストで使う）
    Submenu { label: String, items: Vec<MenuItem> },
}

impl MenuItem {
    /// 項目に含まれるコマンドの項目をすべて、並び順に集める
    #[allow(dead_code)] // 項目の検査に使う（テストで使う）
    pub fn command_items(&self) -> Vec<&CommandItem> {
        match self {
            MenuItem::Command(c) => vec![c],
            MenuItem::Group { items, .. } | MenuItem::Submenu { items, .. } => {
                items.iter().flat_map(|i| i.command_items()).collect()
            }
            _ => Vec::new(),
        }
    }
}

/// 右クリックのメニューを開いたときの対象（コマンドの引数の元になる）
#[derive(Debug, Clone, PartialEq)]
pub enum MenuTarget {
    /// 分子図の原子
    Atom(ContextMenuTarget),
    /// 構造ツリーのノード
    TreeNode(TreeContextTarget),
    /// Representation の行
    Representation(RepContextTarget),
    /// プロジェクトツリーのファイル（Open with）
    File(crate::file_open_dispatch::OpenWithTarget),
}

/// メニューを組み立てるときの、アプリの状態（項目の「押せるか」「チェック状態」を決める）
#[derive(Debug, Clone, Default)]
pub struct MenuContext {
    pub show_left: bool,
    pub show_right: bool,
    pub show_bottom: bool,
    pub editor_split: bool,
    pub camera_link: bool,
    /// 今の UI のテーマ
    pub theme: crate::ui_style::ThemeKind,
    /// 選択の強調の方法
    pub highlight: crate::selection::HighlightStyle,
    /// 原子が選ばれているか
    pub has_selection: bool,
    /// 軌跡を開いているか
    pub has_trajectory: bool,
    /// CUBE の等値面を開いているか
    pub has_cube: bool,
    /// 自動回転・首振りが動いているか、自動回転の速さ（度 / 秒）
    pub spinning: bool,
    pub rocking: bool,
    pub spin_speed: f32,
    /// 再生できるか（フレームが 2 つ以上）
    pub has_playback: bool,
    /// 再生中か
    pub playing: bool,
    /// 再生のループの方法
    pub loop_mode: crate::playback::LoopMode,
    pub has_structure: bool,
    /// 右クリックのメニューの対象
    pub target: Option<MenuTarget>,
}

type Producer = Box<dyn Fn(&MenuContext) -> Vec<MenuItem>>;

struct Contribution {
    order: i32,
    seq: u64,
    producer: Producer,
}

/// メニューの場所ごとの、項目の受け口。
///
/// 組み込みのメニューも、将来のプラグインも、同じ `contribute` で項目を足す。
/// 場所の中の並びは、`order` の小さい順で、同じなら足した順である。
/// 組み込みは 100 刻み（100, 200, 300 ...）で足すので、プラグインは、その間（例: 150）にも、末尾（例: 1000）にも足せる。
#[derive(Default)]
pub struct MenuRegistry {
    by_location: BTreeMap<String, Vec<Contribution>>,
    next_seq: u64,
}

impl MenuRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 状態から項目を求める関数を足す（押せるか・チェック状態が状態で変わる項目用）
    pub fn contribute(
        &mut self,
        location: &str,
        order: i32,
        producer: impl Fn(&MenuContext) -> Vec<MenuItem> + 'static,
    ) {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.by_location
            .entry(location.to_string())
            .or_default()
            .push(Contribution {
                order,
                seq,
                producer: Box::new(producer),
            });
    }

    /// 決まった項目を足す
    pub fn contribute_item(&mut self, location: &str, order: i32, item: MenuItem) {
        self.contribute(location, order, move |_| vec![item.clone()]);
    }

    /// 指定した場所のメニューの項目を、並び順に組み立てる（足されていない場所は空）
    pub fn build(&self, location: &str, ctx: &MenuContext) -> Vec<MenuItem> {
        let Some(contributions) = self.by_location.get(location) else {
            return Vec::new();
        };
        let mut sorted: Vec<&Contribution> = contributions.iter().collect();
        sorted.sort_by_key(|c| (c.order, c.seq));
        sorted.into_iter().flat_map(|c| (c.producer)(ctx)).collect()
    }
}
