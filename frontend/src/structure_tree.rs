//! 構造/ファイルツリー（Leftペイン）ウィジェット (ROADMAP Phase 6d-iv)
//!
//! 8.3節「コンテキストメニュー」および 8.4節「アクションパイプラインの原則」に準拠。
//! `AtomGroup`（Model → Chain → Residue → Atom）の階層構造をツリー表示し、
//! 各ノードの展開/折りたたみ、選択（3Dビューポートへのハイライト反映）、
//! およびコンテキストメニュー操作を提供する。

use crate::ui_style::palette;
use iced::widget::{Space, button, column, container, mouse_area, row, scrollable, text};
use iced::{Alignment, Border, Element, Length, Padding};
use std::collections::{BTreeSet, HashMap};
use yui_core::atom_group::{
    AtomGroup, SecondaryStructureKind, SecondaryStructureMap, iter_atoms, iter_chains,
    iter_flat_atoms, iter_models, iter_residues, parse_residue_key,
};
use yui_core::selector::Selector;
use yui_core::{ColorScheme, RepresentationKind};

/// ツリーノードのコンテキストターゲット
#[derive(Debug, Clone, PartialEq)]
pub struct TreeContextTarget {
    pub key: String,
    pub label: String,
    pub selector: Selector,
}

/// ツリーの状態管理構造体
#[derive(Debug, Clone, Default)]
pub struct StructureTreeState {
    /// 展開されているノードのキー集合（既定は空＝すべて折りたたみ）
    pub expanded_nodes: BTreeSet<String>,
    /// 現在選択されているノードキー
    pub selected_node: Option<String>,
}

impl StructureTreeState {
    /// 新規ツリー状態を作成（既定はすべて折りたたみ）
    pub fn new() -> Self {
        Self {
            expanded_nodes: BTreeSet::new(),
            selected_node: None,
        }
    }

    /// ノードが展開されているかどうか
    pub fn is_expanded(&self, key: &str) -> bool {
        self.expanded_nodes.contains(key)
    }

    /// ノードの展開/折りたたみを切り替える
    pub fn toggle_expanded(&mut self, key: &str) {
        if self.expanded_nodes.contains(key) {
            self.expanded_nodes.remove(key);
        } else {
            self.expanded_nodes.insert(key.to_string());
        }
    }

    /// 指定ノードを展開する
    #[allow(dead_code)]
    pub fn expand(&mut self, key: &str) {
        self.expanded_nodes.insert(key.to_string());
    }

    /// 指定ノードを折りたたむ
    #[allow(dead_code)]
    pub fn collapse(&mut self, key: &str) {
        self.expanded_nodes.remove(key);
    }

    /// 選択中のノードを設定する
    pub fn select(&mut self, key: Option<String>) {
        self.selected_node = key;
    }
}

/// ツリーから発行されるアクション
#[derive(Debug, Clone, PartialEq)]
pub enum StructureTreeAction {
    /// 展開/折りたたみのトグル
    ToggleNode(String),
    /// ノードの左クリック選択（ハイライト表示）
    SelectNode {
        key: String,
        label: String,
        selector: Selector,
    },
    /// ノードの右クリック（コンテキストメニュー表示要求）
    OpenContextMenu { target: TreeContextTarget },
    /// 表示対象モデルの選択切り替え（None は全モデル表示）
    SelectModel(Option<String>),
}

/// ツリーのコンテキストメニューから発行されるアクション
#[derive(Debug, Clone, PartialEq)]
pub enum TreeContextMenuAction {
    /// 選択 & ハイライト
    Select,
    /// 非表示
    Hide,
    /// 表現形式変更
    ShowAs(RepresentationKind),
    /// 配色スキーム変更
    ColorBy(ColorScheme),
    /// 名前付き選択として保存
    SaveAsNamedSelection,
    /// メニューを閉じる
    Close,
}

