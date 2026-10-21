//! 表示中のビューを PNG にして保存する（File → Export Image (PNG)...）。
//!
//! 画面とは別の wgpu のデバイスで、画面と同じ描画の準備（`ViewportProgram::draw`）の結果を、
//! 画面外のテクスチャに描く（`viewport::offscreen`）。大きく描いて縮める（アンチエイリアス）。
//! 保存先は、先にダイアログで決める（描くのに時間がかかるため、取り消しを早く知らせる）。

use std::path::PathBuf;

use iced::widget::{button, column, container, row, text};
use iced::{Alignment, Element, Length};

use crate::canvas::{CanvasAction, CanvasOptions};
use crate::viewport::primitive::ViewportPrimitive;

/// 幅の候補（ピクセル）
pub const WIDTH_PRESETS: [u32; 4] = [1000, 2000, 3000, 4000];
/// なめらかさ（大きく描いて縮める倍率）の候補。1 は、縮めない。
pub const SMOOTHING_PRESETS: [u32; 3] = [1, 2, 3];

/// PNG の書き出しの設定
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PngOptions {
    /// 画像の幅（ピクセル）。高さは、ウィンドウの縦横比から決める。
    pub width: u32,
    /// 大きく描いて縮める倍率（画像が大きいときや、アダプターの限界を超えるときは、自動で下げる）
    pub smoothing: u32,
    /// 背景を透明にするか（`false` なら、分子図の背景色）
    pub transparent: bool,
    /// キャンバスの形と構図。形が `Custom` のときは、`width` ではなく、指定の幅と高さを使う。
    pub canvas: CanvasOptions,
}

impl Default for PngOptions {
    fn default() -> Self {
        Self {
            width: 2000,
            smoothing: 2,
            transparent: false,
            canvas: CanvasOptions::default(),
        }
    }
}

/// 書き出しのダイアログの操作
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PngDialogAction {
    SetWidth(u32),
    SetSmoothing(u32),
    SetTransparent(bool),
    /// キャンバスの形と構図
    Canvas(CanvasAction),
    /// 保存先を選んで、書き出す
    Export,
    Cancel,
}

impl PngOptions {
    /// ダイアログの操作を設定に反映する（`Export` と `Cancel` は、呼び出し側が扱う）。
    /// 候補にない値は無視する。
    pub fn apply(&mut self, action: PngDialogAction) {
        match action {
            PngDialogAction::SetWidth(w) if WIDTH_PRESETS.contains(&w) => self.width = w,
            PngDialogAction::SetSmoothing(f) if SMOOTHING_PRESETS.contains(&f) => {
                self.smoothing = f
            }
            PngDialogAction::SetTransparent(t) => self.transparent = t,
            PngDialogAction::Canvas(c) => self.canvas.apply(c),
            _ => {}
        }
    }
}

