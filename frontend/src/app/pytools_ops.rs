//! pytools のグラフ（`pdf-plot/1` の JSON）を開いて、下のパネルのタブに出す（ROADMAP 6l-ii）。
//!
//! JSON は `core` で `PlotData` にする。原子の対応（`AtomMap`）は、表示中の構造の原子の数と元素が合うときだけ作る。
//! 合わなければ、理由をプロットの下に出す。対応が作れたときだけ、点のクリックで原子を選び、
//! 3D で選んだ原子を点の強調にする（選択は 3D と共通の `Yui.selection`）。

use super::{Message, Yui};
use crate::pane_view::{self, PlotKind};
use crate::status_bar::StatusMessage;
use iced::widget::{column, container, text};
use std::rc::Rc;
use yui_core::pytools_atom_map::{AtomMap, AtomMapError, map_pytools_atoms};
use yui_core::pytools_plot::{PytoolsPlot, parse_pytools_plot};

/// 開いているグラフ。1 つだけ持つ（別のファイルを開くと置き換える）。
pub(crate) struct PytoolsPlotState {
    pub(crate) file_name: String,
    pub(crate) plot: Rc<PytoolsPlot>,
}

/// 原子の対応のキャッシュ。元素と数だけで決まるので、座標が変わる再生では作り直さない。
pub(crate) struct PytoolsMapCache {
    key: (usize, Option<String>),
    map: Rc<Result<AtomMap, AtomMapError>>,
}

/// File → Open Plot (pytools JSON)... のダイアログ。ファイル名と中身の文字列を返す（キャンセルは `None`）。
pub async fn open_plot_dialog() -> Result<Option<(String, String)>, String> {
    let Some(handle) = rfd::AsyncFileDialog::new()
        .add_filter("pytools plot", &["json"])
        .pick_file()
        .await
    else {
        return Ok(None);
    };
    let name = handle.file_name();
    let bytes = handle.read().await;
    let json = String::from_utf8(bytes).map_err(|e| format!("'{name}' is not UTF-8 text: {e}"))?;
    Ok(Some((name, json)))
}

impl Yui {
    pub(super) fn handle_pytools_plot_opened(
        &mut self,
        result: Result<Option<(String, String)>, String>,
    ) {
        let (file_name, json) = match result {
            Ok(Some(file)) => file,
            Ok(None) => return,
            Err(e) => {
                self.status_message = Some(StatusMessage::error(format!(
                    "Failed to open the plot: {e}"
                )));
                return;
            }
        };
        let plot = match parse_pytools_plot(&json) {
            Ok(plot) => plot,
            Err(e) => {
                self.status_message = Some(StatusMessage::error(format!(
                    "Failed to open the plot '{file_name}': {e}"
                )));
                return;
            }
        };
        self.status_message = Some(StatusMessage::info(format!(
            "Opened the plot '{file_name}' ({} points)",
            plot.plot.point_count()
        )));
        self.pytools_plot = Some(PytoolsPlotState {
            file_name,
            plot: Rc::new(plot),
        });
        *self.pytools_map_cache.borrow_mut() = None;
        pane_view::open_plot_tab(&mut self.pane_views, PlotKind::Pytools);
    }

    /// 開いているグラフの原子と、アクティブなモデルの原子の対応。構造がなければ `None`。
    pub(super) fn pytools_atom_map(&self) -> Option<Rc<Result<AtomMap, AtomMapError>>> {
        let state = self.pytools_plot.as_ref()?;
        let atom_group = self.state.atom_group.as_ref()?;
        let key = (
            self.state.molecule.atoms.len(),
            self.state.active_model.clone(),
        );
        let mut cache = self.pytools_map_cache.borrow_mut();
        if let Some(c) = cache.as_ref().filter(|c| c.key == key) {
            return Some(Rc::clone(&c.map));
        }
        let map = Rc::new(map_pytools_atoms(
            atom_group,
            self.state.active_model.as_deref(),
            &state.plot.atoms,
        ));
        *cache = Some(PytoolsMapCache {
            key,
            map: Rc::clone(&map),
        });
        Some(map)
    }

