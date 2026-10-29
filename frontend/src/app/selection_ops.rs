//! 選択の操作（architecture.md 8.9節）。選択の状態は `Yui.selection`（利用者ごと。保存しない）。

use super::Yui;
use crate::pane_view::PlotKind;
use crate::plot::{PlotAdapter, ramachandran, sample};
use crate::selection::{self, ViewportClick};
use crate::status_bar::StatusMessage;
use iced::widget::{column, container, text};
use std::rc::Rc;
use yui_core::ramachandran::{RamachandranPoint, ramachandran_plot, ramachandran_points};
use yui_core::selector::Selector;

/// Ramachandran プロットの点のキャッシュ。`key` が同じ間は、計算し直さない。
pub(crate) struct RamachandranCache {
    key: (usize, Option<String>, u32),
    points: Rc<Vec<RamachandranPoint>>,
}

impl Yui {
    /// プロットの種類ごとの、点と原子の対応
    fn plot_adapter(&self, kind: PlotKind) -> Box<dyn PlotAdapter> {
        match kind {
            PlotKind::AtomPositions => Box::new(sample::AtomAdapter),
            PlotKind::Ramachandran => Box::new(ramachandran::RamachandranAdapter::new(
                self.ramachandran_points(),
            )),
            // 点と原子の連動は次の段階。それまでは、クリックしても何も選ばない
            PlotKind::Pytools => Box::new(sample::NoAtoms),
        }
    }

    /// アクティブなモデルの、残基ごとの φ/ψ。構造（原子の数、モデル、座標）が変わったときだけ作り直す。
    fn ramachandran_points(&self) -> Rc<Vec<RamachandranPoint>> {
        let atoms = &self.state.molecule.atoms;
        // 軌跡の再生で座標が変わるので、座標の和（ビット列）も鍵にする
        let checksum = atoms.iter().fold(0u32, |acc, a| {
            a.position
                .iter()
                .fold(acc, |acc, v| acc.wrapping_add(v.to_bits()))
        });
        let key = (atoms.len(), self.state.active_model.clone(), checksum);
        let mut cache = self.ramachandran_cache.borrow_mut();
        if let Some(c) = cache.as_ref().filter(|c| c.key == key) {
            return Rc::clone(&c.points);
        }
        let points = Rc::new(
            self.state
                .atom_group
                .as_ref()
                .map(|ag| ramachandran_points(ag, self.state.active_model.as_deref()))
                .unwrap_or_default(),
        );
        *cache = Some(RamachandranCache {
            key,
            points: Rc::clone(&points),
        });
        points
    }

