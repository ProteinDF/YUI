//! 汎用ペイン/ビューモデル (Phase 6d-iii)
//!
//! architecture.md 8.1節および 8.4節に準拠。
//! 物理領域（PaneKind: Left/Center/Right/Bottom）からコンテンツを切り離し、
//! 各領域で任意の ViewId（3D Viewport, Tree, Inspector, Log, Console, Terminal, AI Chat）を
//! タブリストとしてホストできるように一般化する。

use iced::widget::{button, column, container, mouse_area, row, scrollable, text};
use iced::{Element, Fill};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use yui_core::state::AppState;

use crate::PaneKind;
use crate::app::GroupId;
use crate::text_editor_pane::DocumentId;
use iced::widget::pane_grid::Axis;

/// 登録されたビュー種別の識別子 (Phase 6d-iii)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewId {
    /// 3D分子ビューポート
    Viewport,
    /// 2つ目の3D分子ビューポート。中央の左右分割がオンのとき、右側のタブに並ぶ（分割をオフにすると消える）。
    Viewport2,
    /// 構造/ファイルツリー（Leftペイン・Phase 6d-ivで本格実装予定）
    StructureTree,
    /// 表現一覧・インスペクタ（Rightペイン・Phase 6d-vで本格実装予定）
    RepresentationInspector,
    /// ログストリーム監視ビューワー（Phase 6a-i）
    LogViewer,
    /// Python対話コンソール（Phase 3 / 将来WASMコンソール）
    PythonConsole,
    /// PTYシェルターミナル（Phase 6a-ii）
    Terminal,
    /// AIチャットアシスタント（Phase 6b）
    AiChat,
    /// 名前付き選択グループ一覧（Leftペイン第2タブ）
    NamedSelections,
    /// 開いているテキストファイル（ドキュメント）1つ分のタブ。中央ペイン、または中央の右側に並ぶ。
    /// ワークスペースの YAML には保存しない（利用者ごとのもの）。
    Document(DocumentId),
    /// 2026.10.2 以前の、1 つだけのテキストエディタのタブ。古いワークスペースを読めるように、
    /// 読み込み専用で残している。読み込み時に取り除く（`normalize_pane_views`）。
    #[serde(rename = "text_editor")]
    LegacyTextEditor,
    /// プロジェクトディレクトリツリー（Leftペイン・Phase 6f-iii）
    ProjectTree,
}

impl ViewId {
    /// タブやヘッダーに表示するユーザー向けラベル
    pub fn label(&self) -> &'static str {
        match self {
            ViewId::Viewport => "3D Viewport",
            ViewId::Viewport2 => "3D Viewport 2",
            ViewId::StructureTree => "Explorer / Playbook",
            ViewId::NamedSelections => "Named Selections",
            ViewId::RepresentationInspector => "Inspector",
            ViewId::LogViewer => "Log Viewer",
            ViewId::PythonConsole => "Python Console",
            ViewId::Terminal => "Terminal",
            ViewId::AiChat => "AI Chat",
            ViewId::Document(_) => "Document",
            ViewId::LegacyTextEditor => "Text Editor",
            ViewId::ProjectTree => "Project",
        }
    }
}

/// 1つのペイン（物理領域）内にドッキングされているビューの状態
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneViewState {
    /// 開いているビュー一覧（タブ順）
    pub views: Vec<ViewId>,
    /// 現在アクティブなビューのインデックス
    pub active_index: usize,
}

impl PaneViewState {
    pub fn new(views: Vec<ViewId>, active_index: usize) -> Self {
        let safe_index = if views.is_empty() {
            0
        } else {
            active_index.min(views.len() - 1)
        };
        Self {
            views,
            active_index: safe_index,
        }
    }

    /// 現在アクティブな ViewId を取得
    pub fn active_view(&self) -> Option<ViewId> {
        self.views.get(self.active_index).copied()
    }

    /// 指定された ViewId をアクティブに切り替える
    pub fn select_view(&mut self, view: ViewId) -> bool {
        if let Some(pos) = self.views.iter().position(|v| *v == view) {
            self.active_index = pos;
            true
        } else {
            false
        }
    }

    /// 指定された ViewId をタブから外す。アクティブなビューは、できるだけ同じものを保つ。
    /// 外したときは `true` を返す。
    pub fn remove_view(&mut self, view: ViewId) -> bool {
        let Some(pos) = self.views.iter().position(|v| *v == view) else {
            return false;
        };
        let active = self.active_view();
        self.views.remove(pos);
        self.active_index = match active {
            Some(a) if a != view => self.views.iter().position(|v| *v == a).unwrap_or(0),
            _ => pos.min(self.views.len().saturating_sub(1)),
        };
        true
    }

    /// `view` のタブを、`target` のタブがある位置へ移す（ドラッグでの並べ替え）。
    /// 選択中のビューは変えない。移したときは `true` を返す。
    pub fn move_view_to(&mut self, view: ViewId, target: ViewId) -> bool {
        let (Some(from), Some(to)) = (
            self.views.iter().position(|v| *v == view),
            self.views.iter().position(|v| *v == target),
        ) else {
            return false;
        };
        if from == to {
            return false;
        }
        let active = self.active_view();
        let moved = self.views.remove(from);
        self.views.insert(to, moved);
        if let Some(a) = active {
            self.select_view(a);
        }
        true
    }

    /// インデックス指定でアクティブビューを切り替える
    #[allow(dead_code)]
    pub fn select_index(&mut self, index: usize) -> bool {
        if index < self.views.len() {
            self.active_index = index;
            true
        } else {
            false
        }
    }
}

/// エディタのグループの並び（中央の領域の分割の木）。葉がグループ。
///
/// 任意の向き（左右・上下）と数に分割できる。タブをグループの端へ落とすと、その葉を分割する（`split_leaf`）。
#[derive(Debug, Clone, PartialEq)]
pub enum GroupNode {
    Group(GroupId),
    Split {
        axis: Axis,
        ratio: f32,
        a: Box<GroupNode>,
        b: Box<GroupNode>,
    },
}

/// タブを落とす場所: グループの本体の中央（そのグループへ移す）か、上下左右の端（その向きに分割する）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropZone {
    Center,
    Left,
    Right,
    Top,
    Bottom,
}

impl GroupNode {
    /// 葉 `target` を、`target` と新しいグループ `new` の分割に置き換える。`edge` の側に `new` を置く。
    /// 葉が見つかれば `true`。
    pub fn split_leaf(&mut self, target: GroupId, new: GroupId, edge: DropZone) -> bool {
        match self {
            GroupNode::Group(id) if *id == target => {
                let (axis, new_first) = match edge {
                    DropZone::Left => (Axis::Vertical, true),
                    DropZone::Right | DropZone::Center => (Axis::Vertical, false),
                    DropZone::Top => (Axis::Horizontal, true),
                    DropZone::Bottom => (Axis::Horizontal, false),
                };
                let (a, b) = if new_first {
                    (GroupNode::Group(new), GroupNode::Group(target))
                } else {
                    (GroupNode::Group(target), GroupNode::Group(new))
                };
                *self = GroupNode::Split {
                    axis,
                    ratio: 0.5,
                    a: Box::new(a),
                    b: Box::new(b),
                };
                true
            }
            GroupNode::Group(_) => false,
            GroupNode::Split { a, b, .. } => {
                a.split_leaf(target, new, edge) || b.split_leaf(target, new, edge)
            }
        }
    }

    /// 葉 `target` を取り除き、その親の分割を、もう一方の子で置き換える。
    /// 木が葉 1 つだけなら取り除かない。取り除いたら `true`。
    pub fn remove_leaf(&mut self, target: GroupId) -> bool {
        let GroupNode::Split { a, b, .. } = self else {
            return false;
        };
        if **a == GroupNode::Group(target) {
            *self = std::mem::replace(&mut **b, GroupNode::Group(target));
            return true;
        }
        if **b == GroupNode::Group(target) {
            *self = std::mem::replace(&mut **a, GroupNode::Group(target));
            return true;
        }
        a.remove_leaf(target) || b.remove_leaf(target)
    }

    /// 分割の比率を、`ratio_of`（分割の両側のグループの組 → 比率）から取り込む。見つからない分割は、そのまま。
    pub fn sync_ratios(&mut self, ratio_of: &impl Fn(&[PaneKind], &[PaneKind]) -> Option<f32>) {
        if let GroupNode::Split { ratio, a, b, .. } = self {
            let kinds = |n: &GroupNode| -> Vec<PaneKind> {
                let mut v: Vec<PaneKind> = n.groups().into_iter().map(PaneKind::Group).collect();
                v.sort();
                v
            };
            if let Some(r) = ratio_of(&kinds(a), &kinds(b)) {
                *ratio = r;
            }
            a.sync_ratios(ratio_of);
            b.sync_ratios(ratio_of);
        }
    }

    /// 葉のグループを、左（上）から順に並べる
    pub fn groups(&self) -> Vec<GroupId> {
        match self {
            GroupNode::Group(id) => vec![*id],
            GroupNode::Split { a, b, .. } => {
                let mut out = a.groups();
                out.extend(b.groups());
                out
            }
        }
    }
}

/// ペインの構成。固定のドック（Left / Right / Bottom）と、エディタのグループの、タブの並び。
/// グループの配置は、木（`editor`）で表す。木の葉と、`panes` のグループのキーは、いつも一致させる。
///
/// 保存の YAML は、以前と同じ `pane_views` のマップ（`left`、`center`、`right`、`bottom`、`center_right`）のまま。
/// 読み込むときに、マップのグループから木を作る。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    from = "BTreeMap<PaneKind, PaneViewState>",
    into = "BTreeMap<PaneKind, PaneViewState>"
)]
pub struct PaneLayout {
    panes: BTreeMap<PaneKind, PaneViewState>,
    editor: GroupNode,
}