/// 書き出しのダイアログ（設定を選んで、「Export...」で保存先の選択へ進む）
pub fn view_png_dialog<'a, Message: Clone + 'a>(
    options: PngOptions,
    on_action: impl Fn(PngDialogAction) -> Message + Copy + 'a,
) -> Element<'a, Message> {
    let chip = |label: String, selected: bool, action: PngDialogAction| {
        button(text(label).size(12))
            .padding([3, 10])
            .style(crate::ui_style::choice_button(selected))
            .on_press(on_action(action))
    };
    let canvas_rows =
        crate::canvas::view_canvas_rows(options.canvas, crate::canvas::chip, move |a| {
            on_action(PngDialogAction::Canvas(a))
        });
    let mut widths = row![text("Width").size(12).width(Length::Fixed(80.0))].spacing(6);
    for w in WIDTH_PRESETS {
        widths = widths.push(chip(
            format!("{w} px"),
            options.width == w,
            PngDialogAction::SetWidth(w),
        ));
    }
    let mut smoothing = row![text("Smoothing").size(12).width(Length::Fixed(80.0))].spacing(6);
    for f in SMOOTHING_PRESETS {
        let label = if f == 1 {
            "Off".to_string()
        } else {
            format!("{f}x")
        };
        smoothing = smoothing.push(chip(
            label,
            options.smoothing == f,
            PngDialogAction::SetSmoothing(f),
        ));
    }
    let background = row![
        text("Background").size(12).width(Length::Fixed(80.0)),
        chip(
            "Scene color".to_string(),
            !options.transparent,
            PngDialogAction::SetTransparent(false),
        ),
        chip(
            "Transparent".to_string(),
            options.transparent,
            PngDialogAction::SetTransparent(true),
        ),
    ]
    .spacing(6);

    let mut body = column![text("Export Image (PNG)").size(14)].spacing(10);
    for r in canvas_rows {
        body = body.push(r);
    }
    // 形が Custom のときは、指定の幅と高さを使うので、幅の候補は出さない
    if options.canvas.shape != crate::canvas::CanvasShape::Custom {
        body = body.push(widths.align_y(Alignment::Center));
    }
    let card = container(
        body.push(smoothing.align_y(Alignment::Center))
            .push(background.align_y(Alignment::Center))
            .push(
            text("Smoothing draws larger and averages down; it is lowered automatically for very large images.")
                .size(11))
            .push(
            row![
                button(text("Export...").size(12))
                    .padding([4, 12])
                    .style(crate::ui_style::accent_button).on_press(on_action(PngDialogAction::Export)),
                button(text("Cancel").size(12))
                    .padding([4, 12])
                    .style(crate::ui_style::secondary_button).on_press(on_action(PngDialogAction::Cancel)),
            ]
            .spacing(8),
        ),
    )
    .padding(16)
    .style(container::rounded_box);

    container(card).center(Length::Fill).into()
}

/// 保存ダイアログを開き、`primitive` を描いて PNG で書き出す。キャンセルは `Ok(None)`。
pub async fn export_png_file(
    suggested_name: Option<String>,
    primitive: ViewportPrimitive,
    width: u32,
    height: u32,
    smoothing: u32,
) -> Result<Option<(String, Option<PathBuf>)>, String> {
    let dialog = rfd::AsyncFileDialog::new()
        .add_filter("PNG Image (*.png)", &["png"])
        .set_file_name(suggested_name.unwrap_or_else(|| "view.png".to_string()));
    let Some(handle) = dialog.save_file().await else {
        return Ok(None);
    };
    let file_name = handle.file_name();

    let (png, _, _) =
        crate::viewport::offscreen::render_png(primitive, width, height, smoothing).await?;

    #[cfg(not(target_arch = "wasm32"))]
    let path = Some(handle.path().to_path_buf());
    #[cfg(target_arch = "wasm32")]
    let path = None;

    handle
        .write(&png)
        .await
        .map_err(|e| format!("Failed to write PNG file: {e}"))?;
    Ok(Some((file_name, path)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dialog_actions_change_only_known_options() {
        let mut o = PngOptions::default();
        assert_eq!((o.width, o.smoothing, o.transparent), (2000, 2, false));
        o.apply(PngDialogAction::SetWidth(4000));
        o.apply(PngDialogAction::SetSmoothing(3));
        o.apply(PngDialogAction::SetTransparent(true));
        assert_eq!((o.width, o.smoothing, o.transparent), (4000, 3, true));
        // 候補にない値、保存・取り消しは、設定を変えない
        let before = o;
        o.apply(PngDialogAction::SetWidth(1234));
        o.apply(PngDialogAction::SetSmoothing(9));
        o.apply(PngDialogAction::Export);
        o.apply(PngDialogAction::Cancel);
        assert_eq!(o, before);
    }

    #[test]
    fn test_dialog_view_builds() {
        let _: Element<'_, PngDialogAction> = view_png_dialog(PngOptions::default(), |a| a);
    }
}