    /// 開いているグラフが、表示中の構造に結びついているか。結びついていなければ、理由をステータスバーに出す。
    pub(super) fn pytools_plot_is_linked(&mut self) -> bool {
        let reason = match self.pytools_atom_map().as_deref() {
            Some(Ok(_)) => return true,
            Some(Err(e)) => format!("Not linked to the structure: {e}"),
            None => "Open a structure that matches the plot to select atoms".to_string(),
        };
        self.status_message = Some(StatusMessage::warning(reason));
        false
    }

    /// pytools のグラフのタブの中身。開いていなければ、開き方を案内する。
    pub(super) fn pytools_plot_view(&self) -> iced::Element<'_, Message> {
        let subtle = crate::ui_style::palette().text_subtle;
        let Some(state) = &self.pytools_plot else {
            return container(
                text("No plot is open. Use File → Open Plot (pytools JSON)...")
                    .size(12)
                    .color(subtle),
            )
            .padding(8)
            .into();
        };
        let link_note = match self.pytools_atom_map().as_deref() {
            None => "No structure is open, so the points are not linked to atoms".to_string(),
            Some(Ok(map)) => format!("{} atoms match the structure", map.len()),
            Some(Err(e)) => format!("Not linked to the structure: {e}"),
        };
        let title = if state.plot.title.is_empty() {
            state.file_name.clone()
        } else {
            format!("{} ({})", state.plot.title, state.file_name)
        };
        let note = format!(
            "{title}: {} points. {link_note}",
            state.plot.plot.point_count()
        );
        let highlighted = self
            .plot_adapter(PlotKind::Pytools)
            .keys_for(&self.selection);
        let chart = crate::plot::view(state.plot.plot.clone(), highlighted, |click| {
            Message::PlotClicked(PlotKind::Pytools, click)
        });
        column![
            chart,
            container(text(note).size(12).color(subtle)).padding([0, 8])
        ]
        .spacing(4)
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PaneKind;
    use crate::pane_view::ViewId;
    use crate::plot::PlotClick;
    use yui_core::structure_loader::load_structure_into_state;

    fn app_with_structure() -> Yui {
        let mut app = Yui::default();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/tests/data/1hls.pdb");
        let bytes = std::fs::read(path).expect("failed to read 1hls.pdb");
        load_structure_into_state(&mut app.state, &bytes, "1hls.pdb").expect("load");
        app
    }

    /// `pdf-plot-pop.py --json` の形の JSON。`elements` の順に原子が並ぶ。
    fn plot_json(elements: &[String]) -> String {
        let atoms: Vec<_> = elements
            .iter()
            .enumerate()
            .map(|(i, e)| serde_json::json!({"index": i + 1, "element": e, "label": ""}))
            .collect();
        let points: Vec<_> = elements
            .iter()
            .enumerate()
            .map(|(i, _)| {
                serde_json::json!({"x": i + 1, "y": ((i % 7) as f64 - 3.0) / 10.0, "atom": i + 1})
            })
            .collect();
        serde_json::json!({
            "schema": "pdf-plot/1", "kind": "scatter", "title": "Mulliken charge difference",
            "axes": {"x": {"label": "atom index", "unit": null},
                     "y": {"label": "charge difference", "unit": "e"}},
            "atoms": atoms,
            "series": [{"name": "Mulliken (diff)", "points": points}],
        })
        .to_string()
    }

    fn elements_of(app: &Yui) -> Vec<String> {
        app.state
            .molecule
            .atoms
            .iter()
            .map(|a| a.element.clone())
            .collect()
    }

    fn open(app: &mut Yui, json: String) {
        let _ = app.update(Message::PytoolsPlotOpened(Ok(Some((
            "pop.json".into(),
            json,
        )))));
    }

