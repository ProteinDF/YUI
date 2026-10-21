//! 選択の操作（architecture.md 8.9節）。選択の状態は `Yui.selection`（利用者ごと。保存しない）。

use super::Yui;
use crate::selection::{self, ViewportClick};
use crate::status_bar::StatusMessage;
use yui_core::selector::Selector;

impl Yui {
    /// 分子図の左クリックで、選択を変える。
    ///
    /// 原子のないところのクリックは、修飾キーがなければ選択を外す。ダブルクリックは、クリックした原子の残基全体。
    pub(super) fn handle_viewport_clicked(&mut self, click: ViewportClick) {
        let hit: Vec<u32> = match &click.atom {
            None => Vec::new(),
            Some(a) if click.double => selection::residue_atom_ids(
                &self.state.molecule.atoms,
                &a.chain_id,
                a.residue_index,
            ),
            Some(a) => vec![a.atom_id],
        };
        let op = selection::op_from_modifiers(click.shift, click.command);
        selection::apply(&mut self.selection, op, &hit);
        self.report_selection();
    }

    /// 選択を、指定した原子だけにする（右クリックのメニューの Select Atom / Select Residue、構造ツリー）。
    /// Representation は増やさない。
    pub(super) fn select_only(&mut self, atom_ids: &[u32]) {
        selection::apply(&mut self.selection, selection::SelectOp::Replace, atom_ids);
        self.report_selection();
    }

    /// セレクターに当たる原子だけを選ぶ（構造がなければ、何も選ばない）
    pub(super) fn select_by_selector(&mut self, selector: &Selector) {
        let ids = self
            .state
            .atom_group
            .as_ref()
            .map(|ag| selector.select_atom_ids(ag))
            .unwrap_or_default();
        self.select_only(&ids);
    }

    /// 選択を外す
    pub(super) fn clear_selection(&mut self) {
        if !self.selection.is_empty() {
            self.selection.clear();
            self.status_message = Some(StatusMessage::info("Selection cleared".to_string()));
        }
    }

    /// 今の選択を、名前付き選択として保存する（選択そのものは保存しないので、残したいときに使う）
    pub(super) fn save_selection_as_named(&mut self) {
        if self.selection.is_empty() {
            self.status_message = Some(StatusMessage::warning("Nothing is selected".to_string()));
            return;
        }
        let selector = Selector::AtomIds(self.selection.iter().copied().collect());
        let name =
            crate::named_selections::generate_unique_selection_name(&self.state.named_selections);
        self.state.named_selections.insert(name.clone(), selector);
        self.status_message = Some(StatusMessage::info(format!(
            "Saved {} selected atom(s) as named selection '{name}'",
            self.selection.len()
        )));
    }

