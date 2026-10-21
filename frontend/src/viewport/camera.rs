use crate::math::vec3_normalize;
use std::f32::consts::PI;
use yui_core::state::CameraState;

/// 注視点からカメラまでの距離の下限（Å）
pub(crate) const MIN_CAMERA_DISTANCE: f32 = 2.0;
/// 注視点からカメラまでの距離の上限（Å）。大きな構造（例: 1AON、約58,000原子）に合わせても届くようにする。
pub(crate) const MAX_CAMERA_DISTANCE: f32 = 5000.0;
/// 投影の near（Å）
pub(crate) const NEAR_PLANE: f32 = 0.1;
/// 投影の far の下限（Å）
const MIN_FAR_PLANE: f32 = 1000.0;
/// 構造に合わせるとき、原子の中心に足す余白（Å）。原子の球の大きさの分。
const FIT_ATOM_PAD: f32 = 2.0;
/// 構造に合わせるとき、外接球の半径に掛ける余白の倍率
const FIT_MARGIN: f32 = 1.1;

/// カメラ（と、描画領域の縦横比）から、ワールド座標をクリップ座標にする行列（列優先）。
/// 描画（`primitive.rs`）と SVG の書き出しで同じものを使う。
pub(crate) fn view_proj_matrix(cam: &CameraState, aspect: f32) -> crate::math::Mat4 {
    let view = crate::math::look_at_rh(cam.eye, cam.target, cam.up);
    let proj =
        crate::math::perspective_rh(cam.fov.to_radians(), aspect, NEAR_PLANE, far_plane_for(cam));
    crate::math::mat4_mul(&proj, &view)
}

/// 投影の far を、注視点までの距離から決める。
///
/// 構造に合わせたカメラでは、構造の外接球は注視点のまわり（距離 ± 半径）に収まり、
/// 半径は距離より小さいので、距離の2倍あれば構造の奥まで描ける。ズームアウトの余地を見て4倍とする。
pub(crate) fn far_plane_for(cam: &CameraState) -> f32 {
    let d = crate::math::vec3_sub(cam.eye, cam.target);
    let dist = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    (dist * 4.0).max(MIN_FAR_PLANE)
}

/// 原子の座標の集まりに合わせたカメラを求める（構造全体が分子図に収まる位置）。
///
/// - 注視点: 座標の範囲（バウンディングボックス）の中心
/// - 距離: 注視点を中心とする外接球（原子の余白を含む）に余白の倍率を掛けた球が、
///   縦・横のうち狭い方の画角に収まる距離
/// - 向き: 既定のカメラと同じ（+z 側から -z 方向を見る、up は +y）
///
/// 座標が1つもなければ `None` を返す（呼び出し側で既定のカメラにする）。
pub(crate) fn fit_camera_to_points(
    points: impl IntoIterator<Item = [f32; 3]>,
    fov_deg: f32,
    aspect: f32,
) -> Option<CameraState> {
    let points: Vec<[f32; 3]> = points.into_iter().collect();
    let first = *points.first()?;
    let mut min = first;
    let mut max = first;
    for p in &points {
        for k in 0..3 {
            min[k] = min[k].min(p[k]);
            max[k] = max[k].max(p[k]);
        }
    }
    let center = [
        (min[0] + max[0]) * 0.5,
        (min[1] + max[1]) * 0.5,
        (min[2] + max[2]) * 0.5,
    ];
    let max_dist_sq = points
        .iter()
        .map(|p| {
            let d = crate::math::vec3_sub(*p, center);
            d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
        })
        .fold(0.0_f32, f32::max);
    let radius = (max_dist_sq.sqrt() + FIT_ATOM_PAD) * FIT_MARGIN;

    let half_v = (fov_deg.to_radians() * 0.5).clamp(1e-3, PI * 0.49);
    let aspect = if aspect.is_finite() && aspect > 0.0 {
        aspect
    } else {
        1.0
    };
    let half_h = (half_v.tan() * aspect).atan();
    let half = half_v.min(half_h);
    let dist = (radius / half.sin()).clamp(MIN_CAMERA_DISTANCE, MAX_CAMERA_DISTANCE);

    Some(CameraState {
        eye: [center[0], center[1], center[2] + dist],
        target: center,
        up: [0.0, 1.0, 0.0],
        fov: fov_deg,
    })
}

