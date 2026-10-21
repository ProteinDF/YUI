//! 3D分子ビューポート（iced::widget::shader + wgpu ジオメトリインスタンシング）
//!
//! architecture.md 4.1節「ジオメトリインスタンシング」に準拠した ball-and-stick 描画。

use iced::mouse;
use iced::widget::shader::{self, Action};
use iced::{Event, Rectangle};

use crate::context_menu::ContextMenuTarget;
use crate::math::screen_to_world_ray;
use crate::ribbon::build_all_cartoons;
use yui_core::atom_group::{AtomGroup, SecondaryStructureMap};
use yui_core::representation::{Representation, RepresentationKind};
use yui_core::spatial::AtomBvh;
use yui_core::state::{CameraState, Color4, Molecule};

pub mod camera;
pub mod gpu_types;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod headless;
pub mod mesh;
pub mod offscreen;
pub mod pipeline;
pub mod primitive;
pub mod scene;
pub(crate) use camera::*;
pub use gpu_types::*;
pub use pipeline::ViewportId;
pub use primitive::*;
pub use scene::*;

/// ビューポートの対話操作状態
///
/// 視点の持ち主はアプリ（`Yui`）である。ここの `camera` は、ドラッグ中に同じフレームで届く複数のイベントが
/// 古い視点から計算し直されないようにするための、操作中の手元のコピー。
#[derive(Debug, Clone)]
pub struct ViewportState {
    /// 操作中の手元の視点
    pub camera: CameraState,
    /// アプリへ最後に渡した（または、アプリから最後に受け取った）視点。
    /// アプリ側の視点がこれと違うときは、外から視点が変わった（協調編集、連動、Reset Camera など）ので採り入れる。
    pub synced: Option<CameraState>,
    pub is_dragging: bool,
    pub last_cursor_pos: Option<[f32; 2]>,
    /// 視点を構造に合わせて、アプリへ知らせたときの `camera_fit_token`
    pub fit_published_token: u64,
    /// 左ボタンを押した位置（ビューポート内の座標）。クリックかドラッグかの判定に使う。
    pub press_pos: Option<[f32; 2]>,
    /// 押してから、クリックのしきい値を超えて動かしたか（超えたら回転のドラッグ）
    pub drag_started: bool,
    /// 今の修飾キー（離したときのイベントには修飾キーがないため、変化のたびに覚える）
    pub modifiers: iced::keyboard::Modifiers,
    /// 直前のクリックの時刻と位置（ダブルクリックの判定）
    pub last_click: Option<(iced::time::Instant, [f32; 2])>,
}

impl Default for ViewportState {
    fn default() -> Self {
        Self {
            camera: crate::menu_bar::default_camera_state(),
            synced: None,
            is_dragging: false,
            last_cursor_pos: None,
            fit_published_token: 0,
            press_pos: None,
            drag_started: false,
            modifiers: iced::keyboard::Modifiers::default(),
            last_click: None,
        }
    }
}

/// いま描画に使う視点。
///
/// アプリ側の視点（`external`）が、最後にやりとりした視点（`synced`）と同じなら、操作中の手元のコピー（`local`）。
/// 違えば、外から変わったので `external`。まだ一度もやりとりしていなければ `external`。
pub(crate) fn effective_camera<'a>(
    local: &'a CameraState,
    synced: Option<&CameraState>,
    external: &'a CameraState,
) -> &'a CameraState {
    if synced == Some(external) {
        local
    } else {
        external
    }
}

/// 3Dビューポートの Shader Program
/// 3D ペインの、最後に描かれたときの大きさ（分子図ごと）。
///
/// 書き出しの縦横比（`3D pane`）に使う。大きさは、描くときの `bounds` からしか分からず、`draw` は
/// `&self` で呼ばれるので、アトミックな値に入れる（幅と高さの `f32` を 1 つの `u64` に詰める）。
#[derive(Debug, Default)]
pub struct ViewportSizes {
    primary: std::sync::atomic::AtomicU64,
    secondary: std::sync::atomic::AtomicU64,
}