impl From<BTreeMap<PaneKind, PaneViewState>> for PaneLayout {
    fn from(panes: BTreeMap<PaneKind, PaneViewState>) -> Self {
        let mut layout = PaneLayout {
            panes,
            editor: GroupNode::Group(GroupId(0)),
        };
        layout.rebuild_editor_tree();
        layout
    }
}

impl From<PaneLayout> for BTreeMap<PaneKind, PaneViewState> {
    fn from(layout: PaneLayout) -> Self {
        layout.panes
    }
}

impl std::ops::Index<&PaneKind> for PaneLayout {
    type Output = PaneViewState;
    fn index(&self, kind: &PaneKind) -> &PaneViewState {
        &self.panes[kind]
    }
}

impl PaneLayout {
    pub fn get(&self, kind: &PaneKind) -> Option<&PaneViewState> {
        self.panes.get(kind)
    }

    pub fn get_mut(&mut self, kind: &PaneKind) -> Option<&mut PaneViewState> {
        self.panes.get_mut(kind)
    }

    pub fn contains_key(&self, kind: &PaneKind) -> bool {
        self.panes.contains_key(kind)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&PaneKind, &PaneViewState)> {
        self.panes.iter()
    }

    pub fn values(&self) -> impl Iterator<Item = &PaneViewState> {
        self.panes.values()
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut PaneViewState> {
        self.panes.values_mut()
    }

    pub fn len(&self) -> usize {
        self.panes.len()
    }

    /// ペインを足す（テスト用。読み込んだ YAML の状態を作る）。グループなら、木も作り直す
    #[cfg(test)]
    pub fn insert(&mut self, kind: PaneKind, state: PaneViewState) {
        self.panes.insert(kind, state);
        self.rebuild_editor_tree();
    }

    /// エディタのグループの並び（木）
    pub fn editor(&self) -> &GroupNode {
        &self.editor
    }

    /// エディタのグループを、左（上）から順に
    pub fn groups(&self) -> Vec<PaneKind> {
        self.editor
            .groups()
            .into_iter()
            .map(PaneKind::Group)
            .collect()
    }

    /// `panes` にあるグループから、木を作り直す（番号の順に左右に並べる）。
    ///
    /// 読み込んだ YAML のように、グループのタブの並びしかないときに使う（木の形は、まだ保存しない）。
    fn rebuild_editor_tree(&mut self) {
        let mut ids: Vec<GroupId> = self
            .panes
            .keys()
            .filter_map(|k| match k {
                PaneKind::Group(id) => Some(*id),
                _ => None,
            })
            .collect();
        ids.sort_by_key(|id| id.0);
        if ids.is_empty() {
            ids.push(GroupId(0));
        }
        let mut node = GroupNode::Group(*ids.last().unwrap_or(&GroupId(0)));
        for id in ids.iter().rev().skip(1) {
            node = GroupNode::Split {
                axis: Axis::Vertical,
                ratio: 0.5,
                a: Box::new(GroupNode::Group(*id)),
                b: Box::new(node),
            };
        }
        self.editor = node;
    }

    /// 保存されていた木を使う。木にないグループは右端に足し、タブの並びがない葉には空のグループを作る
    /// （空のグループは、読み込みの `normalize_pane_views` で取り除く）。
    pub fn set_editor_tree(&mut self, tree: GroupNode) {
        let missing: Vec<PaneKind> = self
            .groups_in_panes()
            .into_iter()
            .filter(|kind| !matches!(kind, PaneKind::Group(id) if tree.groups().contains(id)))
            .collect();
        for id in tree.groups() {
            self.panes
                .entry(PaneKind::Group(id))
                .or_insert_with(|| PaneViewState::new(vec![], 0));
        }
        self.editor = tree;
        for kind in missing {
            // いったん外して、空のグループとして木の右端に足してから、タブの並びを戻す
            if let Some(state) = self.panes.remove(&kind) {
                *self.group_mut(kind) = state;
            }
        }
    }

    /// タブの並びのあるグループ（木とは関係なく）
    fn groups_in_panes(&self) -> Vec<PaneKind> {
        self.panes
            .keys()
            .filter(|k| matches!(k, PaneKind::Group(_)))
            .copied()
            .collect()
    }

    /// まだ使っていないグループの番号（今ある番号の最大 + 1）
    fn next_group_id(&self) -> GroupId {
        GroupId(
            self.editor
                .groups()
                .iter()
                .map(|id| id.0 + 1)
                .max()
                .unwrap_or(0),
        )
    }

    /// グループを取り除く（木と、タブの並びの両方から）。最後の 1 つは取り除かない。
    fn remove_group(&mut self, kind: PaneKind) -> Option<PaneViewState> {
        let PaneKind::Group(id) = kind else {
            return None;
        };
        if !self.editor.remove_leaf(id) {
            return None;
        }
        self.panes.remove(&kind)
    }

    /// タブのなくなったグループを取り除く（最後の 1 つは残す）。取り除いたら `true`。
    pub fn prune_empty_groups(&mut self) -> bool {
        let mut removed = false;
        for kind in self.groups() {
            if self.panes.get(&kind).is_none_or(|s| s.views.is_empty())
                && self.remove_group(kind).is_some()
            {
                removed = true;
            }
        }
        removed
    }

    /// 分割の比率を、`ratio_of` から取り込む（`GroupNode::sync_ratios`）
    pub fn sync_ratios(&mut self, ratio_of: &impl Fn(&[PaneKind], &[PaneKind]) -> Option<f32>) {
        self.editor.sync_ratios(ratio_of);
    }

    /// タブをグループの端へ落として、新しいグループを作る。`target` を `edge` の側に分割し、`from` のペイン
    /// （エディタのグループかドック）から `view` を新しいグループへ移して選ぶ。分割するのは、エディタのグループだけ。元のグループが空になっても、ここでは残す（`prune_empty_groups`）。
    /// 同じグループの唯一のタブは、分割しない（移すと元が空になり、同じ形に戻るだけ）。新しいグループを返す。
    pub fn split_with_view(
        &mut self,
        view: ViewId,
        from: PaneKind,
        target: PaneKind,
        edge: DropZone,
    ) -> Option<PaneKind> {
        let PaneKind::Group(target_id) = target else {
            return None;
        };
        if edge == DropZone::Center || !self.panes.contains_key(&target) {
            return None;
        }
        let source = self.panes.get(&from)?;
        if !source.views.contains(&view) || (from == target && source.views.len() < 2) {
            return None;
        }
        let new_id = self.next_group_id();
        if !self.editor.split_leaf(target_id, new_id, edge) {
            return None;
        }
        self.panes.get_mut(&from)?.remove_view(view);
        let new_kind = PaneKind::Group(new_id);
        self.panes
            .insert(new_kind, PaneViewState::new(vec![view], 0));
        Some(new_kind)
    }

    /// 1 つのグループを、右に分割する（View → Split Editor Right）。新しいグループを返す。
    fn split_single_group_right(&mut self) -> Option<PaneKind> {
        let GroupNode::Group(only) = self.editor else {
            return None;
        };
        let new_id = self.next_group_id();
        self.editor.split_leaf(only, new_id, DropZone::Right);
        let new_kind = PaneKind::Group(new_id);
        self.panes.insert(new_kind, PaneViewState::new(vec![], 0));
        Some(new_kind)
    }

    /// すべてのグループを、最初のグループにまとめる（タブは、グループの順に末尾へ足す）
    fn merge_all_groups(&mut self) {
        let groups = self.groups();
        let Some((&first, rest)) = groups.split_first() else {
            return;
        };
        for kind in rest {
            if let Some(state) = self.remove_group(*kind) {
                self.group_mut(first).views.extend(state.views);
            }
        }
    }

    /// ペインの間で、タブを移す（ドラッグ）。エディタのグループとドック（左・右・下）のどちらからでも、どちらへでも
    /// 移せる。`before` があれば、そのタブの位置に、なければ末尾に入れ、移したタブを選ぶ。元のペインでは、
    /// 選んでいたタブをできるだけ保つ。空になっても残す（グループは `prune_empty_groups` で消し、ドックは残す）。
    /// 移したときは `true` を返す。
    pub fn move_view_between(
        &mut self,
        view: ViewId,
        from: PaneKind,
        to: PaneKind,
        before: Option<ViewId>,
    ) -> bool {
        if from == to || !self.panes.contains_key(&to) {
            return false;
        }
        if !self
            .panes
            .get_mut(&from)
            .is_some_and(|source| source.remove_view(view))
        {
            return false;
        }
        let target = self.group_mut(to);
        let index = before
            .and_then(|b| target.views.iter().position(|v| *v == b))
            .unwrap_or(target.views.len());
        target.views.insert(index, view);
        target.active_index = index;
        true
    }

    /// グループのタブの並び。なければ、空のグループとして作り、木の右端に足す
    fn group_mut(&mut self, kind: PaneKind) -> &mut PaneViewState {
        if let std::collections::btree_map::Entry::Vacant(entry) = self.panes.entry(kind) {
            entry.insert(PaneViewState::new(vec![], 0));
            if let PaneKind::Group(id) = kind
                && !self.editor.groups().contains(&id)
            {
                let root = std::mem::replace(&mut self.editor, GroupNode::Group(id));
                self.editor = GroupNode::Split {
                    axis: Axis::Vertical,
                    ratio: 0.5,
                    a: Box::new(root),
                    b: Box::new(GroupNode::Group(id)),
                };
            }
        }
        self.panes.get_mut(&kind).expect("just inserted")
    }
}

/// 既定のペイン構成（現在の配置と100%一致）
pub fn default_pane_views() -> PaneLayout {
    let mut map = BTreeMap::new();
    map.insert(
        PaneKind::Left,
        PaneViewState::new(
            vec![
                ViewId::StructureTree,
                ViewId::NamedSelections,
                ViewId::ProjectTree,
            ],
            0,
        ),
    );
    map.insert(
        PaneKind::CENTER,
        PaneViewState::new(vec![ViewId::Viewport], 0),
    );
    map.insert(
        PaneKind::Right,
        PaneViewState::new(vec![ViewId::RepresentationInspector], 0),
    );
    map.insert(
        PaneKind::Bottom,
        PaneViewState::new(
            vec![
                ViewId::LogViewer,
                ViewId::PythonConsole,
                ViewId::Terminal,
                ViewId::AiChat,
            ],
            0,
        ),
    );
    PaneLayout::from(map)
}

/// テキストエディタを中央の右側に分けて表示しているか（エディタのグループが 2 つ以上あるか）。
///
/// エディタのグループが 2 つ以上ある（View → Split Editor Right の印）。
pub fn is_editor_split(pane_views: &PaneLayout) -> bool {
    pane_views.editor().groups().len() > 1
}

/// 2 つ目の分子図（`Viewport2`）のタブが、どこかのグループにある
pub fn has_second_viewport(pane_views: &PaneLayout) -> bool {
    pane_views
        .values()
        .any(|s| s.views.contains(&ViewId::Viewport2))
}

fn is_document(view: &ViewId) -> bool {
    matches!(view, ViewId::Document(_))
}

/// 配置の規則: そのビューを置くグループ。エディタのグループに置くビュー（3D Viewport、2 つ目の 3D Viewport、
/// ドキュメント）なら、そのグループを返す。ドックに置くビュー、または置き場所がないとき（分割していないときの
/// 2 つ目の 3D Viewport）は `None`。
///
/// - 3D Viewport: 最初のグループ
/// - 2 つ目の 3D Viewport: 2 つ目のグループ（分割しているときだけ）
/// - ドキュメント: 最後のグループ（分割していれば右側、していなければ中央）
pub fn home_group(pane_views: &PaneLayout, view: ViewId) -> Option<PaneKind> {
    let groups = pane_views.groups();
    match view {
        ViewId::Viewport => groups.first().copied(),
        ViewId::Viewport2 => groups.get(1).copied(),
        ViewId::Document(_) => groups.last().copied(),
        _ => None,
    }
}

/// 指定した種類のタブを、すべてのペインから外す（アクティブなタブはできるだけ保つ）。
/// 外したタブを、見つけた順に返す。
fn take_views(pane_views: &mut PaneLayout, pred: impl Fn(&ViewId) -> bool) -> Vec<ViewId> {
    let mut taken = Vec::new();
    for state in pane_views.values_mut() {
        while let Some(v) = state.views.iter().copied().find(|v| pred(v)) {
            state.remove_view(v);
            taken.push(v);
        }
    }
    taken
}

/// 指定した種類のタブを、エディタのグループから外す（ドックのタブは、そのまま）。外したタブを、グループの順に返す。
fn take_group_views(pane_views: &mut PaneLayout, pred: impl Fn(&ViewId) -> bool) -> Vec<ViewId> {
    let mut taken = Vec::new();
    for kind in pane_views.groups() {
        if let Some(state) = pane_views.get_mut(&kind) {
            while let Some(v) = state.views.iter().copied().find(|v| pred(v)) {
                state.remove_view(v);
                taken.push(v);
            }
        }
    }
    taken
}

/// 選んでいるテキストファイルのタブのドキュメント。
///
/// 右（後ろ）のグループから順に、アクティブなタブを調べる。グループになければ、ドック（左・右・下）を調べる
/// （タブをドックへ移したとき）。
pub fn active_document(pane_views: &PaneLayout) -> Option<DocumentId> {
    let docks = [PaneKind::Left, PaneKind::Right, PaneKind::Bottom];
    pane_views
        .groups()
        .iter()
        .rev()
        .chain(docks.iter())
        .find_map(|kind| match pane_views.get(kind)?.active_view()? {
            ViewId::Document(id) => Some(id),
            _ => None,
        })
}

/// ドキュメントのタブを追加する。分割なしなら中央のペイン、分割ありなら右側に並べる（`home_group`）。
/// すでにタブがあれば追加しない。`select` なら、そのタブを選ぶ。
pub fn add_document_tab(pane_views: &mut PaneLayout, id: DocumentId, select: bool) {
    let view = ViewId::Document(id);
    if !pane_views.values().any(|s| s.views.contains(&view)) {
        let target = home_group(pane_views, view).unwrap_or(PaneKind::CENTER);
        pane_views.group_mut(target).views.push(view);
    }
    if select {
        select_document_tab(pane_views, id);
    }
}

/// ドキュメントのタブを選ぶ。タブがあれば `true`。
pub fn select_document_tab(pane_views: &mut PaneLayout, id: DocumentId) -> bool {
    pane_views
        .values_mut()
        .any(|s| s.select_view(ViewId::Document(id)))
}

/// ドキュメントのタブを閉じる。閉じたあとは、隣のタブが選ばれる。タブがあれば `true`。
pub fn close_document_tab(pane_views: &mut PaneLayout, id: DocumentId) -> bool {
    pane_views
        .values_mut()
        .any(|s| s.remove_view(ViewId::Document(id)))
}

/// 開いているドキュメントのタブを、分割の状態に合わせて並べ直す（`pane_views` を差し替えたあとに使う）。
/// 選んでいたタブは `prev_active` で渡せば、そのタブを選び直す。
pub fn attach_documents(
    pane_views: &mut PaneLayout,
    ids: &[DocumentId],
    prev_active: Option<DocumentId>,
) {
    for id in ids {
        add_document_tab(pane_views, *id, false);
    }
    if let Some(id) = prev_active {
        select_document_tab(pane_views, id);
    }
}

/// View → Split Editor Right のオン・オフ。
///
/// - オン（グループが 1 つのとき）: 右に新しいグループを作り、2つ目の分子図のタブ（`Viewport2`）と、ドキュメントの
///   タブを並べる（開いているファイルがなくても分割する）。選んでいたドキュメントがあれば、そのタブを選ぶ。
///   すでに分割しているときは、何もしない。
/// - オフ: `Viewport2` のタブを消し、すべてのグループを最初のグループにまとめる。ドキュメントのタブは末尾に戻す。
///
/// ドキュメントの内容は `Yui` の `DocumentStore` にあり、タブの置き場所とは独立なので、切り替えで失われない。
pub fn set_editor_split(pane_views: &mut PaneLayout, on: bool) {
    if on && is_editor_split(pane_views) {
        return;
    }
    let prev_active = active_document(pane_views);
    // ドックへ移したドキュメントは、そのまま残す（動かすのは、エディタのグループにあるものだけ）
    let docs = take_group_views(pane_views, is_document);
    take_views(pane_views, |v| matches!(v, ViewId::Viewport2));
    let target = if on {
        let right = pane_views
            .split_single_group_right()
            .unwrap_or(PaneKind::CENTER);
        pane_views.group_mut(right).views.push(ViewId::Viewport2);
        right
    } else {
        pane_views.merge_all_groups();
        pane_views.groups()[0]
    };
    let state = pane_views.group_mut(target);
    state.views.extend(docs);
    if let Some(id) = prev_active {
        state.select_view(ViewId::Document(id));
    }
}

/// 読み込んだペイン構成を、今の配置の規則に合わせる。
///
/// - ドキュメントのタブ（保存しないもの）と、古い 1 つだけのテキストエディタのタブは取り除く。
/// - 同じ分子図のタブが 2 つ以上あれば、後ろのものを取り除く（手で書き換えた YAML など）。
///   分割していなければ、2つ目の分子図（`Viewport2`）を取り除く。
/// - 2つ目の分子図がどこにもなく、`center_right` のグループが空なら、そこに 2つ目の分子図を置く（2つ目の分子図より
///   前の版で、分割して保存したワークスペース）。
/// - 3D Viewport がどのグループにもなければ、最初のグループの先頭に置く（`home_group`）。
///   タブをドラッグして、ほかのグループへ移したものは、その場所のまま残す。
/// - タブのなくなったグループは取り除く（`prune_empty_groups`）。
pub fn normalize_pane_views(pane_views: &mut PaneLayout) {
    let split = is_editor_split(pane_views);
    take_views(pane_views, |v| {
        is_document(v)
            || matches!(v, ViewId::LegacyTextEditor)
            || (!split && *v == ViewId::Viewport2)
    });
    for view in [ViewId::Viewport, ViewId::Viewport2] {
        let mut seen = false;
        for state in pane_views.values_mut() {
            if state.views.contains(&view) {
                if seen {
                    state.remove_view(view);
                }
                seen = true;
            }
        }
    }
    // 2つ目の分子図より前の版の分割（中身が空の `center_right`）は、2つ目の分子図を足して、分割のまま開く
    if !has_second_viewport(pane_views)
        && let Some(right) = pane_views.get_mut(&PaneKind::CENTER_RIGHT)
        && right.views.is_empty()
    {
        right.views.push(ViewId::Viewport2);
        right.active_index = 0;
    }
    if !pane_views
        .values()
        .any(|s| s.views.contains(&ViewId::Viewport))
    {
        let home = home_group(pane_views, ViewId::Viewport).unwrap_or(PaneKind::CENTER);
        let group = pane_views.group_mut(home);
        group.views.insert(0, ViewId::Viewport);
        group.active_index = 0;
    }
    pane_views.prune_empty_groups();
}

/// 保存用に、ドキュメントのタブを取り除いた複製を返す（空になったグループは、読み込みのときに取り除く）。
pub fn pane_views_for_saving(pane_views: &PaneLayout) -> PaneLayout {
    let mut copy = pane_views.clone();
    take_views(&mut copy, is_document);
    copy
}

/// タブの帯の横スクロールの識別子（縦のホイールを、横のスクロールに変えるときに使う）
pub fn tab_bar_scroll_id(pane: PaneKind) -> iced::widget::Id {
    iced::widget::Id::from(format!("yui-tab-bar-{}", pane.key()))
}

/// 汎用タブバーUIウィジェットの構築。入りきらないタブは、エディタのグループでは横にスクロールし、
/// ドックでは折り返す。
///
/// - `label_of`: タブの名前（ドキュメントはファイル名。未保存なら `*` 付き）
/// - `on_select`: タブを押した（選択と、ドラッグの開始）
/// - `on_hover`: マウスがタブの上に入った（ドラッグ中なら、そこへ並べ替える）
/// - `on_close`: ドキュメントのタブの閉じるボタン（`×`）
/// - `on_wheel`: 縦のホイールを、横のスクロールの量に変えたもの
pub fn view_tab_bar<'a, Message: Clone + 'a>(
    pane: PaneKind,
    pane_state: &PaneViewState,
    label_of: impl Fn(ViewId) -> String,
    on_select: impl Fn(PaneKind, ViewId) -> Message + 'a,
    on_hover: impl Fn(PaneKind, ViewId) -> Message + 'a,
    on_close: impl Fn(DocumentId) -> Message + 'a,
    on_wheel: impl Fn(PaneKind, f32) -> Message + 'a,
) -> Element<'a, Message> {
    let palette = crate::ui_style::palette();
    let ui = crate::ui_style::ui_settings();
    let (tab_style, font_size) = (ui.tab_style, ui.tab_font_size);
    let mut tab_buttons: Vec<Element<'a, Message>> = Vec::new();
    for (idx, &view) in pane_state.views.iter().enumerate() {
        let label = label_of(view);
        let is_active = idx == pane_state.active_index;
        // ドキュメントのタブは、右に閉じるボタンが続くので、右の余白を詰める
        let padding = if matches!(view, ViewId::Document(_)) {
            iced::Padding {
                top: 4.0,
                right: 2.0,
                bottom: 4.0,
                left: 10.0,
            }
        } else {
            iced::Padding::from([4.0, 10.0])
        };
        // 押したことは、ボタンではなく中の `mouse_area` で受ける（ボタンは、離したときにしか知らせないため）。
        // ボタンは、マウスが乗ったときの色のために残す。余白を 0 にして、`mouse_area` がタブ全体を覆うようにする。
        let label = mouse_area(
            container(
                text(label)
                    .size(font_size)
                    .wrapping(iced::widget::text::Wrapping::None),
            )
            .padding(padding),
        )
        .on_press(on_select(pane, view))
        .on_enter(on_hover(pane, view));
        let btn = button(label)
            .on_press(on_select(pane, view))
            .padding(0)
            .style(move |_theme, status| {
                crate::ui_style::tab_button_style(&palette, tab_style, is_active, status)
            });
        let mut tab = row![btn].spacing(0);
        if let ViewId::Document(id) = view {
            tab = tab.push(
                button(text("×").size(font_size))
                    .on_press(on_close(id))
                    .padding([4, 6])
                    .style(move |_theme, status| {
                        crate::ui_style::tab_button_style(&palette, tab_style, false, status)
                    }),
            );
        }
        // 選択中のタブの下の線。タブの幅に合わせるため、外側の枠の背景を、下の余白だけに見せる
        // （内側は、帯と同じ色で塗りつぶす。`Filled` では、背景で示すので、線は引かない）。
        let underline = is_active && tab_style == crate::ui_style::TabStyle::Underline;
        let inner = container(tab).style(move |_theme| crate::ui_style::tab_bar_style(&palette));
        let outer = container(inner)
            .padding(iced::Padding {
                bottom: crate::ui_style::TAB_INDICATOR_HEIGHT,
                ..iced::Padding::ZERO
            })
            .style(move |_theme| crate::ui_style::tab_indicator_style(&palette, underline));
        tab_buttons.push(outer.into());
    }