    #[test]
    fn opening_a_plot_shows_it_in_the_bottom_pane() {
        let mut app = app_with_structure();
        let json = plot_json(&elements_of(&app));
        open(&mut app, json);
        assert_eq!(
            app.pane_views[&PaneKind::Bottom].active_view(),
            Some(ViewId::Plot(PlotKind::Pytools))
        );
        let state = app.pytools_plot.as_ref().expect("the plot is kept");
        assert_eq!(state.file_name, "pop.json");
        assert_eq!(state.plot.atoms.len(), app.state.molecule.atoms.len());
        let status = app.status_message.as_ref().expect("a message").text.clone();
        assert!(status.contains("pop.json"), "{status}");
    }

    #[test]
    fn a_bad_file_or_a_cancel_changes_nothing() {
        let mut app = app_with_structure();
        let _ = app.update(Message::PytoolsPlotOpened(Ok(None)));
        assert!(app.pytools_plot.is_none());
        assert!(app.status_message.is_none());

        let bad = r#"{"schema": "pdf-plot/2"}"#.to_string();
        open(&mut app, bad);
        assert!(app.pytools_plot.is_none());
        let status = app.status_message.as_ref().unwrap().text.clone();
        assert!(status.contains("pdf-plot/2"), "{status}");
        assert!(
            !app.pane_views
                .values()
                .any(|s| s.views.contains(&ViewId::Plot(PlotKind::Pytools)))
        );

        let _ = app.update(Message::PytoolsPlotOpened(Err("denied".into())));
        assert!(app.status_message.as_ref().unwrap().text.contains("denied"));
    }

    #[test]
    fn a_failed_open_keeps_the_plot_that_was_open() {
        let mut app = app_with_structure();
        let json = plot_json(&elements_of(&app));
        open(&mut app, json);
        open(&mut app, "not json".into());
        assert!(app.pytools_plot.is_some());
    }

    #[test]
    fn the_atom_map_is_built_only_when_the_structure_matches() {
        let mut app = app_with_structure();
        assert!(app.pytools_atom_map().is_none(), "no plot yet");

        let elements = elements_of(&app);
        open(&mut app, plot_json(&elements));
        let map = app.pytools_atom_map().expect("a structure is open");
        assert_eq!(map.as_ref().as_ref().map(AtomMap::len), Ok(elements.len()));
        // the same map is reused until the structure changes
        assert!(Rc::ptr_eq(&map, &app.pytools_atom_map().unwrap()));

        // a plot of another molecule is refused, with the reason
        open(&mut app, plot_json(&elements[..elements.len() - 1]));
        let map = app.pytools_atom_map().unwrap();
        assert!(matches!(
            map.as_ref(),
            Err(AtomMapError::CountMismatch { .. })
        ));

        // without a structure there is nothing to link with
        let _ = app.update(Message::MenuAction(crate::menu_bar::MenuAction::FileNew));
        assert!(app.pytools_atom_map().is_none());
    }

    fn click(key: Option<u64>, shift: bool, command: bool) -> Message {
        Message::PlotClicked(
            PlotKind::Pytools,
            PlotClick {
                key: key.map(yui_core::plot::PlotKey),
                shift,
                command,
            },
        )
    }

    /// 点のクリックで原子が選ばれ（3D と同じ選択）、3D で選んだ原子は点の強調になる
    #[test]
    fn clicks_select_atoms_and_the_selection_highlights_the_points() {
        use yui_core::plot::PlotKey;

        let mut app = app_with_structure();
        let json = plot_json(&elements_of(&app));
        open(&mut app, json);
        let id = |index: u32| {
            app.pytools_atom_map()
                .unwrap()
                .as_ref()
                .as_ref()
                .unwrap()
                .yui_id(index)
                .unwrap()
        };
        let (first, fifth, tenth) = (id(1), id(5), id(10));

        // 点の番号は、構造の原子の順（ProteinDF の番号 = 構造の i 番目の原子）
        let _ = app.update(click(Some(5), false, false));
        assert_eq!(
            app.selection.iter().copied().collect::<Vec<_>>(),
            vec![fifth]
        );
        let _ = app.update(click(Some(1), true, false));
        assert_eq!(app.selection, [first, fifth].into());
        let _ = app.update(click(Some(10), true, false));
        let _ = app.update(click(Some(5), false, true));
        assert_eq!(app.selection, [first, tenth].into());
        // 何もないところのクリックは、選択を外す
        let _ = app.update(click(None, false, false));
        assert!(app.selection.is_empty());

        // 3D の選択が、点の強調になる
        app.selection = [first, tenth, 999_999].into();
        let keys = app.plot_adapter(PlotKind::Pytools).keys_for(&app.selection);
        assert_eq!(keys, [PlotKey(1), PlotKey(10)].into());
    }