impl ViewportSizes {
    fn slot(&self, id: ViewportId) -> &std::sync::atomic::AtomicU64 {
        match id {
            ViewportId::Primary => &self.primary,
            ViewportId::Secondary => &self.secondary,
        }
    }

    /// 描いた大きさを覚える
    pub fn set(&self, id: ViewportId, width: f32, height: f32) {
        let packed = ((width.to_bits() as u64) << 32) | height.to_bits() as u64;
        self.slot(id)
            .store(packed, std::sync::atomic::Ordering::Relaxed);
    }

    /// 最後に描かれた大きさ（幅、高さ）。まだ描かれていない、または大きさが使えないときは `None`
    pub fn get(&self, id: ViewportId) -> Option<(f32, f32)> {
        let packed = self.slot(id).load(std::sync::atomic::Ordering::Relaxed);
        let (w, h) = (
            f32::from_bits((packed >> 32) as u32),
            f32::from_bits(packed as u32),
        );
        (w.is_finite() && h.is_finite() && w >= 1.0 && h >= 1.0).then_some((w, h))
    }
}

pub struct ViewportProgram<'a, Message> {
    /// どの分子図か（GPU の資源の選択と、視点の変更の通知に使う）
    pub id: ViewportId,
    /// アプリが持つ、この分子図の視点
    pub camera: &'a CameraState,
    pub molecule: &'a Molecule,
    pub atom_group: Option<&'a AtomGroup>,
    pub secondary_structure_map: &'a SecondaryStructureMap,
    pub representations: &'a [Representation],
    pub resolved_interactions: &'a [yui_core::interaction::ResolvedInteraction],
    pub active_model: Option<&'a str>,
    pub model_atom_ranges: &'a [yui_core::atom_group::ModelAtomRange],
    pub surface_batches: &'a crate::surface_render::SurfaceBatches,
    pub background_color: Color4,
    /// 描いた大きさの記録先（書き出しの縦横比に使う）
    pub sizes: &'a ViewportSizes,
    /// 今選ばれている原子（利用者ごと。保存しない）
    pub selection: &'a std::collections::BTreeSet<u32>,
    /// 選択の強調の方法
    pub highlight: crate::selection::HighlightStyle,
    /// 強調の色（UI のパレットから）
    pub selection_color: Color4,
    /// 値が変わると、カメラを表示中の構造に合わせる（構造を開いたとき、Reset Camera など）
    pub camera_fit_token: u64,
    /// この分子図が、構造に合わせる視点をまだアプリへ知らせていないか
    pub fit_pending: bool,
    /// 合わせる対象が、表示中の構造全体ではなく、選択された原子か（選択が空なら、全体）
    pub fit_selection: bool,
    /// 視点が変わったとき（ドラッグ、ホイール、構造に合わせたとき）の通知。第3引数は、構造に合わせた結果か。
    #[allow(clippy::type_complexity)]
    pub on_camera_changed: Option<Box<dyn Fn(ViewportId, CameraState, bool) -> Message + 'a>>,
    /// 左クリック（ほとんど動かさずに離した）の通知
    pub on_click: Option<Box<dyn Fn(crate::selection::ViewportClick) -> Message + 'a>>,
    #[allow(clippy::type_complexity)]
    // 右クリック時の座標およびターゲット通知用コールバッククロージャの型定義を維持
    pub on_right_click: Option<Box<dyn Fn([f32; 2], Option<ContextMenuTarget>) -> Message + 'a>>,
}

impl<'a, Message> ViewportProgram<'a, Message> {
    /// 表示中のモデルの原子に合わせたカメラ（原子がなければ既定のカメラ）
    fn fitted_camera(&self, bounds: Rectangle) -> CameraState {
        let default = crate::menu_bar::default_camera_state();
        let points = if self.fit_selection && !self.selection.is_empty() {
            crate::selection::positions_of(&self.molecule.atoms, self.selection)
        } else {
            fit_target_positions(self.molecule, self.active_model, self.model_atom_ranges)
        };
        let aspect = bounds.width / bounds.height.max(1.0);
        fit_camera_to_points(points, default.fov, aspect).unwrap_or(default)
    }

