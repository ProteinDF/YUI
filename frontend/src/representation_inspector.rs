//! Representation 一覧・インスペクター（Rightペイン）ウィジェット (ROADMAP Phase 6d-v)
//!
//! 8.3節「コンテキストメニュー」および 8.4節「アクションパイプラインの原則」に準拠。
//! `AppState.representations` の一覧表示、可視性トグル、表示形式（Kind）の変更、
//! 配色（Color）の変更、追加・削除、およびコンテキストメニュー操作を提供する。

use crate::ui_style::palette;
use iced::widget::{Space, button, column, container, mouse_area, row, scrollable, slider, text};
use iced::{Alignment, Border, Element, Length, Padding};
use yui_core::interaction::InteractionKind;
use yui_core::representation::{ColorScheme, Representation, RepresentationKind};

/// Representationインスペクターから発行されるアクション
#[derive(Debug, Clone, PartialEq)]
pub enum RepresentationAction {
    /// 新規Representationの追加
    AddRepresentation,
    /// 指定インデックスの可視性トグル
    ToggleVisibility(usize),
    /// 指定インデックスの表示形式変更
    ChangeKind {
        index: usize,
        kind: RepresentationKind,
    },
    /// 指定インデックスの相互作用種類トグル
    ToggleInteractionKind { index: usize, kind: InteractionKind },
    /// 指定インデックスの配色変更
    ChangeColor { index: usize, color: ColorScheme },
    /// 指定インデックスの透明度変更（0.0〜1.0。範囲外は丸める）
    ChangeOpacity { index: usize, opacity: f32 },
    /// 指定インデックスのRepresentation削除
    Delete(usize),
    /// 右クリックコンテキストメニューを開く
    OpenContextMenu { index: usize, screen_pos: [f32; 2] },
}

/// Representationコンテキストメニューから発行されるアクション
#[derive(Debug, Clone, PartialEq)]
pub enum RepContextMenuAction {
    /// 可視性トグル
    ToggleVisibility(usize),
    /// 表示形式変更
    ShowAs {
        index: usize,
        kind: RepresentationKind,
    },
    /// 配色変更
    ColorBy { index: usize, scheme: ColorScheme },
    /// 削除
    Delete(usize),
    /// メニューを閉じる
    Close,
}

/// コンテキストメニュー用のターゲット情報
#[derive(Debug, Clone, PartialEq)]
pub struct RepContextTarget {
    pub index: usize,
    pub id: String,
    pub kind: RepresentationKind,
    pub visible: bool,
}

/// 指定した行の透明度だけを設定する。値は 0.0〜1.0 に丸める（NaN は 1.0 = 不透明とする）。
/// 範囲外のインデックスなら、何も変えず `false` を返す。
pub fn set_opacity(representations: &mut [Representation], index: usize, opacity: f32) -> bool {
    let Some(rep) = representations.get_mut(index) else {
        return false;
    };
    rep.opacity = if opacity.is_nan() {
        1.0
    } else {
        opacity.clamp(0.0, 1.0)
    };
    true
}

/// 透明度が描画に反映される表示形式か（分子表面と相互作用の線。ほかは使っていない）
pub fn kind_uses_opacity(kind: RepresentationKind) -> bool {
    matches!(
        kind,
        RepresentationKind::Surface | RepresentationKind::Interaction
    )
}

/// 現在の相互作用種類の有効フィルタ一覧と、トグルされた種類から、
/// 更新後の相互作用種類の配列を算出する純粋関数。
///
/// 規則:
/// - 空スライス（`&[]`）は「全種類が有効」を表す。
/// - 全種類有効の状態から1種類をトグル（オフ）にした場合、その種類を除外した残り3種類を返す。
/// - 現在オンになっている種類が1種類だけの状態で、その種類をオフにしようとした場合、
///   変更せずそのまま返す（最後の1種類はオフにできない。非表示にしたい場合は行の可視性トグルを使用）。
/// - 3種類が有効な状態で残りの1種類をオンにした場合、全種類有効を表す空配列（`Vec::new()`）を返す。
pub fn toggle_interaction_kind(
    current: &[InteractionKind],
    kind: InteractionKind,
) -> Vec<InteractionKind> {
    let all_kinds = [
        InteractionKind::HydrogenBond,
        InteractionKind::SaltBridge,
        InteractionKind::ChPi,
        InteractionKind::Disulfide,
    ];

    let mut current_set: std::collections::HashSet<_> = if current.is_empty() {
        all_kinds.into_iter().collect()
    } else {
        current.iter().copied().collect()
    };

    if current_set.contains(&kind) {
        if current_set.len() > 1 {
            current_set.remove(&kind);
        }
    } else {
        current_set.insert(kind);
    }

    if current_set.len() == all_kinds.len() {
        Vec::new()
    } else {
        all_kinds
            .into_iter()
            .filter(|k| current_set.contains(k))
            .collect()
    }
}