    /// 構造と合わないグラフでは、クリックしても何も選ばず、理由を出す（誤った原子を選ばない）
    #[test]
    fn a_plot_that_does_not_match_never_selects_atoms() {
        let mut app = app_with_structure();
        let elements = elements_of(&app);
        // 数は同じで、1 つだけ元素が違う
        let mut other = elements.clone();
        other[1] = if other[1] == "C" {
            "N".into()
        } else {
            "C".into()
        };
        open(&mut app, plot_json(&other));
        app.selection.insert(3);
        let _ = app.update(click(Some(2), false, false));
        assert_eq!(app.selection.iter().copied().collect::<Vec<_>>(), vec![3]);
        let status = app.status_message.as_ref().unwrap().text.clone();
        assert!(status.contains("Not linked"), "{status}");
        assert!(status.contains("#2"), "{status}");
        // 何もないところのクリックでも、選択は外れない
        let _ = app.update(click(None, false, false));
        assert_eq!(app.selection.len(), 1);
        // 強調もしない
        assert!(
            app.plot_adapter(PlotKind::Pytools)
                .keys_for(&app.selection)
                .is_empty()
        );
    }

    /// 構造を閉じると、クリックしても選ばず、構造を開くよう案内する
    #[test]
    fn without_a_structure_clicks_select_nothing() {
        let mut app = app_with_structure();
        let json = plot_json(&elements_of(&app));
        open(&mut app, json);
        let _ = app.update(Message::MenuAction(crate::menu_bar::MenuAction::FileNew));
        let _ = app.update(click(Some(1), false, false));
        assert!(app.selection.is_empty());
        let status = app.status_message.as_ref().unwrap().text.clone();
        assert!(status.contains("Open a structure"), "{status}");
    }

    /// 開いたグラフ（連動できる場合、できない場合、何も開いていない場合）を両方のテーマで描く
    /// （`YUI_UI_SHOTS_DIR` があれば、確認用の画像も書く）
    #[test]
    fn the_plot_draws_in_both_themes() {
        let _gpu = crate::test_gpu::lock();
        let mut app = app_with_structure();
        let elements = elements_of(&app);
        let mut cases = vec![("none", None)];
        cases.push(("linked", Some(plot_json(&elements))));
        cases.push(("refused", Some(plot_json(&elements[..elements.len() - 2]))));
        for (kind, theme) in [
            (crate::ui_style::ThemeKind::Dark, "dark"),
            (crate::ui_style::ThemeKind::Light, "light"),
        ] {
            crate::ui_style::set_theme(kind);
            for (case, json) in &cases {
                match json {
                    Some(json) => open(&mut app, json.clone()),
                    None => app.pytools_plot = None,
                }
                let element: iced::Element<'_, Message> =
                    iced::widget::container(app.pytools_plot_view())
                        .width(560)
                        .height(300)
                        .into();
                let mut ui = iced_test::simulator(element);
                let snapshot = ui.snapshot(&iced::Theme::Dark).expect("take a snapshot");
                if let Ok(dir) = std::env::var("YUI_UI_SHOTS_DIR") {
                    let base = std::path::Path::new(&dir).join(format!("pytools-{case}-{theme}"));
                    let _ = std::fs::remove_file(base.with_extension("png"));
                    std::fs::create_dir_all(&dir).expect("create the picture folder");
                    snapshot.matches_image(&base).expect("write the picture");
                }
            }
        }
        crate::ui_style::set_theme(crate::ui_style::ThemeKind::Dark);
    }
}