    /// ビューポート内の位置（`pos`）の下にある原子。右クリックと左クリックで共通。
    fn pick_atom(&self, pos: [f32; 2], bounds: Rectangle, camera: &CameraState) -> Option<AtomHit> {
        let (ray_origin, ray_direction) =
            screen_to_world_ray(pos, [bounds.width, bounds.height], camera);

        let layers = style_layers(
            self.molecule,
            self.atom_group,
            self.representations,
            self.active_model,
            self.model_atom_ranges,
        );
        let candidates = pickable_atoms(&layers);

        let positions: Vec<[f32; 3]> = candidates.iter().map(|(a, _)| a.position).collect();
        let radii: Vec<f32> = candidates.iter().map(|&(_, r)| r).collect();

        let bvh = AtomBvh::build(&positions);
        let (atom_idx, _distance) = bvh.query_ray(ray_origin, ray_direction, Some(&radii), 0.5)?;
        let atom = candidates[atom_idx].0;
        Some(AtomHit {
            id: atom.id,
            element: atom.element.clone(),
            residue_name: atom.residue_name.clone(),
            residue_index: atom.residue_index,
            chain_id: atom.chain_id.clone(),
            position: atom.position,
        })
    }

    /// 左クリックを、アプリへ知らせる（修飾キーとダブルクリックを添える）。通知先がなければ何もしない。
    fn click_action(
        &self,
        state: &mut ViewportState,
        pos: [f32; 2],
        bounds: Rectangle,
    ) -> Option<Action<Message>> {
        let cb = self.on_click.as_ref()?;
        let now = iced::time::Instant::now();
        let double = state.last_click.is_some_and(|(t, p)| {
            crate::selection::is_double_click(now.duration_since(t).as_millis(), p, pos)
        });
        // ダブルクリックの次のクリックを、もう一度ダブルクリックとみなさない
        state.last_click = if double { None } else { Some((now, pos)) };

        let atom =
            self.pick_atom(pos, bounds, &state.camera)
                .map(|hit| crate::selection::ClickedAtom {
                    atom_id: hit.id,
                    chain_id: hit.chain_id,
                    residue_index: hit.residue_index,
                });
        Some(Action::publish(cb(crate::selection::ViewportClick {
            atom,
            shift: state.modifiers.shift(),
            command: state.modifiers.command(),
            double,
        })))
    }

    /// 視点の変更をアプリへ知らせる（通知先がなければ、再描画だけ求める）
    fn camera_action(&self, camera: CameraState, fitted: bool) -> Action<Message> {
        match &self.on_camera_changed {
            Some(cb) => Action::publish(cb(self.id, camera, fitted)),
            None => Action::request_redraw(),
        }
    }
}

/// ピッキングで当たった原子の情報
struct AtomHit {
    id: u32,
    element: String,
    residue_name: String,
    residue_index: i32,
    chain_id: String,
    position: [f32; 3],
}

/// カメラを合わせる対象の原子の座標。
///
/// マルチモデルの構造では、表示中のモデル（`active_model`）の原子だけとする。全モデル表示（`None`）なら全原子。
pub(crate) fn fit_target_positions(
    molecule: &Molecule,
    active_model: Option<&str>,
    model_atom_ranges: &[yui_core::atom_group::ModelAtomRange],
) -> Vec<[f32; 3]> {
    let range = active_model.and_then(|m| model_atom_ranges.iter().find(|r| r.model_key == m));
    molecule
        .atoms
        .iter()
        .filter(|a| match (active_model, range) {
            (None, _) => true,
            (Some(_), Some(r)) => r.contains(a.id),
            (Some(_), None) => false,
        })
        .map(|a| a.position)
        .collect()
}

