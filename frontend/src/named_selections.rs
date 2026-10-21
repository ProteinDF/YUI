use crate::ui_style::palette;
use iced::widget::{Space, button, column, container, row, scrollable, text, text_input};
use iced::{Alignment, Border, Element, Length, Padding};
use std::collections::BTreeMap;
use yui_core::Selector;

/// 名前付き選択グループパネルから発行されるアクション
#[derive(Debug, Clone, PartialEq)]
pub enum NamedSelectionAction {
    /// 入力中のドラフトを更新するのみ（BTreeMapへは反映しない）
    RenameDraft { key: String, draft: String },
    /// Enter押下等で確定し、実際にBTreeMapのキーを書き換える
    RenameCommit { old_name: String },
    /// 削除
    Delete(String),
    /// 選択の適用（ハイライト用Representationの作成・更新）
    Apply(String),
}

/// 既存の名前付き選択マップと重複しない一意なキー名（`selection_1`, `selection_2`, ...）を生成する
pub fn generate_unique_selection_name(named_selections: &BTreeMap<String, Selector>) -> String {
    let mut n = 1;
    let mut name = format!("selection_{n}");
    while named_selections.contains_key(&name) {
        n += 1;
        name = format!("selection_{n}");
    }
    name
}

/// 名前付き選択グループ一覧（Leftペイン第2タブ）を描画する純粋関数
pub fn view_named_selections<'a, Message: Clone + 'a>(
    named_selections: &'a BTreeMap<String, Selector>,
    draft: Option<&'a (String, String)>,
    on_action: impl Fn(NamedSelectionAction) -> Message + Copy + 'a,
) -> Element<'a, Message> {
    // 1. ヘッダーツールバー
    let title = text("Named Selections").size(14).color(palette().text);
    let count_badge = container(
        text(format!("{}", named_selections.len()))
            .size(11)
            .color(palette().text_value),
    )
    .padding(Padding::from([2, 6]))
    .style(|_theme| container::Style {
        background: Some(palette().header_bg.into()),
        border: Border {
            radius: 10.0.into(),
            ..Border::default()
        },
        ..container::Style::default()
    });

    let header_row = row![title, Space::new().width(Length::Fixed(6.0)), count_badge]
        .align_y(Alignment::Center)
        .padding(Padding::from([4, 4]));

    // 2. リスト本体
    let body: Element<'a, Message> = if named_selections.is_empty() {
        container(
            column![
                text("No named selections yet.")
                    .size(12)
                    .color(palette().text_secondary),
                Space::new().height(Length::Fixed(4.0)),
                text("Right-click a structure tree node or 3D atom and choose \"Save as Named Selection\" to create one.")
                    .size(11)
                    .color(palette().text_subtle),
            ]
            .spacing(2),
        )
        .padding(Padding::from(12))
        .width(Length::Fill)
        .into()
    } else {
        let mut list_col = column![].spacing(6).width(Length::Fill);

        for (name, selector) in named_selections {
            let item_card = view_selection_card(name, selector, draft, on_action);
            list_col = list_col.push(item_card);
        }

        scrollable(list_col.padding(Padding::from([4, 2])))
            .height(Length::Fill)
            .into()
    };

    container(
        column![header_row, body]
            .spacing(6)
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .padding(Padding::from(8))
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// 個別の名前付き選択カードを描画
fn view_selection_card<'a, Message: Clone + 'a>(
    name: &'a str,
    selector: &'a Selector,
    draft: Option<&'a (String, String)>,
    on_action: impl Fn(NamedSelectionAction) -> Message + Copy + 'a,
) -> Element<'a, Message> {
    // 1. 名前入力・編集欄（編集中ドラフトがあればそちらを優先表示）
    let is_editing = draft.map(|(k, _)| k == name).unwrap_or(false);
    let display_val = if is_editing { &draft.unwrap().1 } else { name };

    let name_key = name.to_string();
    let name_for_commit = name.to_string();
    let name_input = text_input("Selection name...", display_val)
        .size(12)
        .padding(Padding::from([3, 6]))
        .on_input(move |new_val| {
            on_action(NamedSelectionAction::RenameDraft {
                key: name_key.clone(),
                draft: new_val,
            })
        })
        .on_submit(on_action(NamedSelectionAction::RenameCommit {
            old_name: name_for_commit,
        }));

    // 2. Apply ボタン（ハイライト）
    let apply_btn = button(text("Apply").size(11))
        .padding(Padding::from([3, 8]))
        .style(|_theme, status| {
            let bg = match status {
                button::Status::Hovered => palette().gold_hover,
                button::Status::Pressed => palette().gold_pressed,
                _ => palette().gold,
            };
            button::Style {
                background: Some(bg.into()),
                text_color: palette().on_gold,
                border: Border {
                    radius: 4.0.into(),
                    ..Border::default()
                },
                ..button::Style::default()
            }
        })
        .on_press(on_action(NamedSelectionAction::Apply(name.to_string())));

    // 3. 削除ボタン
    let delete_btn = button(text("×").size(12))
        .padding(Padding::from([2, 6]))
        .style(|_theme, status| {
            let bg = match status {
                button::Status::Hovered => palette().danger_btn_hover_bg,
                _ => palette().danger_btn_bg,
            };
            button::Style {
                background: Some(bg.into()),
                text_color: palette().danger_btn_text,
                border: Border {
                    radius: 4.0.into(),
                    ..Border::default()
                },
                ..button::Style::default()
            }
        })
        .on_press(on_action(NamedSelectionAction::Delete(name.to_string())));

    let top_row = row![
        name_input,
        Space::new().width(Length::Fixed(4.0)),
        apply_btn,
        Space::new().width(Length::Fixed(4.0)),
        delete_btn,
    ]
    .align_y(Alignment::Center);

    // 4. セレクタ要約表示
    let summary = format_selector_summary(selector);
    let summary_text = text(summary).size(10).color(palette().text_subtle);

    let card_content = column![top_row, summary_text]
        .spacing(4)
        .padding(Padding::from(6));

    container(card_content)
        .style(|_theme| container::Style {
            background: Some(palette().panel_bg.into()),
            border: Border {
                color: palette().panel_border,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..container::Style::default()
        })
        .width(Length::Fill)
        .into()
}

/// セレクタの要約ラベルを生成
fn format_selector_summary(selector: &Selector) -> String {
    match selector {
        Selector::All => "All atoms".to_string(),
        Selector::None => "No atoms".to_string(),
        Selector::AtomIds(ids) => {
            if ids.len() <= 3 {
                format!("Atom IDs: {:?}", ids)
            } else {
                format!("Atom IDs: {} atoms ({:?}...)", ids.len(), &ids[..3])
            }
        }
        Selector::Symbol(symbols) => format!("Element: {}", symbols.join(", ")),
        Selector::Name(names) => format!("Name: {}", names.join(", ")),
        Selector::ResidueName(names) => format!("Residue: {}", names.join(", ")),
        Selector::ResidueRange { chain, start, end } => {
            let ch = chain.as_deref().unwrap_or("*");
            format!("Range: /{ch}/{start}..{end}")
        }
        Selector::PathWildcard(p) => format!("Wildcard: {p}"),
        Selector::PathRegex(r) => format!("Regex: {r}"),
        Selector::And(subs) => format!("AND ({} rules)", subs.len()),
        Selector::Or(subs) => format!("OR ({} rules)", subs.len()),
        Selector::Not { inner } => format!("NOT ({})", format_selector_summary(inner)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_selector_summary() {
        assert_eq!(format_selector_summary(&Selector::All), "All atoms");
        assert_eq!(
            format_selector_summary(&Selector::AtomIds(vec![1, 2])),
            "Atom IDs: [1, 2]"
        );
        assert_eq!(
            format_selector_summary(&Selector::Not {
                inner: Box::new(Selector::Symbol(vec!["C".to_string()]))
            }),
            "NOT (Element: C)"
        );
    }

    #[test]
    fn test_generate_unique_selection_name() {
        let mut map = BTreeMap::new();
        assert_eq!(generate_unique_selection_name(&map), "selection_1");
        map.insert("selection_1".to_string(), Selector::All);
        assert_eq!(generate_unique_selection_name(&map), "selection_2");
        map.insert("selection_2".to_string(), Selector::All);
        assert_eq!(generate_unique_selection_name(&map), "selection_3");
    }

    #[test]
    fn test_named_selections_empty_view() {
        let map = BTreeMap::new();
        let _element: Element<'_, NamedSelectionAction> =
            view_named_selections(&map, None, |action| action);
    }

    #[test]
    fn test_named_selections_populated_view() {
        let mut map = BTreeMap::new();
        map.insert("site_a".to_string(), Selector::AtomIds(vec![1, 2, 3]));
        map.insert(
            "non_carbon".to_string(),
            Selector::Not {
                inner: Box::new(Selector::Symbol(vec!["C".to_string()])),
            },
        );
        let draft = ("site_a".to_string(), "active_site".to_string());
        let _element: Element<'_, NamedSelectionAction> =
            view_named_selections(&map, Some(&draft), |action| action);
    }
}