    fn report_selection(&mut self) {
        self.status_message = Some(StatusMessage::info(match self.selection.len() {
            0 => "Selection cleared".to_string(),
            n => format!("{n} atom(s) selected"),
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Message;
    use crate::context_menu::ContextMenuAction;
    use crate::selection::ClickedAtom;
    use crate::structure_tree::StructureTreeAction;
    use yui_core::structure_loader::load_structure_into_state;

    /// 1hls を読み込んだアプリ
    fn app_with_structure() -> Yui {
        let mut app = Yui::default();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/tests/data/1hls.pdb");
        let bytes = std::fs::read(path).expect("failed to read 1hls.pdb");
        load_structure_into_state(&mut app.state, &bytes, "1hls.pdb").expect("load");
        app
    }

    fn click_on(
        atom: Option<&yui_core::state::Atom>,
        shift: bool,
        command: bool,
        double: bool,
    ) -> Message {
        Message::ViewportClicked(ViewportClick {
            atom: atom.map(|a| ClickedAtom {
                atom_id: a.id,
                chain_id: a.chain_id.clone(),
                residue_index: a.residue_index,
            }),
            shift,
            command,
            double,
        })
    }

    #[test]
    fn click_replaces_adds_toggles_and_clears() {
        let mut app = app_with_structure();
        let atoms = app.state.molecule.atoms.clone();
        let (a, b) = (&atoms[0], &atoms[1]);

        let _ = app.update(click_on(Some(a), false, false, false));
        assert_eq!(
            app.selection.iter().copied().collect::<Vec<_>>(),
            vec![a.id]
        );

        // Shift で追加
        let _ = app.update(click_on(Some(b), true, false, false));
        assert_eq!(app.selection.len(), 2);

        // Ctrl / Cmd で切り替え（選ばれていれば外す）
        let _ = app.update(click_on(Some(b), false, true, false));
        assert_eq!(
            app.selection.iter().copied().collect::<Vec<_>>(),
            vec![a.id]
        );

        // 修飾キーつきで何もないところをクリックしても、外れない
        let _ = app.update(click_on(None, true, false, false));
        assert_eq!(app.selection.len(), 1);

        // 修飾キーなしで何もないところをクリックすると、外れる
        let _ = app.update(click_on(None, false, false, false));
        assert!(app.selection.is_empty());
    }

    #[test]
    fn double_click_selects_the_whole_residue() {
        let mut app = app_with_structure();
        let atoms = app.state.molecule.atoms.clone();
        let a = &atoms[10];
        let expected = crate::selection::residue_atom_ids(&atoms, &a.chain_id, a.residue_index);
        assert!(expected.len() > 1, "a residue has several atoms");

        let _ = app.update(click_on(Some(a), false, false, true));
        assert_eq!(app.selection.iter().copied().collect::<Vec<_>>(), expected);
    }

    #[test]
    fn menu_and_tree_selection_do_not_add_representations() {
        let mut app = app_with_structure();
        let before = app.state.representations.clone();
        let atoms = app.state.molecule.atoms.clone();
        let a = &atoms[3];

        let _ = app.update(Message::ContextMenuAction(ContextMenuAction::SelectAtom(
            a.id as usize,
        )));
        assert_eq!(
            app.selection.iter().copied().collect::<Vec<_>>(),
            vec![a.id]
        );

        let _ = app.update(Message::ContextMenuAction(
            ContextMenuAction::SelectResidue {
                chain_id: a.chain_id.clone(),
                residue_id: a.residue_index,
            },
        ));
        assert_eq!(
            app.selection.iter().copied().collect::<Vec<_>>(),
            crate::selection::residue_atom_ids(&atoms, &a.chain_id, a.residue_index)
        );

        let selector = Selector::ResidueRange {
            chain: Some(a.chain_id.clone()),
            start: a.residue_index,
            end: a.residue_index,
        };
        let _ = app.update(Message::StructureTreeAction(
            StructureTreeAction::SelectNode {
                key: "res".into(),
                label: "residue".into(),
                selector,
            },
        ));
        assert!(!app.selection.is_empty());

        // Representation は 1 つも増えない（以前は黄色の Representation を足していた）
        assert_eq!(app.state.representations, before);
    }

    #[test]
    fn selection_is_not_part_of_the_workspace_or_the_shared_state() {
        let mut app = app_with_structure();
        let panes = crate::pane_view::default_pane_views();
        let yaml_before = menu_bar_yaml(&app, &panes);
        let state_before = app.state.clone();

        let atoms = app.state.molecule.atoms.clone();
        let _ = app.update(click_on(Some(&atoms[0]), false, false, true));
        assert!(!app.selection.is_empty());

        // ワークスペースの YAML も、協調編集で共有する AppState も、選択では変わらない
        assert_eq!(menu_bar_yaml(&app, &panes), yaml_before);
        assert_eq!(app.state, state_before);
    }

    fn menu_bar_yaml(app: &Yui, panes: &crate::pane_view::PaneLayout) -> String {
        crate::menu_bar::export_workspace_to_yaml(&app.state, panes).expect("export")
    }

    #[test]
    fn clear_and_save_as_named_selection() {
        let mut app = app_with_structure();
        // 何も選ばれていないときは、保存しない
        let _ = app.update(Message::SaveSelectionAsNamed);
        assert!(app.state.named_selections.is_empty());

        let atoms = app.state.molecule.atoms.clone();
        let _ = app.update(click_on(Some(&atoms[0]), false, false, false));
        let _ = app.update(click_on(Some(&atoms[1]), true, false, false));
        let _ = app.update(Message::SaveSelectionAsNamed);
        assert_eq!(app.state.named_selections.len(), 1);
        let saved = app.state.named_selections.values().next().unwrap();
        assert_eq!(saved, &Selector::AtomIds(vec![atoms[0].id, atoms[1].id]));

        // 保存しても選択は残り、Esc / メニューで外れる
        assert_eq!(app.selection.len(), 2);
        let _ = app.update(Message::ClearSelection);
        assert!(app.selection.is_empty());
    }

    #[test]
    fn loading_a_new_structure_drops_the_old_selection() {
        let mut app = app_with_structure();
        let atoms = app.state.molecule.atoms.clone();
        let _ = app.update(click_on(Some(&atoms[0]), false, false, false));
        assert!(!app.selection.is_empty());
        let _ = app.update(Message::MenuAction(crate::menu_bar::MenuAction::FileNew));
        assert!(app.selection.is_empty());
    }
}