    // エディタのグループは、横にスクロールする（VSCode と同じ）。横のスクロールは、トラックパッドと
    // Shift + ホイールで動く。縦のホイールは、`on_wheel` で横に変える。
    // 左・右・下のドックは、細いことが多いので、今までどおり折り返して、すべてのタブを見せる。
    let strip: Element<'a, Message> = if matches!(pane, PaneKind::Group(_)) {
        let strip = scrollable(row(tab_buttons).spacing(2))
            .id(tab_bar_scroll_id(pane))
            .direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::new().width(3).scroller_width(3),
            ))
            .width(Fill);
        mouse_area(strip)
            .on_scroll(move |delta| {
                let dy = match delta {
                    iced::mouse::ScrollDelta::Lines { y, .. } => y * 60.0,
                    iced::mouse::ScrollDelta::Pixels { y, .. } => y,
                };
                on_wheel(pane, -dy)
            })
            .into()
    } else {
        // 幅を `Fill` にした部品を足すと、折り返しの計算で、タブが 1 つずつ行を占めてしまう
        row(tab_buttons)
            .spacing(2)
            .wrap()
            .vertical_spacing(2)
            .into()
    };
    let tabs = container(strip)
        .width(Fill)
        .padding([2, 6])
        .style(move |_theme| crate::ui_style::tab_bar_style(&palette));
    // 帯の下に、細い区切り線
    column![tabs, crate::ui_style::menu_separator(palette)].into()
}