/// カメラの軌道回転
pub(crate) fn orbit_camera(cam: &mut CameraState, yaw: f32, pitch: f32) {
    let eye = cam.eye;
    let target = cam.target;
    let dx = eye[0] - target[0];
    let dy = eye[1] - target[1];
    let dz = eye[2] - target[2];

    let radius = (dx * dx + dy * dy + dz * dz).sqrt();
    if radius < 1e-4 {
        return;
    }

    let mut theta = dz.atan2(dx); // 方位角
    let mut phi = (dy / radius).asin(); // 仰角

    theta += yaw;
    phi = (phi + pitch).clamp(-PI * 0.48, PI * 0.48);

    cam.eye = [
        target[0] + radius * phi.cos() * theta.cos(),
        target[1] + radius * phi.sin(),
        target[2] + radius * phi.cos() * theta.sin(),
    ];
}

/// カメラのズーム（距離伸縮）
pub(crate) fn zoom_camera(cam: &mut CameraState, factor: f32) {
    let eye = cam.eye;
    let target = cam.target;
    let dx = eye[0] - target[0];
    let dy = eye[1] - target[1];
    let dz = eye[2] - target[2];

    let new_radius = ((dx * dx + dy * dy + dz * dz).sqrt() * factor)
        .clamp(MIN_CAMERA_DISTANCE, MAX_CAMERA_DISTANCE);
    let dir = vec3_normalize([dx, dy, dz]);

    cam.eye = [
        target[0] + dir[0] * new_radius,
        target[1] + dir[1] * new_radius,
        target[2] + dir[2] * new_radius,
    ];
}

/// 左ドラッグの動き（修飾キーで決まる）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DragMode {
    /// 注視点のまわりに回す（修飾キーなし）
    Orbit,
    /// 画面に平行に動かす（Shift）
    Pan,
    /// 拡大・縮小する（Ctrl。macOS は Cmd）
    Zoom,
}

impl DragMode {
    /// 修飾キーから、ドラッグの動きを決める。Shift と Ctrl / Cmd の両方なら、Shift（平行移動）を優先する。
    pub(crate) fn from_modifiers(modifiers: iced::keyboard::Modifiers) -> Self {
        if modifiers.shift() {
            Self::Pan
        } else if modifiers.command() {
            Self::Zoom
        } else {
            Self::Orbit
        }
    }
}

/// 画面に平行な、カメラの平行移動。`dx` / `dy` はマウスの移動量（ピクセル、右・下が正）、
/// `height_px` は分子図の高さ（ピクセル）。
///
/// 注視点の距離にある面で、マウスの動きと同じだけ分子が動く（右へドラッグすると分子が右へ動く）よう、
/// 視点と注視点を一緒に動かす。向きと距離は変わらない。
pub(crate) fn pan_camera(cam: &mut CameraState, dx: f32, dy: f32, height_px: f32) {
    if !(height_px.is_finite() && height_px > 0.0) {
        return;
    }
    let to_target = crate::math::vec3_sub(cam.target, cam.eye);
    let distance =
        (to_target[0] * to_target[0] + to_target[1] * to_target[1] + to_target[2] * to_target[2])
            .sqrt();
    if distance < 1e-4 {
        return;
    }
    let forward = vec3_normalize(to_target);
    let right = vec3_normalize(crate::math::vec3_cross(forward, cam.up));
    let up = crate::math::vec3_cross(right, forward);

    // 注視点の距離で、分子図の高さ全体が見える長さ（Å）を、ピクセルへ割る
    let world_per_px = 2.0 * distance * (cam.fov.to_radians() * 0.5).tan() / height_px;
    let shift = [
        (-dx * right[0] + dy * up[0]) * world_per_px,
        (-dx * right[1] + dy * up[1]) * world_per_px,
        (-dx * right[2] + dy * up[2]) * world_per_px,
    ];
    for (k, d) in shift.iter().enumerate() {
        cam.eye[k] += d;
        cam.target[k] += d;
    }
}