/// Representation一覧（Rightペイン）描画純粋関数
pub fn view_representation_inspector<'a, Message: Clone + 'a>(
    representations: &'a [Representation],
    on_action: impl Fn(RepresentationAction) -> Message + Copy + 'a,
) -> Element<'a, Message> {
    // 1. 上部ツールバー（ヘッダー・件数・+ Add ボタン）
    let header_bar = row![
        text("Representations")
            .size(13)
            .color(palette().text_secondary),
        Space::new().width(Length::Fixed(6.0)),
        text(format!("({} items)", representations.len()))
            .size(11)
            .color(palette().text_faint),
        Space::new().width(Length::Fill),
        button(
            text("+ Add")
                .size(11)
                .wrapping(iced::widget::text::Wrapping::None)
        )
        .padding(Padding::from([2, 8]))
        .style(|_theme, status| match status {
            button::Status::Hovered => button::Style {
                background: Some(palette().select_bg.into()),
                text_color: palette().text,
                border: Border::default().rounded(3),
                ..button::Style::default()
            },
            _ => button::Style {
                background: Some(palette().select_bg_soft.into()),
                text_color: palette().select_text,
                border: Border::default().rounded(3),
                ..button::Style::default()
            },
        })
        .on_press(on_action(RepresentationAction::AddRepresentation)),
    ]
    .align_y(Alignment::Center)
    .padding(Padding::from([6, 8]));

    // 2. リスト本体
    let content: Element<'a, Message> = if representations.is_empty() {
        container(
            column![
                text("No representations")
                    .size(13)
                    .color(palette().text_subtle),
                Space::new().height(Length::Fixed(8.0)),
                text("Click '+ Add' above to create one,")
                    .size(11)
                    .color(palette().text_faint),
                text("or right-click objects in 3D Viewport / Tree")
                    .size(11)
                    .color(palette().text_faint),
            ]
            .spacing(3)
            .align_x(Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .padding(Padding::from(16))
        .into()
    } else {
        let mut list_column = column![].spacing(6);

        for (index, rep) in representations.iter().enumerate() {
            list_column = list_column.push(render_representation_card(index, rep, on_action));
        }

        scrollable(list_column.padding(Padding::from([4, 6])))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    };

    column![header_bar, content]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// 単一 Representation カードの描画
fn render_representation_card<'a, Message: Clone + 'a>(
    index: usize,
    rep: &'a Representation,
    on_action: impl Fn(RepresentationAction) -> Message + Copy + 'a,
) -> Element<'a, Message> {
    // 可視性トグルアイコン
    let (vis_icon, vis_color) = if rep.visible {
        ("👁", palette().visible_icon)
    } else {
        ("🚫", palette().text_subtle)
    };

    let vis_btn = button(text(vis_icon).size(12))
        .padding(Padding::from([2, 4]))
        .style(move |_theme, _status| button::Style {
            background: None,
            text_color: vis_color,
            ..button::Style::default()
        })
        .on_press(on_action(RepresentationAction::ToggleVisibility(index)));

    // 削除ボタン
    let del_btn = button(text("✕").size(11))
        .padding(Padding::from([2, 6]))
        .style(|_theme, status| match status {
            button::Status::Hovered => button::Style {
                background: Some(palette().danger_soft_bg.into()),
                text_color: palette().danger_soft_text,
                border: Border::default().rounded(2),
                ..button::Style::default()
            },
            _ => button::Style {
                background: None,
                text_color: palette().text_subtle,
                ..button::Style::default()
            },
        })
        .on_press(on_action(RepresentationAction::Delete(index)));

    // 1行目: 可視トグル + ID + 削除ボタン
    let top_row = row![
        vis_btn,
        text(&rep.id).size(12).color(if rep.visible {
            palette().text
        } else {
            palette().text_subtle
        }),
        Space::new().width(Length::Fill),
        del_btn,
    ]
    .align_y(Alignment::Center)
    .spacing(4);

    // 2行目: Selection 要約
    let sel_summary = if rep.hides_atoms {
        format!("{:?} (hides these atoms)", rep.selection)
    } else {
        format!("{:?}", rep.selection)
    };
    let sel_row = row![
        text("Selection:").size(10).color(palette().text_subtle),
        Space::new().width(Length::Fixed(4.0)),
        text(sel_summary).size(10).color(palette().text_secondary),
    ]
    .align_y(Alignment::Center);

    // 3行目: Kind 選択ボタン群
    let kind_btn = |k: RepresentationKind, label: &'static str| {
        let is_active = rep.kind == k;
        button(text(label).size(10))
            .padding(Padding::from([1, 4]))
            .style(move |_theme, status| {
                if is_active {
                    button::Style {
                        background: Some(palette().chip_blue_bg.into()),
                        text_color: palette().on_chip,
                        border: Border::default().rounded(2),
                        ..button::Style::default()
                    }
                } else if status == button::Status::Hovered {
                    button::Style {
                        background: Some(palette().item_hover_bg.into()),
                        text_color: palette().text,
                        border: Border::default().rounded(2),
                        ..button::Style::default()
                    }
                } else {
                    button::Style {
                        background: Some(palette().chip_bg.into()),
                        text_color: palette().text_secondary,
                        border: Border::default().rounded(2),
                        ..button::Style::default()
                    }
                }
            })
            .on_press(on_action(RepresentationAction::ChangeKind {
                index,
                kind: k,
            }))
    };

    let kind_row = column![
        row![
            text("Kind:").size(10).color(palette().text_subtle),
            Space::new().width(Length::Fixed(4.0)),
            text(format!("{:?}", rep.kind))
                .size(10)
                .color(palette().text_value),
        ]
        .align_y(Alignment::Center),
        row![
            kind_btn(RepresentationKind::BallAndStick, "Ball&Stick"),
            kind_btn(RepresentationKind::Cartoon, "Cartoon"),
            kind_btn(RepresentationKind::Cpk, "Spacefill"),
            kind_btn(RepresentationKind::Licorice, "Licorice"),
            kind_btn(RepresentationKind::Line, "Line"),
            kind_btn(RepresentationKind::Surface, "Surface"),
            kind_btn(RepresentationKind::Interaction, "Interactions"),
        ]
        .spacing(3)
        .wrap(),
    ]
    .spacing(2);

    // 相互作用の種類の絞り込みトグルボタン群（kind == Interaction のときのみ表示）
    let interaction_types_row: Option<Element<'a, Message>> = if rep.kind
        == RepresentationKind::Interaction
    {
        let toggle_btn = |k: InteractionKind, label: &'static str| {
            let is_active = rep.interaction_kinds.is_empty() || rep.interaction_kinds.contains(&k);
            button(text(label).size(10))
                .padding(Padding::from([1, 5]))
                .style(move |_theme, status| {
                    if is_active {
                        button::Style {
                            background: Some(palette().chip_teal_bg.into()),
                            text_color: palette().on_chip,
                            border: Border::default().rounded(2),
                            ..button::Style::default()
                        }
                    } else if status == button::Status::Hovered {
                        button::Style {
                            background: Some(palette().item_hover_bg.into()),
                            text_color: palette().text,
                            border: Border::default().rounded(2),
                            ..button::Style::default()
                        }
                    } else {
                        button::Style {
                            background: Some(palette().chip_bg.into()),
                            text_color: palette().text_subtle,
                            border: Border::default().rounded(2),
                            ..button::Style::default()
                        }
                    }
                })
                .on_press(on_action(RepresentationAction::ToggleInteractionKind {
                    index,
                    kind: k,
                }))
        };

        let row_widget = column![
            row![
                text("Types:").size(10).color(palette().text_subtle),
                Space::new().width(Length::Fixed(4.0)),
                text(if rep.interaction_kinds.is_empty() {
                    "All (4 kinds)".to_string()
                } else {
                    format!("{} active", rep.interaction_kinds.len())
                })
                .size(10)
                .color(palette().text_value),
            ]
            .align_y(Alignment::Center),
            row![
                toggle_btn(InteractionKind::HydrogenBond, "H-bond"),
                toggle_btn(InteractionKind::SaltBridge, "Salt bridge"),
                toggle_btn(InteractionKind::ChPi, "CH-π"),
                toggle_btn(InteractionKind::Disulfide, "Disulfide"),
            ]
            .spacing(3)
            .wrap(),
        ]
        .spacing(2);

        Some(row_widget.into())
    } else {
        None
    };

    // 4行目: Color 選択ボタン群
    let color_name = match rep.color {
        ColorScheme::Element => "Element",
        ColorScheme::Chain => "Chain",
        ColorScheme::Residue => "Residue",
        ColorScheme::SecondaryStructure => "SecondaryStructure",
        ColorScheme::Uniform(_) => "Uniform",
    };

    let color_btn = |scheme: ColorScheme, label: &'static str| {
        let is_active = std::mem::discriminant(&rep.color) == std::mem::discriminant(&scheme);
        button(text(label).size(10))
            .padding(Padding::from([1, 4]))
            .style(move |_theme, status| {
                if is_active {
                    button::Style {
                        background: Some(palette().chip_orange_bg.into()),
                        text_color: palette().on_chip,
                        border: Border::default().rounded(2),
                        ..button::Style::default()
                    }
                } else if status == button::Status::Hovered {
                    button::Style {
                        background: Some(palette().item_hover_bg.into()),
                        text_color: palette().text,
                        border: Border::default().rounded(2),
                        ..button::Style::default()
                    }
                } else {
                    button::Style {
                        background: Some(palette().chip_bg.into()),
                        text_color: palette().text_secondary,
                        border: Border::default().rounded(2),
                        ..button::Style::default()
                    }
                }
            })
            .on_press(on_action(RepresentationAction::ChangeColor {
                index,
                color: scheme,
            }))
    };

    let color_row = column![
        row![
            text("Color:").size(10).color(palette().text_subtle),
            Space::new().width(Length::Fixed(4.0)),
            text(color_name).size(10).color(palette().text_value),
        ]
        .align_y(Alignment::Center),
        row![
            color_btn(ColorScheme::Element, "Element"),
            color_btn(ColorScheme::Chain, "Chain"),
            color_btn(ColorScheme::Residue, "Residue"),
            color_btn(ColorScheme::SecondaryStructure, "SS"),
            color_btn(ColorScheme::Uniform([1.0, 0.85, 0.1, 1.0]), "Uniform"),
        ]
        .spacing(3)
        .wrap(),
    ]
    .spacing(2);

    let mut card_column = column![top_row, sel_row, kind_row];
    if let Some(types_row) = interaction_types_row {
        card_column = card_column.push(types_row);
    }
    card_column = card_column.push(color_row);
    if kind_uses_opacity(rep.kind) {
        let opacity_row = row![
            text("Opacity:").size(10).color(palette().text_subtle),
            slider(0.0..=1.0, rep.opacity, move |v| on_action(
                RepresentationAction::ChangeOpacity { index, opacity: v }
            ))
            .step(0.01_f32)
            .width(Length::Fill),
            text(format!("{}%", (rep.opacity * 100.0).round() as i32))
                .size(10)
                .color(palette().text_value),
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        card_column = card_column.push(opacity_row);
    }

    let card_content = card_column.spacing(5).padding(Padding::from(6));

    let card_container = container(card_content)
        .style(|_theme| container::Style {
            background: Some(palette().panel_bg.into()),
            border: Border {
                color: palette().panel_border,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..container::Style::default()
        })
        .width(Length::Fill);

    // 右クリックでコンテキストメニューを開く
    mouse_area(card_container)
        .on_right_press(on_action(RepresentationAction::OpenContextMenu {
            index,
            screen_pos: [200.0, 150.0],
        }))
        .into()
}

/// Representation カード用の右クリックコンテキストメニュー描画
pub fn view_rep_context_menu<'a, Message: Clone + 'a>(
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
        270.0,
        crate::menu::Anchor::Cursor(screen_pos),
        window,
        on_command,
        on_close,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use yui_core::selector::Selector;

    #[test]
    fn test_inspector_empty_state() {
        let reps: Vec<Representation> = Vec::new();
        let _elem: Element<'_, RepresentationAction> = view_representation_inspector(&reps, |a| a);
    }

    #[test]
    fn test_inspector_populated_state() {
        let reps = vec![
            Representation::new(
                "rep_1",
                Selector::All,
                RepresentationKind::BallAndStick,
                ColorScheme::Element,
            ),
            Representation::new(
                "rep_2",
                Selector::AtomIds(vec![1, 2, 3]),
                RepresentationKind::Cartoon,
                ColorScheme::Chain,
            )
            .with_visible(false),
            Representation::new(
                "rep_3",
                Selector::All,
                RepresentationKind::Interaction,
                ColorScheme::Uniform([1.0, 0.8, 0.0, 1.0]),
            )
            .with_interaction_kinds(vec![
                InteractionKind::HydrogenBond,
                InteractionKind::Disulfide,
            ]),
        ];

        let _elem: Element<'_, RepresentationAction> = view_representation_inspector(&reps, |a| a);
    }

    #[test]
    fn test_inspector_actions_mutation() {
        let mut reps = vec![
            Representation::new(
                "rep_1",
                Selector::All,
                RepresentationKind::BallAndStick,
                ColorScheme::Element,
            ),
            Representation::new(
                "rep_2",
                Selector::All,
                RepresentationKind::Cartoon,
                ColorScheme::Chain,
            ),
            Representation::new(
                "rep_3",
                Selector::All,
                RepresentationKind::Interaction,
                ColorScheme::Element,
            ),
        ];

        // 1. ToggleVisibility
        reps[0].visible = !reps[0].visible;
        assert!(!reps[0].visible);
        reps[0].visible = !reps[0].visible;
        assert!(reps[0].visible);

        // 2. ChangeKind
        reps[0].kind = RepresentationKind::Cpk;
        assert_eq!(reps[0].kind, RepresentationKind::Cpk);

        reps[1].kind = RepresentationKind::Interaction;
        assert_eq!(reps[1].kind, RepresentationKind::Interaction);

        // 3. ChangeColor
        reps[0].color = ColorScheme::Residue;
        assert_eq!(reps[0].color, ColorScheme::Residue);

        // 4. Toggle interaction kinds on rep_3
        assert!(
            reps[2].interaction_kinds.is_empty(),
            "starts empty (all kinds)"
        );
        // Toggle Disulfide -> should now have HB, SB, ChPi (3 kinds)
        reps[2].interaction_kinds =
            toggle_interaction_kind(&reps[2].interaction_kinds, InteractionKind::Disulfide);
        assert_eq!(reps[2].interaction_kinds.len(), 3);
        assert!(
            !reps[2]
                .interaction_kinds
                .contains(&InteractionKind::Disulfide)
        );

        // Toggle Disulfide back on -> all on, should clear back to empty
        reps[2].interaction_kinds =
            toggle_interaction_kind(&reps[2].interaction_kinds, InteractionKind::Disulfide);
        assert!(
            reps[2].interaction_kinds.is_empty(),
            "all on should be represented as empty"
        );

        // 5. AddRepresentation
        let new_rep = Representation::default();
        reps.push(new_rep);
        assert_eq!(reps.len(), 4);

        // 6. Delete
        reps.remove(1);
        assert_eq!(reps.len(), 3);
        assert_eq!(reps[0].id, "rep_1");
        assert_eq!(reps[1].id, "rep_3");
    }

    #[test]
    fn test_rep_context_menu_instantiation() {
        let target = RepContextTarget {
            index: 0,
            id: "rep_test".to_string(),
            kind: RepresentationKind::Cartoon,
            visible: true,
        };
        let items = crate::menu::builtin_items(
            crate::menu::location::CONTEXT_REPRESENTATION,
            Some(crate::menu::MenuTarget::Representation(target)),
        );
        let _menu: Element<'_, RepContextMenuAction> = view_rep_context_menu(
            [150.0, 120.0],
            &items,
            iced::Size::new(1024.0, 768.0),
            |_| RepContextMenuAction::Close,
            RepContextMenuAction::Close,
        );
    }

    #[test]
    fn test_toggle_interaction_kind() {
        // 1. 全種類(空)から1つオフにすると残り3種類になる
        let initial: Vec<InteractionKind> = Vec::new();
        let res1 = toggle_interaction_kind(&initial, InteractionKind::Disulfide);
        assert_eq!(res1.len(), 3);
        assert!(!res1.contains(&InteractionKind::Disulfide));
        assert!(res1.contains(&InteractionKind::HydrogenBond));
        assert!(res1.contains(&InteractionKind::SaltBridge));
        assert!(res1.contains(&InteractionKind::ChPi));

        // 2. 3種類の状態で残りの1つをオンにすると空(全種類)に戻る
        let res2 = toggle_interaction_kind(&res1, InteractionKind::Disulfide);
        assert!(res2.is_empty(), "all on should be represented as empty");

        // 3. 1つずつオフにしていき、1種類だけ残す
        let res_2kinds = toggle_interaction_kind(&res1, InteractionKind::ChPi);
        assert_eq!(res_2kinds.len(), 2);
        let res_1kind = toggle_interaction_kind(&res_2kinds, InteractionKind::SaltBridge);
        assert_eq!(res_1kind, vec![InteractionKind::HydrogenBond]);

        // 4. 1種類だけの状態でそれをオフにしても変わらない（最後の1種類はオフにできない）
        let res_unchanged = toggle_interaction_kind(&res_1kind, InteractionKind::HydrogenBond);
        assert_eq!(
            res_unchanged,
            vec![InteractionKind::HydrogenBond],
            "the last remaining kind cannot be toggled off"
        );
    }

    #[test]
    fn test_set_opacity_clamps_and_changes_only_that_row() {
        let mut reps = vec![
            Representation::new(
                "a",
                yui_core::selector::Selector::All,
                RepresentationKind::Surface,
                ColorScheme::Element,
            ),
            Representation::new(
                "b",
                yui_core::selector::Selector::All,
                RepresentationKind::Surface,
                ColorScheme::Element,
            ),
        ];
        assert!(set_opacity(&mut reps, 1, 0.4));
        assert_eq!((reps[0].opacity, reps[1].opacity), (1.0, 0.4));
        set_opacity(&mut reps, 1, 1.7);
        assert_eq!(reps[1].opacity, 1.0);
        set_opacity(&mut reps, 1, -0.3);
        assert_eq!(reps[1].opacity, 0.0);
        set_opacity(&mut reps, 1, f32::NAN);
        assert_eq!(reps[1].opacity, 1.0);
        // 範囲外のインデックスは何も変えない
        assert!(!set_opacity(&mut reps, 2, 0.5));
        assert_eq!(reps[0].opacity, 1.0);
    }

    #[test]
    fn test_opacity_only_for_surface_and_interaction() {
        assert!(kind_uses_opacity(RepresentationKind::Surface));
        assert!(kind_uses_opacity(RepresentationKind::Interaction));
        assert!(!kind_uses_opacity(RepresentationKind::Cartoon));
        assert!(!kind_uses_opacity(RepresentationKind::BallAndStick));
    }

    #[test]
    fn test_opacity_change_does_not_change_the_surface_geometry_signature() {
        let mut reps = vec![Representation::new(
            "s",
            yui_core::selector::Selector::All,
            RepresentationKind::Surface,
            ColorScheme::Element,
        )];
        let before = crate::surface_render::extract_surface_geometry_signature(&reps);
        set_opacity(&mut reps, 0, 0.3);
        let after = crate::surface_render::extract_surface_geometry_signature(&reps);
        assert_eq!(before, after);
    }
}
