//! 3Dビューポートの右クリックコンテキストメニュー (ROADMAP Phase 6d-ii)
//!
//! 8.3節「コンテキストメニュー」および 8.4節「アクションパイプラインの原則」に準拠。
//! ピッキングされた原子・残基に対して、Selection/Representation 操作を
//! メニュー経由で適用するためのUIおよびデータ型を提供する。

use iced::Element;
use yui_core::{ColorScheme, RepresentationKind};

/// 右クリックで選択された原子・残基のコンテキスト情報
#[derive(Debug, Clone, PartialEq)]
pub struct ContextMenuTarget {
    pub atom_id: usize,
    pub element: String,
    pub atom_name: String,
    pub residue_name: String,
    pub residue_id: Option<i32>,
    pub chain_id: String,
    pub position: [f32; 3],
}

impl ContextMenuTarget {
    /// ユーザーフレンドリーな要約ラベルを取得
    pub fn summary_label(&self) -> String {
        let res_info = match self.residue_id {
            Some(rid) => format!("{}:{}", self.residue_name, rid),
            None => self.residue_name.clone(),
        };
        let chain_prefix = if !self.chain_id.is_empty() {
            format!("{}/", self.chain_id)
        } else {
            String::new()
        };
        let name = if !self.atom_name.is_empty() {
            &self.atom_name
        } else {
            &self.element
        };
        format!("{}{}.{} (#{})", chain_prefix, res_info, name, self.atom_id)
    }
}

/// コンテキストメニューからトリガーされるアクション
#[derive(Debug, Clone, PartialEq)]
pub enum ContextMenuAction {
    /// 単一原子を選択（ハイライト表示）
    SelectAtom(usize),
    /// 所属する残基全体を選択
    SelectResidue { chain_id: String, residue_id: i32 },
    /// 対象原子を非表示
    HideAtom(usize),
    /// 表現形式を変更
    ShowAs {
        atom_id: usize,
        kind: RepresentationKind,
    },
    /// 配色スキームを変更
    ColorBy { atom_id: usize, scheme: ColorScheme },
    /// 対象原子を名前付き選択として保存
    SaveAtomAsNamedSelection(usize),
    /// 所属残基全体を名前付き選択として保存
    SaveResidueAsNamedSelection { chain_id: String, residue_id: i32 },
    /// メニューを閉じる
    Close,
}