    /// 散布図のタブの中身。データは今の構造から作り、強調する点は今の選択から求める。
    pub(super) fn plot_view(&self, kind: PlotKind) -> iced::Element<'_, super::Message> {
        let on_click = move |click| super::Message::PlotClicked(kind, click);
        let highlighted = |adapter: &dyn PlotAdapter| adapter.keys_for(&self.selection);
        match kind {
            PlotKind::AtomPositions => {
                let data = sample::atom_positions(&self.state.molecule.atoms);
                crate::plot::view(data, highlighted(&sample::AtomAdapter), on_click)
            }
            PlotKind::Pytools => self.pytools_plot_view(),
            PlotKind::Ramachandran => {
                let points = self.ramachandran_points();
                let plot = ramachandran_plot(&points);
                let note = if points.is_empty() {
                    "No protein residues to plot".to_string()
                } else {
                    format!(
                        "{} residues plotted, {} without both angles (chain ends and breaks)",
                        plot.data.point_count(),
                        plot.unplotted
                    )
                };
                let adapter = ramachandran::RamachandranAdapter::new(points);
                let chart = crate::plot::view(plot.data, highlighted(&adapter), on_click);
                column![
                    chart,
                    container(
                        text(note)
                            .size(12)
                            .color(crate::ui_style::palette().text_subtle)
                    )
                    .padding([0, 8])
                ]
                .spacing(4)
                .into()
            }
        }
    }

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

    /// 散布図のクリックで、選択を変える（3D の選択と同じ状態。修飾キーの扱いも同じ）。
    pub(super) fn handle_plot_clicked(&mut self, kind: PlotKind, click: &crate::plot::PlotClick) {
        if kind == PlotKind::Pytools {
            return;
        }
        let adapter = self.plot_adapter(kind);
        crate::plot::apply_click(&mut self.selection, click, adapter.as_ref());
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

    /// メニューから開いたプロットのクリックと、3D の選択の連動
    #[test]
    fn plot_clicks_and_the_3d_selection_follow_each_other() {
        use crate::PaneKind;
        use crate::menu_bar::MenuAction;
        use crate::pane_view::ViewId;
        use crate::plot::PlotClick;
        use yui_core::plot::PlotKey;

        let mut app = app_with_structure();
        let _ = app.update(Message::MenuAction(MenuAction::ViewOpenPlot(
            PlotKind::AtomPositions,
        )));
        assert_eq!(
            app.pane_views[&PaneKind::Bottom].active_view(),
            Some(ViewId::Plot(PlotKind::AtomPositions))
        );

        let ids: Vec<u32> = app
            .state
            .molecule
            .atoms
            .iter()
            .take(3)
            .map(|a| a.id)
            .collect();
        let click = |id: u32, shift: bool, command: bool| {
            Message::PlotClicked(
                PlotKind::AtomPositions,
                PlotClick {
                    key: Some(PlotKey(u64::from(id))),
                    shift,
                    command,
                },
            )
        };
        // プロットで選ぶと、3D と同じ選択が変わる
        let _ = app.update(click(ids[0], false, false));
        assert_eq!(
            app.selection.iter().copied().collect::<Vec<_>>(),
            vec![ids[0]]
        );
        let _ = app.update(click(ids[1], true, false));
        assert_eq!(app.selection.len(), 2);
        let _ = app.update(click(ids[0], false, true));
        assert_eq!(
            app.selection.iter().copied().collect::<Vec<_>>(),
            vec![ids[1]]
        );
        // 何もないところのクリックは、選択を外す
        let _ = app.update(Message::PlotClicked(
            PlotKind::AtomPositions,
            PlotClick {
                key: None,
                shift: false,
                command: false,
            },
        ));
        assert!(app.selection.is_empty());

        // 3D で選んだ原子は、プロットの強調になる
        let _ = app.update(click_on(
            app.state.molecule.atoms.get(5),
            false,
            false,
            false,
        ));
        let keys = app
            .plot_adapter(PlotKind::AtomPositions)
            .keys_for(&app.selection);
        let picked = app.state.molecule.atoms[5].id;
        assert_eq!(keys, [PlotKey(u64::from(picked))].into());
    }

    /// Ramachandran プロットの点のクリックで残基が選ばれ、3D の選択がプロットの強調になる
    #[test]
    fn ramachandran_clicks_select_whole_residues_and_the_selection_highlights_them() {
        use crate::menu_bar::MenuAction;
        use crate::plot::PlotClick;
        use yui_core::ramachandran::plot_key;

        let mut app = app_with_structure();
        let _ = app.update(Message::MenuAction(MenuAction::ViewOpenPlot(
            PlotKind::Ramachandran,
        )));
        let points = app.ramachandran_points();
        assert!(points.len() > 40);
        // 角度が両方そろった残基を 2 つ選ぶ
        let plotted: Vec<_> = points
            .iter()
            .filter(|p| p.phi.is_some() && p.psi.is_some())
            .take(2)
            .collect();
        let click = |key, shift| {
            Message::PlotClicked(
                PlotKind::Ramachandran,
                PlotClick {
                    key,
                    shift,
                    command: false,
                },
            )
        };

        let _ = app.update(click(plot_key(plotted[0]), false));
        let expected: std::collections::BTreeSet<u32> =
            plotted[0].atom_ids.iter().copied().collect();
        assert!(
            expected.len() > 3,
            "the whole residue, not only the backbone"
        );
        assert_eq!(app.selection, expected);

        let _ = app.update(click(plot_key(plotted[1]), true));
        assert_eq!(
            app.selection.len(),
            plotted[0].atom_ids.len() + plotted[1].atom_ids.len()
        );
        let keys = app
            .plot_adapter(PlotKind::Ramachandran)
            .keys_for(&app.selection);
        assert_eq!(
            keys,
            [plot_key(plotted[0]), plot_key(plotted[1])]
                .into_iter()
                .flatten()
                .collect()
        );

        // 3D で原子を 1 つ選ぶと、その残基の点だけが強調される
        let atom = app.state.molecule.atoms[plotted[1].atom_ids[0] as usize - 1].clone();
        let _ = app.update(click_on(Some(&atom), false, false, false));
        let keys = app
            .plot_adapter(PlotKind::Ramachandran)
            .keys_for(&app.selection);
        assert_eq!(keys, plot_key(plotted[1]).into_iter().collect());

        // 何もないところのクリックで、選択が外れる
        let _ = app.update(click(None, false));
        assert!(app.selection.is_empty());
    }

    #[test]
    fn the_ramachandran_points_are_cached_until_the_structure_changes() {
        let mut app = app_with_structure();
        let first = app.ramachandran_points();
        assert!(Rc::ptr_eq(&first, &app.ramachandran_points()));
        let _ = app.update(Message::MenuAction(crate::menu_bar::MenuAction::FileNew));
        assert!(app.ramachandran_points().is_empty());
        assert!(!Rc::ptr_eq(&first, &app.ramachandran_points()));
    }

    #[test]
    fn without_a_structure_the_ramachandran_plot_is_empty() {
        let app = Yui::default();
        assert!(app.ramachandran_points().is_empty());
        let _ = app.plot_view(PlotKind::Ramachandran);
    }

    /// 実構造の Ramachandran プロットを両方のテーマで描く（`YUI_UI_SHOTS_DIR` があれば、確認用の画像も書く）
    #[test]
    fn ramachandran_plot_draws_in_both_themes() {
        let _gpu = crate::test_gpu::lock();
        let mut app = app_with_structure();
        let first = app.ramachandran_points()[3].atom_ids.clone();
        app.selection = first.into_iter().collect();
        for (kind, name) in [
            (crate::ui_style::ThemeKind::Dark, "ramachandran-dark"),
            (crate::ui_style::ThemeKind::Light, "ramachandran-light"),
        ] {
            crate::ui_style::set_theme(kind);
            let element: iced::Element<'_, Message> =
                iced::widget::container(app.plot_view(PlotKind::Ramachandran))
                    .width(480)
                    .height(360)
                    .into();
            let mut ui = iced_test::simulator(element);
            let snapshot = ui.snapshot(&iced::Theme::Dark).expect("take a snapshot");
            if let Ok(dir) = std::env::var("YUI_UI_SHOTS_DIR") {
                let base = std::path::Path::new(&dir).join(name);
                let _ = std::fs::remove_file(base.with_extension("png"));
                std::fs::create_dir_all(&dir).expect("create the picture folder");
                snapshot.matches_image(&base).expect("write the picture");
            }
        }
        crate::ui_style::set_theme(crate::ui_style::ThemeKind::Dark);
    }
}