/// ドラッグの縦の動き（ピクセル、下が正）から、ズームの倍率（距離に掛ける値）を求める。
/// 下へドラッグすると遠ざかり（縮小）、上へドラッグすると近づく（拡大）。
pub(crate) fn drag_zoom_factor(dy: f32) -> f32 {
    (dy * 0.01).exp().clamp(0.2, 5.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{vec3_cross, vec3_normalize, vec3_sub};

    /// 点がカメラの縦・横の画角に収まっているか
    fn inside_view(cam: &CameraState, aspect: f32, p: [f32; 3]) -> bool {
        let f = vec3_normalize(vec3_sub(cam.target, cam.eye));
        let r = vec3_normalize(vec3_cross(f, cam.up));
        let u = vec3_cross(r, f);
        let d = vec3_sub(p, cam.eye);
        let z = d[0] * f[0] + d[1] * f[1] + d[2] * f[2];
        if z <= 0.0 {
            return false;
        }
        let x = d[0] * r[0] + d[1] * r[1] + d[2] * r[2];
        let y = d[0] * u[0] + d[1] * u[1] + d[2] * u[2];
        let tan_v = (cam.fov.to_radians() * 0.5).tan();
        let tan_h = tan_v * aspect;
        (y / z).abs() <= tan_v && (x / z).abs() <= tan_h
    }

    fn dist(cam: &CameraState) -> f32 {
        let d = vec3_sub(cam.eye, cam.target);
        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
    }

    fn box_points(center: [f32; 3], half: [f32; 3]) -> Vec<[f32; 3]> {
        let mut pts = Vec::new();
        for sx in [-1.0, 1.0] {
            for sy in [-1.0, 1.0] {
                for sz in [-1.0, 1.0] {
                    pts.push([
                        center[0] + sx * half[0],
                        center[1] + sy * half[1],
                        center[2] + sz * half[2],
                    ]);
                }
            }
        }
        pts
    }

    #[test]
    fn test_fit_target_is_center_and_all_points_visible() {
        let pts = box_points([-10.0, 5.0, 30.0], [20.0, 8.0, 12.0]);
        for aspect in [0.3, 1.0, 1.8, 4.0] {
            let cam = fit_camera_to_points(pts.clone(), 45.0, aspect).unwrap();
            assert_eq!(cam.target, [-10.0, 5.0, 30.0]);
            assert_eq!(cam.up, [0.0, 1.0, 0.0]);
            for p in &pts {
                assert!(inside_view(&cam, aspect, *p), "aspect={aspect} p={p:?}");
            }
        }
    }

    #[test]
    fn test_fit_narrow_view_needs_more_distance() {
        // 縦長の分子図（横が狭い）では、横の画角に合わせて遠くなる
        let pts = box_points([0.0, 0.0, 0.0], [15.0, 15.0, 15.0]);
        let wide = fit_camera_to_points(pts.clone(), 45.0, 2.0).unwrap();
        let narrow = fit_camera_to_points(pts, 45.0, 0.4).unwrap();
        assert!(dist(&narrow) > dist(&wide));
    }

    #[test]
    fn test_fit_single_point_and_empty() {
        assert!(fit_camera_to_points(Vec::new(), 45.0, 1.0).is_none());

        let cam = fit_camera_to_points(vec![[1.0, 2.0, 3.0]], 45.0, 1.0).unwrap();
        assert_eq!(cam.target, [1.0, 2.0, 3.0]);
        assert!(cam.eye.iter().all(|v| v.is_finite()));
        assert!(dist(&cam) >= MIN_CAMERA_DISTANCE);
    }

    #[test]
    fn test_fit_large_structure_is_not_clipped() {
        // 半径 150 Å 程度の大きな構造でも、ズームの上限や far で切れない
        let pts = box_points([100.0, -50.0, 20.0], [150.0, 150.0, 150.0]);
        let aspect = 1.5;
        let cam = fit_camera_to_points(pts.clone(), 45.0, aspect).unwrap();
        let d = dist(&cam);
        assert!(d < MAX_CAMERA_DISTANCE, "distance {d} hit the zoom limit");
        let far = far_plane_for(&cam);
        for p in &pts {
            assert!(inside_view(&cam, aspect, *p));
            let to_p = vec3_sub(*p, cam.eye);
            let len = (to_p[0] * to_p[0] + to_p[1] * to_p[1] + to_p[2] * to_p[2]).sqrt();
            assert!(len < far, "point at {len} beyond far {far}");
        }
    }

    #[test]
    fn test_drag_mode_from_modifiers() {
        use iced::keyboard::Modifiers;
        assert_eq!(
            DragMode::from_modifiers(Modifiers::empty()),
            DragMode::Orbit
        );
        assert_eq!(DragMode::from_modifiers(Modifiers::SHIFT), DragMode::Pan);
        // 拡大・縮小のキーは、macOS では Cmd、ほかでは Ctrl（iced の `command()`）
        let (zoom_key, other_key) = if cfg!(target_os = "macos") {
            (Modifiers::LOGO, Modifiers::CTRL)
        } else {
            (Modifiers::CTRL, Modifiers::LOGO)
        };
        assert_eq!(DragMode::from_modifiers(zoom_key), DragMode::Zoom);
        assert_eq!(DragMode::from_modifiers(other_key), DragMode::Orbit);
        assert_eq!(
            DragMode::from_modifiers(Modifiers::SHIFT | zoom_key),
            DragMode::Pan
        );
        // Alt だけでは、何も変えない
        assert_eq!(DragMode::from_modifiers(Modifiers::ALT), DragMode::Orbit);
    }

    #[test]
    fn test_pan_moves_eye_and_target_together_and_keeps_direction_and_distance() {
        let mut cam = CameraState::default();
        let before = cam.clone();
        pan_camera(&mut cam, 30.0, -12.0, 600.0);
        assert!((dist(&cam) - dist(&before)).abs() < 1e-3);
        let dir = |c: &CameraState| vec3_normalize(vec3_sub(c.target, c.eye));
        let (a, b) = (dir(&before), dir(&cam));
        assert!(
            (a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5 && (a[2] - b[2]).abs() < 1e-5
        );
        assert_ne!(cam.target, before.target);
    }

    #[test]
    fn test_pan_follows_the_mouse_and_scales_with_distance() {
        // 既定のカメラ: +z から -z を見る、右 = +x、上 = +y。右・下へドラッグすると、分子も右・下へ動く
        // （= カメラが左・上へ動く）。
        let mut cam = CameraState::default();
        pan_camera(&mut cam, 10.0, 0.0, 600.0);
        assert!(cam.target[0] < 0.0 && cam.target[1].abs() < 1e-5);
        let mut cam = CameraState::default();
        pan_camera(&mut cam, 0.0, 10.0, 600.0);
        assert!(cam.target[1] > 0.0 && cam.target[0].abs() < 1e-5);

        // 分子図の高さと同じだけ動かすと、注視点の距離で見える高さの分だけ動く
        let mut cam = CameraState::default();
        pan_camera(&mut cam, 0.0, 600.0, 600.0);
        let visible = 2.0 * 100.0 * (cam.fov.to_radians() * 0.5).tan();
        assert!(
            (cam.target[1] - visible).abs() < 1e-2,
            "{} vs {visible}",
            cam.target[1]
        );

        // 距離が 2 倍なら、同じマウスの動きで 2 倍動く
        let mut near = CameraState::default();
        let mut far = CameraState {
            eye: [0.0, 0.0, 200.0],
            ..CameraState::default()
        };
        pan_camera(&mut near, 10.0, 0.0, 600.0);
        pan_camera(&mut far, 10.0, 0.0, 600.0);
        assert!((far.target[0] / near.target[0] - 2.0).abs() < 1e-4);
    }

    #[test]
    fn test_pan_ignores_a_degenerate_viewport() {
        let mut cam = CameraState::default();
        let before = cam.clone();
        pan_camera(&mut cam, 10.0, 10.0, 0.0);
        pan_camera(&mut cam, 10.0, 10.0, f32::NAN);
        assert_eq!(cam.eye, before.eye);
        assert_eq!(cam.target, before.target);
    }

    #[test]
    fn test_drag_zoom_factor_direction_and_limits() {
        assert!(drag_zoom_factor(10.0) > 1.0); // 下へ: 遠ざかる
        assert!(drag_zoom_factor(-10.0) < 1.0); // 上へ: 近づく
        assert!((drag_zoom_factor(0.0) - 1.0).abs() < 1e-6);
        assert_eq!(drag_zoom_factor(1000.0), 5.0);
        assert_eq!(drag_zoom_factor(-1000.0), 0.2);
    }
}