/// コンテキストメニュー描画
///
/// 外側クリックで閉じるための全画面透明バックドロップと、
/// クリックされたスクリーン座標に配置されるメニューカードを構築する。
pub fn view_context_menu<'a, Message: Clone + 'a>(
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

    #[test]
    fn test_context_menu_target_summary_label() {
        let target = ContextMenuTarget {
            atom_id: 42,
            element: "C".to_string(),
            atom_name: "CA".to_string(),
            residue_name: "ALA".to_string(),
            residue_id: Some(15),
            chain_id: "A".to_string(),
            position: [1.0, 2.0, 3.0],
        };

        assert_eq!(target.summary_label(), "A/ALA:15.CA (#42)");

        let target_no_chain = ContextMenuTarget {
            atom_id: 1,
            element: "O".to_string(),
            atom_name: "O".to_string(),
            residue_name: "HOH".to_string(),
            residue_id: None,
            chain_id: "".to_string(),
            position: [0.0, 0.0, 0.0],
        };

        assert_eq!(target_no_chain.summary_label(), "HOH.O (#1)");
    }

    #[test]
    fn test_context_menu_action_variants() {
        let action1 = ContextMenuAction::SelectAtom(10);
        let action2 = ContextMenuAction::SelectResidue {
            chain_id: "A".to_string(),
            residue_id: 3,
        };
        let action3 = ContextMenuAction::HideAtom(10);
        let action4 = ContextMenuAction::ShowAs {
            atom_id: 10,
            kind: RepresentationKind::Cartoon,
        };
        let action5 = ContextMenuAction::ColorBy {
            atom_id: 10,
            scheme: ColorScheme::Chain,
        };
        let action6 = ContextMenuAction::Close;

        assert_eq!(action1, ContextMenuAction::SelectAtom(10));
        assert_ne!(action1, action3);
        assert_eq!(action6, ContextMenuAction::Close);
        assert_eq!(
            action2,
            ContextMenuAction::SelectResidue {
                chain_id: "A".to_string(),
                residue_id: 3,
            }
        );
        assert_eq!(
            action4,
            ContextMenuAction::ShowAs {
                atom_id: 10,
                kind: RepresentationKind::Cartoon,
            }
        );
        assert_eq!(
            action5,
            ContextMenuAction::ColorBy {
                atom_id: 10,
                scheme: ColorScheme::Chain,
            }
        );

        let action_save_atom = ContextMenuAction::SaveAtomAsNamedSelection(10);
        let action_save_residue = ContextMenuAction::SaveResidueAsNamedSelection {
            chain_id: "A".to_string(),
            residue_id: 3,
        };
        assert_eq!(
            action_save_atom,
            ContextMenuAction::SaveAtomAsNamedSelection(10)
        );
        assert_eq!(
            action_save_residue,
            ContextMenuAction::SaveResidueAsNamedSelection {
                chain_id: "A".to_string(),
                residue_id: 3,
            }
        );
    }

    #[test]
    fn test_context_menu_view_instantiation() {
        let target = ContextMenuTarget {
            atom_id: 5,
            element: "N".to_string(),
            atom_name: "N".to_string(),
            residue_name: "GLY".to_string(),
            residue_id: Some(2),
            chain_id: "A".to_string(),
            position: [0.0, 0.0, 0.0],
        };

        // view_context_menu がクラッシュせず Element を構築できること
        let items = crate::menu::builtin_items(
            crate::menu::location::CONTEXT_VIEWPORT,
            Some(crate::menu::MenuTarget::Atom(target)),
        );
        let _elem: iced::Element<ContextMenuAction> = view_context_menu(
            [100.0, 150.0],
            &items,
            iced::Size::new(1024.0, 768.0),
            |_| ContextMenuAction::Close,
            ContextMenuAction::Close,
        );
    }

    #[test]
    fn test_raycast_picking_integration() {
        use crate::math::screen_to_world_ray;
        use yui_core::spatial::AtomBvh;
        use yui_core::state::CameraState;

        // カメラ: (0, 0, 50) から (0, 0, 0) を向く
        let camera = CameraState {
            eye: [0.0, 0.0, 50.0],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            fov: 45.0,
        };

        // 3つの原子: #0 は中心 (0, 0, 0), #1 は右 (10, 0, 0), #2 は手前 (0, 0, 20)
        let positions = vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 0.0, 20.0]];
        let radii = vec![1.5, 1.5, 1.5];

        let bvh = AtomBvh::build(&positions);

        // 画面中心 (400, 300) / ビューポート (800, 600) からのレイ
        let (origin, dir) = screen_to_world_ray([400.0, 300.0], [800.0, 600.0], &camera);

        let hit = bvh.query_ray(origin, dir, Some(&radii), 1.0);
        assert!(hit.is_some());
        let (atom_idx, distance) = hit.unwrap();
        // 最も手前にある #2 (0, 0, 20) が最初にヒットするはず（距離は 50 - 20 - 1.5 = 28.5 付近）
        assert_eq!(atom_idx, 2);
        assert!((distance - 28.5).abs() < 1e-2);

        // 画面外（何もない右上隅）をクリックした場合
        let (miss_origin, miss_dir) = screen_to_world_ray([50.0, 50.0], [800.0, 600.0], &camera);
        let miss_hit = bvh.query_ray(miss_origin, miss_dir, Some(&radii), 1.0);
        assert!(miss_hit.is_none());
    }
}