impl<'a, Message: Clone + 'static> shader::Program<Message> for ViewportProgram<'a, Message> {
    type State = ViewportState;
    type Primitive = ViewportPrimitive;

    fn draw(
        &self,
        state: &Self::State,
        _cursor: mouse::Cursor,
        bounds: Rectangle,
    ) -> Self::Primitive {
        self.sizes.set(self.id, bounds.width, bounds.height);
        let mut sphere_instances = Vec::new();
        let mut stick_instances = Vec::new();
        let mut ribbon_vertices = Vec::new();
        let mut ribbon_indices = Vec::new();
        let mut interaction_instances = Vec::new();

        let has_cartoon = self
            .representations
            .iter()
            .any(|r| r.kind == RepresentationKind::Cartoon && r.visible);

        // 1. カートゥーン（リボン）メッシュの生成
        if let Some(atom_group) = self.atom_group
            && has_cartoon
        {
            let mesh = build_all_cartoons(
                atom_group,
                self.representations,
                self.secondary_structure_map,
                self.active_model,
            );
            ribbon_vertices = mesh.vertices;
            ribbon_indices = mesh.indices;
        }

        // 2. 相互作用（破線円柱）インスタンスの生成
        let interaction_lines = crate::interaction_render::build_interaction_lines_from_resolved(
            self.atom_group,
            self.resolved_interactions,
            self.representations,
            self.active_model,
            self.model_atom_ranges,
        );
        for line in interaction_lines {
            interaction_instances.push(InstanceRaw::from_cylinder(
                line.start,
                line.end,
                crate::user_settings::startup_render().interaction_radius,
                line.color,
            ));
        }

        // 3. 球・スティックの生成（表現ごとに独立して描き、重ねる）
        let layers = style_layers(
            self.molecule,
            self.atom_group,
            self.representations,
            self.active_model,
            self.model_atom_ranges,
        );
        for layer in &layers {
            for atom in &layer.atoms {
                let Some(visual_radius) = layer.style.sphere_radius(atom.radius) else {
                    continue;
                };
                let color = layer.colors.get(&atom.id).copied().unwrap_or(atom.color);
                sphere_instances.push(InstanceRaw::from_sphere(
                    atom.position,
                    visual_radius,
                    color,
                ));
            }
            stick_instances.extend(generate_styled_sticks(
                &self.molecule.bonds,
                &layer.atoms,
                layer.style,
                &layer.colors,
            ));
        }

        // 4. 選択の強調（選ばれている原子ごとに、描いた球と同じ大きさの球の位置）
        let selection_instances: Vec<InstanceRaw> =
            if self.highlight == crate::selection::HighlightStyle::Off {
                Vec::new()
            } else {
                pickable_atoms(&layers)
                    .into_iter()
                    .filter(|(a, _)| self.selection.contains(&a.id))
                    .map(|(a, radius)| {
                        InstanceRaw::from_sphere(a.position, radius, self.selection_color)
                    })
                    .collect()
            };

        let camera = if self.fit_pending {
            self.fitted_camera(bounds)
        } else {
            effective_camera(&state.camera, state.synced.as_ref(), self.camera).clone()
        };

        ViewportPrimitive {
            id: self.id,
            camera,
            background_color: self.background_color,
            sphere_instances,
            stick_instances,
            ribbon_vertices,
            ribbon_indices,
            surface_batches: self.surface_batches.clone(),
            interaction_instances,
            selection_instances,
            highlight: self.highlight,
            draw_background: true,
            ssao_strength: crate::user_settings::startup_render().ssao_strength,
            ssao_radius: crate::user_settings::startup_render().ssao_radius,
        }
    }

    fn update(
        &self,
        state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        if self.fit_pending {
            // 構造に合わせる。結果をアプリへ知らせ、アプリが受け取るまでの間は、繰り返し知らせない。
            if state.fit_published_token != self.camera_fit_token {
                state.fit_published_token = self.camera_fit_token;
                let fitted = self.fitted_camera(bounds);
                state.camera = fitted.clone();
                state.synced = Some(fitted.clone());
                state.is_dragging = false;
                state.last_cursor_pos = None;
                return Some(self.camera_action(fitted, true));
            }
        } else if state.synced.as_ref() != Some(self.camera) {
            // アプリ側の視点が外から変わった（協調編集、視点の連動、ワークスペースの読み込みなど）
            state.camera = self.camera.clone();
            state.synced = Some(self.camera.clone());
        }

        let is_over = cursor.position_over(bounds).is_some();

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) if is_over => {
                if let (Some(pos), Some(cursor_pos)) =
                    (cursor.position_in(bounds), cursor.position())
                {
                    let hit = self.pick_atom([pos.x, pos.y], bounds, &state.camera);

                    // スタック内でのメニューカード配置用座標（メニューバーのオフセット30pxを考慮）
                    let menu_screen_pos = [cursor_pos.x, (cursor_pos.y - 30.0).max(0.0)];

                    if let Some(hit_atom) = hit {
                        let atom_name = find_atom_name(self.atom_group, hit_atom.id)
                            .unwrap_or_else(|| hit_atom.element.clone());
                        let target = ContextMenuTarget {
                            atom_id: hit_atom.id as usize,
                            element: hit_atom.element,
                            atom_name,
                            residue_name: hit_atom.residue_name,
                            residue_id: Some(hit_atom.residue_index),
                            chain_id: hit_atom.chain_id,
                            position: hit_atom.position,
                        };
                        if let Some(ref cb) = self.on_right_click {
                            return Some(Action::publish(cb(menu_screen_pos, Some(target))));
                        }
                    } else if let Some(ref cb) = self.on_right_click {
                        return Some(Action::publish(cb(menu_screen_pos, None)));
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) if is_over => {
                if let Some(pos) = cursor.position_in(bounds) {
                    state.is_dragging = true;
                    state.press_pos = Some([pos.x, pos.y]);
                    state.drag_started = false;
                    state.last_cursor_pos = Some([pos.x, pos.y]);
                    // 左クリック操作開始時に開いているコンテキストメニューを閉じる
                    if let Some(ref cb) = self.on_right_click {
                        return Some(Action::publish(cb([0.0, 0.0], None)));
                    }
                    return None;
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.is_dragging {
                    state.is_dragging = false;
                    state.last_cursor_pos = None;
                    let press = state.press_pos.take();
                    let dragged = std::mem::take(&mut state.drag_started);
                    // ほとんど動かさずに離したら、クリック（原子を選ぶ）。動かしていれば、回転のドラッグだった。
                    if let (Some(down), Some(up), false) =
                        (press, cursor.position_in(bounds), dragged)
                    {
                        let up = [up.x, up.y];
                        if crate::selection::is_click(down, up) {
                            return self.click_action(state, up, bounds);
                        }
                    }
                    return None;
                }
            }
            Event::Keyboard(iced::keyboard::Event::ModifiersChanged(modifiers)) => {
                state.modifiers = *modifiers;
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) if state.is_dragging => {
                if let Some(pos) = cursor.position_in(bounds)
                    && let Some(last) = state.last_cursor_pos
                {
                    // 押した位置からしきい値以内の動きは、クリックの手ぶれとして、視点を動かさない
                    if !state.drag_started
                        && let Some(down) = state.press_pos
                    {
                        if crate::selection::is_click(down, [pos.x, pos.y]) {
                            return None;
                        }
                        state.drag_started = true;
                    }
                    let dx = pos.x - last[0];
                    let dy = pos.y - last[1];

                    // 修飾キーで動きを変える: なし = 回転、Shift = 平行移動、Ctrl / Cmd = 拡大・縮小
                    match DragMode::from_modifiers(state.modifiers) {
                        DragMode::Orbit => orbit_camera(&mut state.camera, dx * 0.01, dy * 0.01),
                        DragMode::Pan => pan_camera(&mut state.camera, dx, dy, bounds.height),
                        DragMode::Zoom => zoom_camera(&mut state.camera, drag_zoom_factor(dy)),
                    }
                    state.last_cursor_pos = Some([pos.x, pos.y]);
                    state.synced = Some(state.camera.clone());
                    return Some(self.camera_action(state.camera.clone(), false));
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) if is_over => {
                let zoom_factor = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => 1.0 - y * 0.1,
                    mouse::ScrollDelta::Pixels { y, .. } => 1.0 - y * 0.005,
                };
                zoom_camera(&mut state.camera, zoom_factor.clamp(0.2, 5.0));
                state.synced = Some(state.camera.clone());
                return Some(self.camera_action(state.camera.clone(), false));
            }
            _ => {}
        }

        None
    }
}