/// 保存するエディタのグループの木。グループは、`pane_views` と同じキー（`center`、`center_right`、`group_N`）で書く。
///
/// ```yaml
/// editor_layout:
///   split: stacked      # side_by_side（左右）か stacked（上下）
///   ratio: 0.3          # 左（上）の側の割合
///   a: { group: center }
///   b: { group: group_2 }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SavedGroupNode {
    Group {
        group: String,
    },
    Split {
        split: SavedSplitDirection,
        /// 左（上）の側の割合（0〜1）
        ratio: f32,
        a: Box<SavedGroupNode>,
        b: Box<SavedGroupNode>,
    },
}

/// 保存する分割の向き
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SavedSplitDirection {
    SideBySide,
    Stacked,
}

impl From<&GroupNode> for SavedGroupNode {
    fn from(node: &GroupNode) -> Self {
        match node {
            GroupNode::Group(id) => SavedGroupNode::Group {
                group: PaneKind::Group(*id).key(),
            },
            GroupNode::Split { axis, ratio, a, b } => SavedGroupNode::Split {
                split: match axis {
                    Axis::Vertical => SavedSplitDirection::SideBySide,
                    Axis::Horizontal => SavedSplitDirection::Stacked,
                },
                ratio: *ratio,
                a: Box::new(a.as_ref().into()),
                b: Box::new(b.as_ref().into()),
            },
        }
    }
}

impl SavedGroupNode {
    /// 木に戻す。グループでないキー、同じグループが 2 回出てくる、割合が範囲外のときは `None`
    pub fn to_tree(&self) -> Option<GroupNode> {
        fn walk(node: &SavedGroupNode, seen: &mut Vec<GroupId>) -> Option<GroupNode> {
            match node {
                SavedGroupNode::Group { group: key } => match PaneKind::from_key(key)? {
                    PaneKind::Group(id) if !seen.contains(&id) => {
                        seen.push(id);
                        Some(GroupNode::Group(id))
                    }
                    _ => None,
                },
                SavedGroupNode::Split {
                    split: direction,
                    ratio,
                    a,
                    b,
                } => {
                    if !(0.0..=1.0).contains(ratio) {
                        return None;
                    }
                    Some(GroupNode::Split {
                        axis: match direction {
                            SavedSplitDirection::SideBySide => Axis::Vertical,
                            SavedSplitDirection::Stacked => Axis::Horizontal,
                        },
                        ratio: *ratio,
                        a: Box::new(walk(a, seen)?),
                        b: Box::new(walk(b, seen)?),
                    })
                }
            }
        }
        walk(self, &mut Vec::new())
    }
}

/// state.yaml 保存用の拡張ワークスペース状態（AppState + ペインビュー構成）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedWorkspaceState {
    #[serde(flatten)]
    pub app_state: AppState,
    #[serde(default = "default_pane_views")]
    pub pane_views: PaneLayout,
    /// エディタのグループの並び（木の形と、分割の割合）。グループが 1 つのときは書かない。
    /// ない YAML（以前の版）は、グループを番号の順に左右に並べる。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor_layout: Option<SavedGroupNode>,
}

/// AppState とペイン構成を新形式 YAML 文字列へシリアライズする純粋関数
pub fn export_workspace_to_yaml_str(
    app_state: &AppState,
    pane_views: &PaneLayout,
) -> Result<String, String> {
    let saved = SavedWorkspaceState {
        app_state: app_state.clone(),
        pane_views: pane_views_for_saving(pane_views),
        editor_layout: is_editor_split(pane_views).then(|| pane_views.editor().into()),
    };
    serde_yaml::to_string(&saved).map_err(|e| format!("YAML export error: {e}"))
}