/// 構造ツリーの描画純粋関数
pub fn view_structure_tree<'a, Message: Clone + 'a>(
    state: &'a StructureTreeState,
    atom_group: Option<&'a AtomGroup>,
    sec_map: &'a SecondaryStructureMap,
    active_model: Option<&'a str>,
    on_action: impl Fn(StructureTreeAction) -> Message + Copy + 'a,
) -> Element<'a, Message> {
    let group = match atom_group {
        Some(g) if g.get_number_of_groups() > 0 => g,
        _ => {
            return container(
                column![
                    text("Structure Tree")
                        .size(13)
                        .color(palette().text_secondary),
                    Space::new().height(Length::Fixed(12.0)),
                    text("No structure loaded")
                        .size(12)
                        .color(palette().text_subtle),
                    text("Load a peptide or structure to explore")
                        .size(11)
                        .color(palette().text_faint),
                ]
                .spacing(4)
                .align_x(Alignment::Center),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .padding(Padding::from(16))
            .into();
        }
    };

    let model_keys: Vec<&str> = iter_models(group).map(|(k, _)| k).collect();
    let models_count = model_keys.len();
    let mut tree_rows = column![
        row![
            text(&group.name)
                .size(12)
                .color(palette().text_secondary)
                .width(Length::Fill),
            text(format!("{} models", models_count))
                .size(10)
                .color(palette().text_faint)
                .wrapping(iced::widget::text::Wrapping::None),
        ]
        .spacing(8)
        .padding(Padding::from([4, 6]))
        .align_y(Alignment::Center)
    ]
    .spacing(2);

    // マルチモデル構造（モデル数 > 1）の場合、ツリー上部にモデル切り替えバーを表示
    if models_count > 1 {
        let current_idx = active_model.and_then(|act| model_keys.iter().position(|&k| k == act));
        let model_label = match (active_model, current_idx) {
            (Some(act), Some(idx)) => format!("{act} ({}/{models_count})", idx + 1),
            (Some(act), None) => format!("{act} (?/{models_count})"),
            (None, _) => format!("All ({models_count})"),
        };

        let prev_model = match current_idx {
            Some(idx) => {
                if idx > 0 {
                    Some(model_keys[idx - 1].to_string())
                } else {
                    Some(model_keys[models_count - 1].to_string())
                }
            }
            None => Some(model_keys[models_count - 1].to_string()),
        };

        let next_model = match current_idx {
            Some(idx) => {
                if idx + 1 < models_count {
                    Some(model_keys[idx + 1].to_string())
                } else {
                    Some(model_keys[0].to_string())
                }
            }
            None => Some(model_keys[0].to_string()),
        };

        let prev_btn = button(text("◀").size(10))
            .padding(Padding::from([2, 5]))
            .style(crate::ui_style::secondary_button)
            .on_press(on_action(StructureTreeAction::SelectModel(prev_model)));

        let next_btn = button(text("▶").size(10))
            .padding(Padding::from([2, 5]))
            .style(crate::ui_style::secondary_button)
            .on_press(on_action(StructureTreeAction::SelectModel(next_model)));

        let all_btn = button(text("All").size(10))
            .padding(Padding::from([2, 5]))
            .style(crate::ui_style::secondary_button)
            .on_press(on_action(StructureTreeAction::SelectModel(None)));

        let model_selector_row = row![
            text("Model:").size(11).color(palette().text_secondary),
            prev_btn,
            text(model_label).size(11).color(palette().text),
            next_btn,
            all_btn,
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .padding(Padding::from([2, 6]));

        tree_rows = tree_rows.push(model_selector_row);
    }

    let atom_path_to_id: HashMap<String, u32> = iter_flat_atoms(group)
        .map(|fa| (fa.atom.path.clone(), fa.id))
        .collect();

    for (model_key, model) in iter_models(group) {
        let m_key = format!("m:{}", model_key);
        let m_is_expanded = state.is_expanded(&m_key);
        let m_is_selected = state.selected_node.as_deref() == Some(&m_key);
        let m_label = format!("Model {}", model_key);
        let m_selector = Selector::PathWildcard(format!("{}/*/*/*", model_key));

        let chain_count = iter_chains(model).count();
        tree_rows = tree_rows.push(render_tree_row(
            0,
            &m_key,
            &m_label,
            format!("({} chains)", chain_count),
            m_is_expanded,
            m_is_selected,
            false,
            m_selector.clone(),
            on_action,
        ));

        if m_is_expanded {
            for (chain_key, chain) in iter_chains(model) {
                let c_key = format!("c:{}:{}", model_key, chain_key);
                let c_is_expanded = state.is_expanded(&c_key);
                let c_is_selected = state.selected_node.as_deref() == Some(&c_key);
                let c_label = format!("Chain {}", chain_key);
                let c_selector = Selector::ResidueRange {
                    chain: Some(chain_key.to_string()),
                    start: i32::MIN,
                    end: i32::MAX,
                };

                let res_count = iter_residues(chain).count();
                tree_rows = tree_rows.push(render_tree_row(
                    1,
                    &c_key,
                    &c_label,
                    format!("({} res)", res_count),
                    c_is_expanded,
                    c_is_selected,
                    false,
                    c_selector.clone(),
                    on_action,
                ));

                if c_is_expanded {
                    for (res_key, residue) in iter_residues(chain) {
                        let (res_num, _ins_code) = parse_residue_key(res_key);
                        let r_key = format!("r:{}:{}:{}", model_key, chain_key, res_key);
                        let r_is_expanded = state.is_expanded(&r_key);
                        let r_is_selected = state.selected_node.as_deref() == Some(&r_key);
                        let r_label = format!("{}:{}", residue.name, res_num);
                        let r_selector = Selector::ResidueRange {
                            chain: Some(chain_key.to_string()),
                            start: res_num,
                            end: res_num,
                        };

                        let res_path = format!("/{model_key}/{chain_key}/{res_key}/");
                        let ss_badge = match sec_map.get(&res_path).copied() {
                            Some(SecondaryStructureKind::Helix) => " [Helix]",
                            Some(SecondaryStructureKind::Sheet) => " [Sheet]",
                            Some(SecondaryStructureKind::Loop) => " [Loop]",
                            None => "",
                        };

                        let atom_count = residue.get_number_of_atoms();
                        tree_rows = tree_rows.push(render_tree_row(
                            2,
                            &r_key,
                            &r_label,
                            format!("({} atoms){ss_badge}", atom_count),
                            r_is_expanded,
                            r_is_selected,
                            false,
                            r_selector.clone(),
                            on_action,
                        ));

                        if r_is_expanded {
                            for (_atom_name, atom) in iter_atoms(residue) {
                                let atom_id = atom_path_to_id.get(&atom.path).copied().unwrap_or(0);
                                let a_key = format!(
                                    "a:{}:{}:{}:{}",
                                    model_key, chain_key, res_key, atom_id
                                );
                                let a_is_selected = state.selected_node.as_deref() == Some(&a_key);
                                let a_label = format!("{} (#{})", atom.name, atom_id);
                                let a_selector = Selector::AtomIds(vec![atom_id]);

                                tree_rows = tree_rows.push(render_tree_row(
                                    3,
                                    &a_key,
                                    &a_label,
                                    atom.symbol().unwrap_or("X").to_string(),
                                    false,
                                    a_is_selected,
                                    true,
                                    a_selector,
                                    on_action,
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    scrollable(tree_rows.padding(Padding::from([4, 6])))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// 単一ツリー行の描画ヘルパー
#[allow(clippy::too_many_arguments)]
fn render_tree_row<'a, Message: Clone + 'a>(
    indent_level: usize,
    key: &str,
    label: &str,
    detail: String,
    is_expanded: bool,
    is_selected: bool,
    is_leaf: bool,
    selector: Selector,
    on_action: impl Fn(StructureTreeAction) -> Message + Copy + 'a,
) -> Element<'a, Message> {
    let indent_space = Space::new().width(Length::Fixed((indent_level * 12) as f32));

    let expand_btn: Element<'a, Message> = if is_leaf {
        container(text("•").size(10).color(palette().text_faint))
            .width(Length::Fixed(16.0))
            .center_x(Length::Fixed(16.0))
            .into()
    } else {
        let icon = if is_expanded { "▼" } else { "▶" };
        button(text(icon).size(9).color(palette().text_subtle))
            .width(Length::Fixed(16.0))
            .padding(Padding::from([1, 2]))
            .style(|_theme, status| match status {
                button::Status::Hovered => button::Style {
                    background: Some(palette().item_hover_bg.into()),
                    text_color: palette().text,
                    border: Border::default().rounded(2),
                    ..button::Style::default()
                },
                _ => button::Style {
                    background: None,
                    text_color: palette().text_subtle,
                    ..button::Style::default()
                },
            })
            .on_press(on_action(StructureTreeAction::ToggleNode(key.to_string())))
            .into()
    };

    let target = TreeContextTarget {
        key: key.to_string(),
        label: label.to_string(),
        selector: selector.clone(),
    };

    let label_color = if is_selected {
        palette().highlight_text
    } else {
        palette().text_value
    };

    let label_row = row![
        text(label.to_string()).size(11).color(label_color),
        Space::new().width(Length::Fixed(4.0)),
        text(detail).size(10).color(palette().text_faint),
    ]
    .align_y(Alignment::Center);

    let clickable_area = mouse_area(
        container(label_row)
            .width(Length::Fill)
            .padding(Padding::from([2, 4])),
    )
    .on_press(on_action(StructureTreeAction::SelectNode {
        key: key.to_string(),
        label: label.to_string(),
        selector,
    }))
    .on_right_press(on_action(StructureTreeAction::OpenContextMenu { target }));

    let row_bg = if is_selected {
        Some(palette().row_selected_bg.into())
    } else {
        None
    };

    container(
        row![indent_space, expand_btn, clickable_area]
            .align_y(Alignment::Center)
            .spacing(2)
            .width(Length::Fill),
    )
    .style(move |_theme| container::Style {
        background: row_bg,
        border: Border::default().rounded(3),
        ..container::Style::default()
    })
    .width(Length::Fill)
    .into()
}

/// ツリーノード用の右クリックコンテキストメニュー描画
pub fn view_tree_context_menu<'a, Message: Clone + 'a>(
    screen_pos: [f32; 2],
    items: &[crate::menu::MenuItem],
    window: iced::Size,
    on_command: impl Fn(crate::menu::CommandInvocation) -> Message + Copy + 'a,
    on_close: Message,
) -> Element<'a, Message> {
    // 共通のメニュー（スタイル・ウィンドウに収まる位置・スクロール）で描く
    crate::menu::popup_menu(
        items,
        crate::menu::Surface::Card,
        240.0,
        crate::menu::Anchor::Cursor(screen_pos),
        window,
        on_command,
        on_close,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use yui_core::{AtomGroup, AtomGroupExt};

    #[test]
    fn test_structure_tree_default_collapsed() {
        let state = StructureTreeState::new();
        assert!(
            state.expanded_nodes.is_empty(),
            "既定では全ノードが折りたたまれていること"
        );
        assert_eq!(state.selected_node, None);
        assert!(!state.is_expanded("m:1"));
        assert!(!state.is_expanded("c:1:A"));
    }

    #[test]
    fn test_structure_tree_toggle_expanded() {
        let mut state = StructureTreeState::new();
        state.toggle_expanded("m:1");
        assert!(state.is_expanded("m:1"));

        state.toggle_expanded("m:1");
        assert!(!state.is_expanded("m:1"));

        state.expand("c:1:A");
        assert!(state.is_expanded("c:1:A"));
        state.collapse("c:1:A");
        assert!(!state.is_expanded("c:1:A"));
    }

    #[test]
    fn test_structure_tree_node_selectors() {
        // Model selector
        let model_sel = Selector::PathWildcard(format!("{}/*/*/*", "1"));
        match model_sel {
            Selector::PathWildcard(ref s) => assert_eq!(s, "1/*/*/*"),
            _ => panic!("Expected PathWildcard"),
        }

        // Chain selector
        let chain_sel = Selector::ResidueRange {
            chain: Some("A".to_string()),
            start: i32::MIN,
            end: i32::MAX,
        };
        match chain_sel {
            Selector::ResidueRange {
                ref chain,
                start,
                end,
            } => {
                assert_eq!(chain.as_deref(), Some("A"));
                assert_eq!(start, i32::MIN);
                assert_eq!(end, i32::MAX);
            }
            _ => panic!("Expected ResidueRange"),
        }

        // Residue selector
        let res_sel = Selector::ResidueRange {
            chain: Some("A".to_string()),
            start: 5,
            end: 5,
        };
        match res_sel {
            Selector::ResidueRange {
                ref chain,
                start,
                end,
            } => {
                assert_eq!(chain.as_deref(), Some("A"));
                assert_eq!(start, 5);
                assert_eq!(end, 5);
            }
            _ => panic!("Expected ResidueRange"),
        }

        // Atom selector
        let atom_sel = Selector::AtomIds(vec![42]);
        match atom_sel {
            Selector::AtomIds(ref ids) => assert_eq!(ids, &[42]),
            _ => panic!("Expected AtomIds"),
        }
    }

    #[test]
    fn test_structure_tree_empty_state() {
        let state = StructureTreeState::new();
        let sec_map = SecondaryStructureMap::new();
        // None の場合
        let _elem: Element<'_, StructureTreeAction> =
            view_structure_tree(&state, None, &sec_map, None, |a| a);
        // 空の AtomGroup の場合
        let empty_group = AtomGroup::with_name("empty");
        let _elem2: Element<'_, StructureTreeAction> =
            view_structure_tree(&state, Some(&empty_group), &sec_map, None, |a| a);
    }

    #[test]
    fn test_structure_tree_rendering_nodes() {
        let mut state = StructureTreeState::new();
        let (fragment, sec_map) = AtomGroup::create_test_protein_fragment();

        // 折りたたみ状態での描画
        {
            let _elem: Element<'_, StructureTreeAction> =
                view_structure_tree(&state, Some(&fragment), &sec_map, None, |a| a);
        }

        // Model を展開
        state.expand("m:model_1");
        {
            let _elem: Element<'_, StructureTreeAction> =
                view_structure_tree(&state, Some(&fragment), &sec_map, Some("model_1"), |a| a);
        }

        // Chain を展開
        state.expand("c:model_1:A");
        {
            let _elem: Element<'_, StructureTreeAction> =
                view_structure_tree(&state, Some(&fragment), &sec_map, Some("model_1"), |a| a);
        }

        // Residue を展開
        state.expand("r:model_1:A:1");
        {
            let _elem: Element<'_, StructureTreeAction> =
                view_structure_tree(&state, Some(&fragment), &sec_map, Some("model_1"), |a| a);
        }
    }

    #[test]
    fn test_structure_tree_multi_model_selector() {
        use yui_core::structure_loader::parse_structure_bytes;

        let cif_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/tests/data/1HLS.cif");
        let bytes = std::fs::read(cif_path).expect("failed to read 1HLS.cif");
        let (group, sec_map, _) =
            parse_structure_bytes(&bytes, "1HLS.cif").expect("parse 1HLS.cif");

        let state = StructureTreeState::new();

        // 1. active_model が None の場合（All 表示）
        let _elem_all: Element<'_, StructureTreeAction> =
            view_structure_tree(&state, Some(&group), &sec_map, None, |a| a);

        // 2. active_model が "1" の場合
        let _elem_m1: Element<'_, StructureTreeAction> =
            view_structure_tree(&state, Some(&group), &sec_map, Some("1"), |a| a);
    }

    #[test]
    fn test_tree_context_menu_instantiation() {
        let target = TreeContextTarget {
            key: "c:1:A".to_string(),
            label: "Chain A".to_string(),
            selector: Selector::ResidueRange {
                chain: Some("A".to_string()),
                start: i32::MIN,
                end: i32::MAX,
            },
        };
        let items = crate::menu::builtin_items(
            crate::menu::location::CONTEXT_STRUCTURE_TREE,
            Some(crate::menu::MenuTarget::TreeNode(target)),
        );
        let _menu: Element<'_, TreeContextMenuAction> = view_tree_context_menu(
            [50.0, 100.0],
            &items,
            iced::Size::new(1024.0, 768.0),
            |_| TreeContextMenuAction::Close,
            TreeContextMenuAction::Close,
        );
    }
}