/// YAML 文字列から AppState とペイン構成を復元する純粋関数（旧形式フォールバック対応）
pub fn import_workspace_from_yaml_str(yaml_str: &str) -> Result<(AppState, PaneLayout), String> {
    // 1. 新形式（SavedWorkspaceState: AppState + pane_views）でのデシリアライズを試行
    match serde_yaml::from_str::<SavedWorkspaceState>(yaml_str) {
        Ok(mut saved) => {
            saved.app_state.sync_molecule_from_atom_group();
            if let Some(tree) = saved
                .editor_layout
                .as_ref()
                .and_then(SavedGroupNode::to_tree)
            {
                saved.pane_views.set_editor_tree(tree);
            }
            normalize_pane_views(&mut saved.pane_views);
            Ok((saved.app_state, saved.pane_views))
        }
        Err(_) => {
            // 2. 旧形式（AppState 単体）でのパース試行
            let mut state: AppState = serde_yaml::from_str(yaml_str).map_err(|e| {
                format!("Failed to parse YAML (neither new nor legacy format): {e}")
            })?;
            state.sync_molecule_from_atom_group();
            Ok((state, default_pane_views()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yui_core::AtomGroupExt;

    #[test]
    fn test_pane_kind_keys_and_yaml_order_are_unchanged() {
        for (kind, key) in [
            (PaneKind::Left, "left"),
            (PaneKind::CENTER, "center"),
            (PaneKind::Right, "right"),
            (PaneKind::Bottom, "bottom"),
            (PaneKind::CENTER_RIGHT, "center_right"),
            (PaneKind::Group(GroupId(5)), "group_5"),
        ] {
            assert_eq!(kind.key(), key);
            assert_eq!(PaneKind::from_key(key), Some(kind));
        }
        assert_eq!(PaneKind::from_key("nothing"), None);
        // 保存の YAML のキーの順は、以前（Left, Center, Right, Bottom, CenterRight）と同じ
        let mut views = default_pane_views();
        set_editor_split(&mut views, true);
        let yaml = serde_yaml::to_string(&views).expect("yaml");
        let pos = |k: &str| yaml.find(&format!("{k}:")).expect(k);
        assert!(pos("left") < pos("center"));
        assert!(pos("center") < pos("right"));
        assert!(pos("right") < pos("bottom"));
        assert!(pos("bottom") < pos("center_right"));
        // 読み直すと、同じ構成（木も同じ）
        let back: PaneLayout = serde_yaml::from_str(&yaml).expect("parse");
        assert_eq!(back, views);
    }

    #[test]
    fn test_editor_tree_follows_the_groups() {
        let mut views = default_pane_views();
        assert_eq!(views.editor(), &GroupNode::Group(GroupId(0)));
        assert_eq!(views.groups(), vec![PaneKind::CENTER]);
        set_editor_split(&mut views, true);
        assert_eq!(
            views.editor(),
            &GroupNode::Split {
                axis: Axis::Vertical,
                ratio: 0.5,
                a: Box::new(GroupNode::Group(GroupId(0))),
                b: Box::new(GroupNode::Group(GroupId(1))),
            }
        );
        assert_eq!(
            views.groups(),
            vec![PaneKind::CENTER, PaneKind::CENTER_RIGHT]
        );
        set_editor_split(&mut views, false);
        assert_eq!(views.editor(), &GroupNode::Group(GroupId(0)));
    }

    #[test]
    fn test_home_group_is_the_single_placement_rule() {
        let doc = ViewId::Document(DocumentId(3));
        let mut views = default_pane_views();
        assert_eq!(home_group(&views, ViewId::Viewport), Some(PaneKind::CENTER));
        assert_eq!(home_group(&views, ViewId::Viewport2), None);
        assert_eq!(home_group(&views, doc), Some(PaneKind::CENTER));
        assert_eq!(home_group(&views, ViewId::Terminal), None);
        set_editor_split(&mut views, true);
        assert_eq!(home_group(&views, ViewId::Viewport), Some(PaneKind::CENTER));
        assert_eq!(
            home_group(&views, ViewId::Viewport2),
            Some(PaneKind::CENTER_RIGHT)
        );
        assert_eq!(home_group(&views, doc), Some(PaneKind::CENTER_RIGHT));
    }

    #[test]
    fn test_default_pane_views_assignment() {
        let views = default_pane_views();

        assert_eq!(
            views.get(&PaneKind::Left).unwrap().views,
            vec![
                ViewId::StructureTree,
                ViewId::NamedSelections,
                ViewId::ProjectTree,
            ]
        );
        assert_eq!(
            views.get(&PaneKind::Left).unwrap().active_view(),
            Some(ViewId::StructureTree)
        );

        assert_eq!(
            views.get(&PaneKind::CENTER).unwrap().views,
            vec![ViewId::Viewport]
        );
        assert_eq!(
            views.get(&PaneKind::CENTER).unwrap().active_view(),
            Some(ViewId::Viewport)
        );

        assert_eq!(
            views.get(&PaneKind::Right).unwrap().views,
            vec![ViewId::RepresentationInspector]
        );
        assert_eq!(
            views.get(&PaneKind::Right).unwrap().active_view(),
            Some(ViewId::RepresentationInspector)
        );

        let bottom = views.get(&PaneKind::Bottom).unwrap();
        assert_eq!(
            bottom.views,
            vec![
                ViewId::LogViewer,
                ViewId::PythonConsole,
                ViewId::Terminal,
                ViewId::AiChat,
            ]
        );
        assert_eq!(bottom.active_view(), Some(ViewId::LogViewer));
        assert!(!views.contains_key(&PaneKind::CENTER_RIGHT));
        assert!(!is_editor_split(&views));
    }

    #[test]
    fn test_pane_view_switching() {
        let mut state = PaneViewState::new(
            vec![
                ViewId::LogViewer,
                ViewId::PythonConsole,
                ViewId::Terminal,
                ViewId::AiChat,
            ],
            0,
        );
        assert_eq!(state.active_view(), Some(ViewId::LogViewer));

        // Terminal を選択
        assert!(state.select_view(ViewId::Terminal));
        assert_eq!(state.active_view(), Some(ViewId::Terminal));
        assert_eq!(state.active_index, 2);

        // インデックス直接切り替え
        assert!(state.select_index(1));
        assert_eq!(state.active_view(), Some(ViewId::PythonConsole));

        // 存在しないビュー選択は失敗し変更されない
        assert!(!state.select_view(ViewId::Viewport));
        assert_eq!(state.active_view(), Some(ViewId::PythonConsole));
    }

    #[test]
    fn test_workspace_yaml_round_trip() {
        let mut app_state = AppState::default();
        let (ag, sec_map) = yui_core::AtomGroup::create_test_peptide();
        app_state.atom_group = Some(ag);
        app_state.secondary_structure_map = sec_map;
        app_state.sync_molecule_from_atom_group();

        let mut pane_views = default_pane_views();
        // Bottom で Terminal をアクティブに変更
        pane_views
            .get_mut(&PaneKind::Bottom)
            .unwrap()
            .select_view(ViewId::Terminal);

        let yaml = export_workspace_to_yaml_str(&app_state, &pane_views).expect("Export failed");

        let (loaded_state, loaded_views) =
            import_workspace_from_yaml_str(&yaml).expect("Import failed");

        assert_eq!(loaded_state.atom_group, app_state.atom_group);
        assert_eq!(
            loaded_views.get(&PaneKind::Bottom).unwrap().active_view(),
            Some(ViewId::Terminal)
        );
        assert_eq!(
            loaded_views.get(&PaneKind::CENTER).unwrap().active_view(),
            Some(ViewId::Viewport)
        );
    }

    #[test]
    fn test_legacy_yaml_fallback_to_default_views() {
        let mut app_state = AppState::default();
        let (ag, sec_map) = yui_core::AtomGroup::create_test_peptide();
        app_state.atom_group = Some(ag);
        app_state.secondary_structure_map = sec_map;
        app_state.sync_molecule_from_atom_group();

        // 旧形式（yui_core::yaml からの純粋な AppState YAML）
        let legacy_yaml =
            yui_core::yaml::export_app_state_to_yaml_str(&app_state).expect("Legacy export failed");

        // 旧形式 YAML を読み込んでもパニックせず、既定のビュー構成にフォールバックすること
        let (loaded_state, loaded_views) =
            import_workspace_from_yaml_str(&legacy_yaml).expect("Legacy import should succeed");

        assert_eq!(loaded_state.atom_group, app_state.atom_group);
        assert_eq!(loaded_views, default_pane_views());
        assert_eq!(
            loaded_views.get(&PaneKind::Bottom).unwrap().active_view(),
            Some(ViewId::LogViewer)
        );
    }

    const D1: DocumentId = DocumentId(1);
    const D2: DocumentId = DocumentId(2);
    const D3: DocumentId = DocumentId(3);

    /// ペインのタブ（2つ目の分子図 `Viewport2` は除く。そのタブは専用のテストで確かめる）
    fn doc_views(views: &PaneLayout, kind: PaneKind) -> Vec<ViewId> {
        views
            .get(&kind)
            .map(|s| {
                s.views
                    .iter()
                    .copied()
                    .filter(|v| *v != ViewId::Viewport2)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn right_tabs(views: &PaneLayout) -> Vec<ViewId> {
        views[&PaneKind::CENTER_RIGHT].views.clone()
    }

    #[test]
    fn test_split_adds_second_viewport_tab_and_unsplit_removes_it() {
        let mut views = default_pane_views();
        set_editor_split(&mut views, true);
        assert_eq!(right_tabs(&views), vec![ViewId::Viewport2]);
        assert_eq!(
            views[&PaneKind::CENTER_RIGHT].active_view(),
            Some(ViewId::Viewport2)
        );
        assert_eq!(doc_views(&views, PaneKind::CENTER), vec![ViewId::Viewport]);

        // ドキュメントを開くとタブはその右に並び、分割をオフにすると 2つ目の分子図のタブだけが消える
        add_document_tab(&mut views, D1, true);
        assert_eq!(
            right_tabs(&views),
            vec![ViewId::Viewport2, ViewId::Document(D1)]
        );
        set_editor_split(&mut views, false);
        assert!(!views.contains_key(&PaneKind::CENTER_RIGHT));
        assert!(!views.values().any(|s| s.views.contains(&ViewId::Viewport2)));
        assert_eq!(
            doc_views(&views, PaneKind::CENTER),
            vec![ViewId::Viewport, ViewId::Document(D1)]
        );
    }

    #[test]
    fn test_split_keeps_the_selected_document_active() {
        let mut views = default_pane_views();
        add_document_tab(&mut views, D1, true);
        set_editor_split(&mut views, true);
        assert_eq!(active_document(&views), Some(D1));
    }

    #[test]
    fn test_second_viewport_tab_is_normalized_on_load() {
        // 古い分割（中身が空の center_right）には、2つ目の分子図のタブを足す
        let mut views = default_pane_views();
        views.insert(PaneKind::CENTER_RIGHT, PaneViewState::new(vec![], 0));
        normalize_pane_views(&mut views);
        assert_eq!(right_tabs(&views), vec![ViewId::Viewport2]);

        // 重ねて読み込んでも増えない
        normalize_pane_views(&mut views);
        assert_eq!(right_tabs(&views), vec![ViewId::Viewport2]);

        // 分割していないのに Viewport2 が残っている YAML は、取り除く
        let mut views = default_pane_views();
        views
            .get_mut(&PaneKind::CENTER)
            .unwrap()
            .views
            .push(ViewId::Viewport2);
        normalize_pane_views(&mut views);
        assert_eq!(views[&PaneKind::CENTER].views, vec![ViewId::Viewport]);
        assert!(!views.contains_key(&PaneKind::CENTER_RIGHT));
    }

    #[test]
    fn test_add_document_tab_goes_to_center_when_not_split() {
        let mut views = default_pane_views();
        add_document_tab(&mut views, D1, true);
        add_document_tab(&mut views, D2, true);
        assert_eq!(
            doc_views(&views, PaneKind::CENTER),
            vec![ViewId::Viewport, ViewId::Document(D1), ViewId::Document(D2)]
        );
        // 追加したタブが選ばれる
        assert_eq!(active_document(&views), Some(D2));
        // すでにあるタブは、増えずに選ばれる
        add_document_tab(&mut views, D1, true);
        assert_eq!(doc_views(&views, PaneKind::CENTER).len(), 3);
        assert_eq!(active_document(&views), Some(D1));
        assert!(!is_editor_split(&views));
    }

    #[test]
    fn test_add_document_tab_goes_to_right_when_split() {
        let mut views = default_pane_views();
        set_editor_split(&mut views, true);
        add_document_tab(&mut views, D1, true);
        assert_eq!(
            doc_views(&views, PaneKind::CENTER_RIGHT),
            vec![ViewId::Document(D1)]
        );
        assert_eq!(doc_views(&views, PaneKind::CENTER), vec![ViewId::Viewport]);
        assert_eq!(active_document(&views), Some(D1));
    }

    #[test]
    fn test_close_document_tab_selects_neighbor_and_keeps_viewport() {
        let mut views = default_pane_views();
        for id in [D1, D2, D3] {
            add_document_tab(&mut views, id, true);
        }
        select_document_tab(&mut views, D2);
        // 真ん中のタブを閉じると、隣（次）のタブが選ばれる
        assert!(close_document_tab(&mut views, D2));
        assert_eq!(active_document(&views), Some(D3));
        // 最後のタブを閉じると、手前のタブが選ばれる
        assert!(close_document_tab(&mut views, D3));
        assert_eq!(active_document(&views), Some(D1));
        // 残りを閉じると、ビューポートが選ばれる。ビューポートのタブは残る
        assert!(close_document_tab(&mut views, D1));
        assert_eq!(active_document(&views), None);
        assert_eq!(doc_views(&views, PaneKind::CENTER), vec![ViewId::Viewport]);
        assert_eq!(
            views.get(&PaneKind::CENTER).unwrap().active_view(),
            Some(ViewId::Viewport)
        );
        // 無いタブは閉じられない
        assert!(!close_document_tab(&mut views, D1));
    }

    #[test]
    fn test_viewport_tab_is_not_closable_via_document_api() {
        // ドキュメント用の API は、ビューポートのタブを閉じない
        let mut views = default_pane_views();
        assert!(!close_document_tab(&mut views, DocumentId(0)));
        assert_eq!(doc_views(&views, PaneKind::CENTER), vec![ViewId::Viewport]);
    }

    #[test]
    fn test_editor_split_moves_document_tabs_and_keeps_selection() {
        let mut views = default_pane_views();
        add_document_tab(&mut views, D1, true);
        add_document_tab(&mut views, D2, true);
        select_document_tab(&mut views, D1);

        set_editor_split(&mut views, true);
        assert!(is_editor_split(&views));
        assert_eq!(doc_views(&views, PaneKind::CENTER), vec![ViewId::Viewport]);
        assert_eq!(
            doc_views(&views, PaneKind::CENTER_RIGHT),
            vec![ViewId::Document(D1), ViewId::Document(D2)]
        );
        assert_eq!(active_document(&views), Some(D1));

        set_editor_split(&mut views, false);
        assert!(!is_editor_split(&views));
        assert!(!views.contains_key(&PaneKind::CENTER_RIGHT));
        assert_eq!(
            doc_views(&views, PaneKind::CENTER),
            vec![ViewId::Viewport, ViewId::Document(D1), ViewId::Document(D2)]
        );
        assert_eq!(active_document(&views), Some(D1));
        // 他のペインは変わらない
        let defaults = default_pane_views();
        for kind in [PaneKind::Left, PaneKind::Right, PaneKind::Bottom] {
            assert_eq!(views.get(&kind), defaults.get(&kind));
        }
    }

    #[test]
    fn test_editor_split_is_kept_without_open_files() {
        // 開いているファイルがなくても、分割の状態を保てる
        let mut views = default_pane_views();
        set_editor_split(&mut views, true);
        assert!(is_editor_split(&views));
        assert!(doc_views(&views, PaneKind::CENTER_RIGHT).is_empty());

        // 保存・読み込みを経ても、分割は残る
        let yaml = export_workspace_to_yaml_str(&AppState::default(), &views).expect("export");
        assert!(yaml.contains("center_right"));
        let (_state, loaded) = import_workspace_from_yaml_str(&yaml).expect("import");
        assert!(is_editor_split(&loaded));
        assert!(doc_views(&loaded, PaneKind::CENTER_RIGHT).is_empty());

        set_editor_split(&mut views, false);
        assert!(!is_editor_split(&views));
    }

    #[test]
    fn test_workspace_yaml_has_no_document_tabs() {
        for split in [false, true] {
            let mut views = default_pane_views();
            set_editor_split(&mut views, split);
            add_document_tab(&mut views, D1, true);
            add_document_tab(&mut views, D2, true);

            let yaml = export_workspace_to_yaml_str(&AppState::default(), &views).expect("export");
            assert!(
                !yaml.contains("document"),
                "documents must not be saved:\n{yaml}"
            );
            assert_eq!(yaml.contains("center_right"), split);

            let (_state, loaded) = import_workspace_from_yaml_str(&yaml).expect("import");
            assert_eq!(is_editor_split(&loaded), split);
            assert!(
                loaded
                    .values()
                    .all(|s| s.views.iter().all(|v| !matches!(v, ViewId::Document(_))))
            );
            // 保存用の複製だけが変わり、実行中の構成は変わらない
            assert_eq!(
                doc_views(
                    &views,
                    if split {
                        PaneKind::CENTER_RIGHT
                    } else {
                        PaneKind::CENTER
                    }
                )
                .iter()
                .filter(|v| matches!(v, ViewId::Document(_)))
                .count(),
                2
            );
        }
    }

    #[test]
    fn test_loading_yaml_with_document_tabs_strips_them() {
        // ドキュメントのタブを含む YAML（手書きなど）を読んでも、タブは取り除かれる
        let mut views = default_pane_views();
        set_editor_split(&mut views, true);
        add_document_tab(&mut views, D1, true);
        let raw = serde_yaml::to_string(&SavedWorkspaceState {
            app_state: AppState::default(),
            pane_views: views,
            editor_layout: None,
        })
        .expect("serialize");
        assert!(raw.contains("document"));
        let (_state, loaded) = import_workspace_from_yaml_str(&raw).expect("import");
        assert!(is_editor_split(&loaded));
        assert!(doc_views(&loaded, PaneKind::CENTER_RIGHT).is_empty());
    }

    #[test]
    fn test_legacy_text_editor_tab_is_dropped_on_load() {
        // 旧形式（2026.10.2 まで）: テキストエディタが Bottom ペインの最後のタブ
        let mut legacy = default_pane_views();
        legacy.insert(
            PaneKind::Bottom,
            PaneViewState::new(
                vec![
                    ViewId::LogViewer,
                    ViewId::PythonConsole,
                    ViewId::Terminal,
                    ViewId::AiChat,
                    ViewId::LegacyTextEditor,
                ],
                4,
            ),
        );
        let yaml = serde_yaml::to_string(&SavedWorkspaceState {
            app_state: AppState::default(),
            pane_views: legacy,
            editor_layout: None,
        })
        .expect("serialize");
        assert!(yaml.contains("text_editor"));

        let (_state, views) = import_workspace_from_yaml_str(&yaml).expect("import");
        let bottom = views.get(&PaneKind::Bottom).unwrap();
        assert_eq!(
            bottom.views,
            vec![
                ViewId::LogViewer,
                ViewId::PythonConsole,
                ViewId::Terminal,
                ViewId::AiChat,
            ]
        );
        assert!(bottom.active_index < bottom.views.len());
        assert_eq!(doc_views(&views, PaneKind::CENTER), vec![ViewId::Viewport]);
        assert!(!is_editor_split(&views));
    }

    #[test]
    fn test_legacy_split_with_text_editor_stays_split_without_it() {
        // 2026.10.2 で分割していたワークスペース: 右側が [text_editor]。分割は保ち、旧エディタのタブは外す
        let mut legacy = default_pane_views();
        legacy.insert(
            PaneKind::CENTER_RIGHT,
            PaneViewState::new(vec![ViewId::LegacyTextEditor], 0),
        );
        let yaml = serde_yaml::to_string(&SavedWorkspaceState {
            app_state: AppState::default(),
            pane_views: legacy,
            editor_layout: None,
        })
        .expect("serialize");
        let (_state, views) = import_workspace_from_yaml_str(&yaml).expect("import");
        assert!(is_editor_split(&views));
        assert!(doc_views(&views, PaneKind::CENTER_RIGHT).is_empty());
    }

    #[test]
    fn test_attach_documents_after_replacing_layout() {
        // ワークスペースを読み込んで構成を差し替えたあとも、開いているファイルのタブを並べ直せる
        let mut old = default_pane_views();
        add_document_tab(&mut old, D1, true);
        add_document_tab(&mut old, D2, true);
        let prev = active_document(&old);
        assert_eq!(prev, Some(D2));

        for split in [false, true] {
            let mut fresh = default_pane_views();
            set_editor_split(&mut fresh, split);
            attach_documents(&mut fresh, &[D1, D2], prev);
            let target = if split {
                PaneKind::CENTER_RIGHT
            } else {
                PaneKind::CENTER
            };
            let docs: Vec<_> = doc_views(&fresh, target)
                .into_iter()
                .filter(|v| matches!(v, ViewId::Document(_)))
                .collect();
            assert_eq!(docs, vec![ViewId::Document(D1), ViewId::Document(D2)]);
            assert_eq!(active_document(&fresh), Some(D2));
        }
    }

    #[test]
    fn test_remove_view_keeps_active_view() {
        let mut state = PaneViewState::new(
            vec![ViewId::LogViewer, ViewId::Document(D1), ViewId::Terminal],
            2,
        );
        assert!(state.remove_view(ViewId::Document(D1)));
        assert_eq!(state.views, vec![ViewId::LogViewer, ViewId::Terminal]);
        assert_eq!(state.active_view(), Some(ViewId::Terminal));
        assert!(!state.remove_view(ViewId::Document(D1)));
    }

    #[test]
    fn test_move_view_to_reorders_and_keeps_the_active_view() {
        let d = |n| ViewId::Document(DocumentId(n));
        let mut s = PaneViewState::new(vec![ViewId::Viewport, d(1), d(2), d(3)], 1);
        // 右へ: 1 を 3 の位置へ
        assert!(s.move_view_to(d(1), d(3)));
        assert_eq!(s.views, vec![ViewId::Viewport, d(2), d(3), d(1)]);
        assert_eq!(s.active_view(), Some(d(1)));
        // 左へ: 3 を先頭（Viewport の位置）へ
        assert!(s.move_view_to(d(3), ViewId::Viewport));
        assert_eq!(s.views, vec![d(3), ViewId::Viewport, d(2), d(1)]);
        assert_eq!(s.active_view(), Some(d(1)));
        // 同じタブ、ないタブは、何もしない
        assert!(!s.move_view_to(d(2), d(2)));
        assert!(!s.move_view_to(d(9), d(2)));
        assert!(!s.move_view_to(d(2), d(9)));
        assert_eq!(s.views, vec![d(3), ViewId::Viewport, d(2), d(1)]);
    }

    #[test]
    fn test_move_view_between_groups() {
        let (c, r) = (PaneKind::CENTER, PaneKind::CENTER_RIGHT);
        let d = |n| ViewId::Document(DocumentId(n));
        let mut views = default_pane_views();
        set_editor_split(&mut views, true);
        add_document_tab(&mut views, DocumentId(1), true);
        add_document_tab(&mut views, DocumentId(2), true);
        assert_eq!(views[&r].views, vec![ViewId::Viewport2, d(1), d(2)]);
        // 3D Viewport を、右の 2 つ目の分子図の位置へ。移したタブが選ばれる
        assert!(views.move_view_between(ViewId::Viewport, c, r, Some(ViewId::Viewport2)));
        assert!(views[&c].views.is_empty());
        assert_eq!(
            views[&r].views,
            vec![ViewId::Viewport, ViewId::Viewport2, d(1), d(2)]
        );
        assert_eq!(views[&r].active_view(), Some(ViewId::Viewport));
        // 元のグループでは、選んでいたタブを保つ。行き先を指定しなければ末尾へ
        views.get_mut(&r).unwrap().select_view(d(2));
        assert!(views.move_view_between(d(1), r, c, None));
        assert_eq!(views[&r].active_view(), Some(d(2)));
        assert_eq!(views[&c].views, vec![d(1)]);
        // 同じグループ、ないタブ、ないグループへは移さない
        assert!(!views.move_view_between(d(2), r, r, None));
        assert!(!views.move_view_between(d(9), r, c, None));
        assert!(!views.move_view_between(d(2), r, PaneKind::Group(GroupId(7)), None));
        assert_eq!(views[&c].views, vec![d(1)]);
    }

    #[test]
    fn test_unsplit_keeps_a_viewport_moved_to_the_right() {
        let mut views = default_pane_views();
        set_editor_split(&mut views, true);
        add_document_tab(&mut views, DocumentId(1), true);
        views.move_view_between(
            ViewId::Viewport,
            PaneKind::CENTER,
            PaneKind::CENTER_RIGHT,
            None,
        );
        set_editor_split(&mut views, false);
        assert_eq!(views.groups(), vec![PaneKind::CENTER]);
        assert_eq!(
            views[&PaneKind::CENTER].views,
            vec![ViewId::Viewport, ViewId::Document(DocumentId(1))]
        );
    }

    #[test]
    fn test_saved_layout_keeps_moved_viewports_without_duplicates() {
        let mut views = default_pane_views();
        set_editor_split(&mut views, true);
        // 2 つの分子図を入れ替える
        views.move_view_between(
            ViewId::Viewport,
            PaneKind::CENTER,
            PaneKind::CENTER_RIGHT,
            None,
        );
        views.move_view_between(
            ViewId::Viewport2,
            PaneKind::CENTER_RIGHT,
            PaneKind::CENTER,
            None,
        );
        let yaml = export_workspace_to_yaml_str(&AppState::default(), &views).expect("export");
        let (_, loaded) = import_workspace_from_yaml_str(&yaml).expect("import");
        assert_eq!(loaded[&PaneKind::CENTER].views, vec![ViewId::Viewport2]);
        assert_eq!(
            loaded[&PaneKind::CENTER_RIGHT].views,
            vec![ViewId::Viewport]
        );

        // 同じタブが 2 つある、手で書いた構成は、前のものだけ残す。どこにもなければ、元の場所に足す
        let mut views = default_pane_views();
        set_editor_split(&mut views, true);
        views.get_mut(&PaneKind::CENTER_RIGHT).unwrap().views = vec![ViewId::Viewport];
        normalize_pane_views(&mut views);
        assert_eq!(views[&PaneKind::CENTER].views, vec![ViewId::Viewport]);
        assert_eq!(
            views[&PaneKind::CENTER_RIGHT].views,
            vec![ViewId::Viewport2]
        );
    }

    #[test]
    fn test_group_tree_split_and_remove() {
        let g = |n| GroupNode::Group(GroupId(n));
        let mut tree = g(0);
        assert!(tree.split_leaf(GroupId(0), GroupId(1), DropZone::Right));
        assert!(tree.split_leaf(GroupId(1), GroupId(2), DropZone::Top));
        assert!(!tree.split_leaf(GroupId(9), GroupId(3), DropZone::Left));
        assert_eq!(tree.groups(), vec![GroupId(0), GroupId(2), GroupId(1)]);
        let GroupNode::Split { axis, b, .. } = &tree else {
            panic!("split")
        };
        assert_eq!(*axis, Axis::Vertical);
        assert!(matches!(
            **b,
            GroupNode::Split {
                axis: Axis::Horizontal,
                ..
            }
        ));
        // 葉を取り除くと、親の分割は、もう一方の子になる
        assert!(tree.remove_leaf(GroupId(2)));
        assert_eq!(
            tree,
            GroupNode::Split {
                axis: Axis::Vertical,
                ratio: 0.5,
                a: Box::new(g(0)),
                b: Box::new(g(1)),
            }
        );
        assert!(tree.remove_leaf(GroupId(0)));
        assert_eq!(tree, g(1));
        // 最後の 1 つは取り除かない
        assert!(!tree.remove_leaf(GroupId(1)));
    }

    #[test]
    fn test_split_with_view_and_prune() {
        let mut views = default_pane_views();
        add_document_tab(&mut views, D1, true);
        // 同じグループの右の端: 新しいグループ 1 へ
        let new = views
            .split_with_view(
                ViewId::Document(D1),
                PaneKind::CENTER,
                PaneKind::CENTER,
                DropZone::Right,
            )
            .expect("split");
        assert_eq!(new, PaneKind::CENTER_RIGHT);
        assert_eq!(views[&new].views, vec![ViewId::Document(D1)]);
        assert_eq!(views[&PaneKind::CENTER].views, vec![ViewId::Viewport]);
        // 唯一のタブ、中央、ドック、ないタブは分割しない
        let none = |views: &mut PaneLayout, view, from, to, zone| {
            views.split_with_view(view, from, to, zone).is_none()
        };
        assert!(none(
            &mut views,
            ViewId::Document(D1),
            new,
            new,
            DropZone::Left
        ));
        assert!(none(
            &mut views,
            ViewId::Viewport,
            PaneKind::CENTER,
            new,
            DropZone::Center
        ));
        // ドックの中は分割しない
        assert!(none(
            &mut views,
            ViewId::Document(D1),
            new,
            PaneKind::Bottom,
            DropZone::Left
        ));
        assert!(none(
            &mut views,
            ViewId::Document(D2),
            new,
            PaneKind::CENTER,
            DropZone::Left
        ));
        // ほかのグループの端へ移すと、元は空になる。空のグループは取り除く（最後の 1 つは残す）
        let top = views
            .split_with_view(ViewId::Viewport, PaneKind::CENTER, new, DropZone::Top)
            .expect("split");
        assert_eq!(top, PaneKind::Group(GroupId(2)));
        assert!(views[&PaneKind::CENTER].views.is_empty());
        assert!(views.prune_empty_groups());
        assert_eq!(views.groups(), vec![top, new]);
        assert!(!views.contains_key(&PaneKind::CENTER));
        views.get_mut(&top).unwrap().views.clear();
        views.get_mut(&new).unwrap().views.clear();
        views.prune_empty_groups();
        assert_eq!(views.groups().len(), 1);
    }

    #[test]
    fn test_split_editor_right_merges_every_group_when_turned_off() {
        let mut views = default_pane_views();
        add_document_tab(&mut views, D1, true);
        add_document_tab(&mut views, D2, true);
        views.split_with_view(
            ViewId::Document(D1),
            PaneKind::CENTER,
            PaneKind::CENTER,
            DropZone::Bottom,
        );
        assert!(is_editor_split(&views));
        // すでに分割しているときに「オン」にしても、何もしない
        let before = views.clone();
        set_editor_split(&mut views, true);
        assert_eq!(views, before);
        // オフにすると、1 つのグループにまとまる（ドキュメントは末尾）
        set_editor_split(&mut views, false);
        assert_eq!(views.groups(), vec![PaneKind::CENTER]);
        assert_eq!(
            views[&PaneKind::CENTER].views,
            vec![ViewId::Viewport, ViewId::Document(D2), ViewId::Document(D1)]
        );
        assert!(!has_second_viewport(&views));
    }

    /// 左に 3D Viewport と、その下に 3D Viewport 2 を置いた、上下の分割（割合 0.3）
    fn stacked_viewports() -> PaneLayout {
        let mut views = default_pane_views();
        set_editor_split(&mut views, true);
        views.split_with_view(
            ViewId::Viewport2,
            PaneKind::CENTER_RIGHT,
            PaneKind::CENTER,
            DropZone::Bottom,
        );
        views.prune_empty_groups();
        views.sync_ratios(&|_, _| Some(0.3));
        views
    }

    #[test]
    fn test_editor_layout_is_saved_and_restored() {
        let views = stacked_viewports();
        let lower = PaneKind::Group(GroupId(2));
        assert_eq!(views.groups(), vec![PaneKind::CENTER, lower]);
        let yaml = export_workspace_to_yaml_str(&AppState::default(), &views).expect("export");
        assert!(yaml.contains("editor_layout:"), "{yaml}");
        assert!(yaml.contains("stacked"), "{yaml}");
        let (_, loaded) = import_workspace_from_yaml_str(&yaml).expect("import");
        assert_eq!(loaded.editor(), views.editor());
        assert_eq!(loaded[&lower].views, vec![ViewId::Viewport2]);

        // 分割していなければ、書かない（以前と同じ YAML）
        let yaml = export_workspace_to_yaml_str(&AppState::default(), &default_pane_views())
            .expect("export");
        assert!(!yaml.contains("editor_layout"));
    }

    #[test]
    fn test_saved_editor_layout_that_does_not_fit_is_repaired_or_ignored() {
        let views = stacked_viewports();
        let yaml = export_workspace_to_yaml_str(&AppState::default(), &views).expect("export");
        // 木にないグループは右端に足す。木にあってタブの並びのないグループは、空なので取り除く
        assert!(yaml.contains("  group_2:\n"), "{yaml}");
        assert!(yaml.contains("    group: group_2\n"), "{yaml}");
        let edited = yaml
            .replace("  group_2:\n", "  group_7:\n")
            .replace("    group: group_2\n", "    group: group_9\n");
        let (_, loaded) = import_workspace_from_yaml_str(&edited).expect("import");
        assert_eq!(
            loaded.groups(),
            vec![PaneKind::CENTER, PaneKind::Group(GroupId(7))]
        );
        // グループでないキー、同じグループの重複、範囲外の割合は、木を使わない（番号の順に左右に並べる）
        for bad in [
            yaml.replace("    group: group_2\n", "    group: left\n"),
            yaml.replace("    group: group_2\n", "    group: center\n"),
            yaml.replace("ratio: 0.3", "ratio: 1.5"),
        ] {
            let (_, loaded) = import_workspace_from_yaml_str(&bad).expect("import");
            assert!(
                matches!(
                    loaded.editor(),
                    GroupNode::Split {
                        axis: Axis::Vertical,
                        ..
                    }
                ),
                "{bad}"
            );
            assert_eq!(loaded.groups().len(), 2);
        }
    }

    #[test]
    fn test_tabs_move_between_docks_and_editor_groups() {
        let mut views = default_pane_views();
        add_document_tab(&mut views, D1, true);
        // 下のパネルの Terminal を、エディタの 3D Viewport の位置へ
        assert!(views.move_view_between(
            ViewId::Terminal,
            PaneKind::Bottom,
            PaneKind::CENTER,
            Some(ViewId::Viewport)
        ));
        assert_eq!(
            views[&PaneKind::CENTER].views,
            vec![ViewId::Terminal, ViewId::Viewport, ViewId::Document(D1)]
        );
        assert_eq!(
            views[&PaneKind::CENTER].active_view(),
            Some(ViewId::Terminal)
        );
        assert!(!views[&PaneKind::Bottom].views.contains(&ViewId::Terminal));
        // 3D Viewport を、右のパネルへ。ドキュメントを、下のパネルへ
        assert!(views.move_view_between(ViewId::Viewport, PaneKind::CENTER, PaneKind::Right, None));
        assert!(views.move_view_between(
            ViewId::Document(D1),
            PaneKind::CENTER,
            PaneKind::Bottom,
            None
        ));
        assert_eq!(
            views[&PaneKind::Right].views,
            vec![ViewId::RepresentationInspector, ViewId::Viewport]
        );
        // ドックにあるドキュメントも、選んでいるドキュメントとして扱う
        assert_eq!(active_document(&views), Some(D1));
        // ドックのタブを、エディタの端へ落として分割する
        let new = views
            .split_with_view(
                ViewId::LogViewer,
                PaneKind::Bottom,
                PaneKind::CENTER,
                DropZone::Right,
            )
            .expect("split");
        assert_eq!(views[&new].views, vec![ViewId::LogViewer]);
        // ドックは、空になっても残る（取り除くのは、エディタのグループだけ）
        let left: Vec<ViewId> = views[&PaneKind::Left].views.clone();
        for v in left {
            assert!(views.move_view_between(v, PaneKind::Left, PaneKind::CENTER, None));
        }
        views.prune_empty_groups();
        assert!(views[&PaneKind::Left].views.is_empty());
        assert!(views.contains_key(&PaneKind::Left));

        // 保存して読み込んでも、置き場所は変わらない（ドキュメントは保存しない）。3D Viewport は二重にならない
        let yaml = export_workspace_to_yaml_str(&AppState::default(), &views).expect("export");
        let (_, loaded) = import_workspace_from_yaml_str(&yaml).expect("import");
        assert_eq!(
            loaded[&PaneKind::Right].views,
            vec![ViewId::RepresentationInspector, ViewId::Viewport]
        );
        assert!(!loaded[&PaneKind::CENTER].views.contains(&ViewId::Viewport));
        assert!(loaded[&PaneKind::Left].views.is_empty());
        assert!(
            loaded[&PaneKind::Bottom]
                .views
                .iter()
                .all(|v| !matches!(v, ViewId::Document(_)))
        );

        // 分割メニューは、ドックにあるドキュメントを動かさない
        let mut views = default_pane_views();
        add_document_tab(&mut views, D1, true);
        views.move_view_between(
            ViewId::Document(D1),
            PaneKind::CENTER,
            PaneKind::Bottom,
            None,
        );
        set_editor_split(&mut views, true);
        assert!(
            views[&PaneKind::Bottom]
                .views
                .contains(&ViewId::Document(D1))
        );
        set_editor_split(&mut views, false);
        assert!(
            views[&PaneKind::Bottom]
                .views
                .contains(&ViewId::Document(D1))
        );
    }
}
