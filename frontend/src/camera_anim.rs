//! カメラのアニメーション（architecture.md 8.11節）。
//!
//! 自動回転（spin）、首振り（rock）、視点から視点への滑らかな移動（move）の、カメラの計算。
//! 画面や時計を使わずに試せる純粋な処理だけを置く。いつ動かすか（タイマー、止める条件）は `app/camera_anim_ops.rs`。

use yui_core::state::CameraState;

use crate::math::{
    vec3_add, vec3_cross, vec3_dot, vec3_length, vec3_normalize, vec3_scale, vec3_sub,
};
use crate::viewport::orbit_camera;

/// メニューやコマンドからのカメラの操作
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraAction {
    /// 自動回転のオン・オフ
    ToggleSpin,
    /// 首振りのオン・オフ
    ToggleRock,
    /// 自動回転の速さを、次の選択肢へ
    CycleSpinSpeed,
    /// 選ばれている原子に、滑らかに合わせる
    ZoomToSelection,
}

/// 自動回転の速さの選択肢（度 / 秒）
pub const SPIN_SPEEDS: [f32; 4] = [10.0, 20.0, 40.0, 80.0];
/// 自動回転の速さの既定値（度 / 秒）
pub const DEFAULT_SPIN_SPEED: f32 = 20.0;
/// 首振りの振れ幅（片側、度）
pub const ROCK_AMPLITUDE_DEG: f32 = 30.0;
/// 首振りの周期（秒。左右に 1 往復する時間）
pub const ROCK_PERIOD_S: f32 = 4.0;
/// 視点の滑らかな移動にかける時間（秒）
pub const MOVE_DURATION_S: f32 = 0.4;
/// 1 回の更新で進める時間の上限（秒）。長く止まったあとに、一気に進めないための安全弁。
pub const MAX_STEP_S: f32 = 0.1;

/// 自動回転の速さを、選択肢の次へ（最後の次は最初）
pub fn next_spin_speed(current: f32) -> f32 {
    let pos = SPIN_SPEEDS.iter().position(|s| (*s - current).abs() < 1e-3);
    match pos {
        Some(i) => SPIN_SPEEDS[(i + 1) % SPIN_SPEEDS.len()],
        None => DEFAULT_SPIN_SPEED,
    }
}

/// 0..1 を、始まりと終わりがゆっくりな 0..1 にする（smoothstep）
pub fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 単位ベクトルどうしの球面補間。向きが（ほぼ）反対のときは、途中で注視点を横切らないよう、垂直な方向を通る。
pub fn slerp(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let dot = vec3_dot(a, b).clamp(-1.0, 1.0);
    if dot > 0.9995 {
        // ほぼ同じ向き: 線形補間で十分
        return vec3_normalize(vec3_add(vec3_scale(a, 1.0 - t), vec3_scale(b, t)));
    }
    if dot < -0.9995 {
        // ほぼ反対向き: a に垂直な向き n を通る半円
        let axis = if a[0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        let n = vec3_normalize(vec3_cross(a, axis));
        let angle = std::f32::consts::PI * t;
        return vec3_add(vec3_scale(a, angle.cos()), vec3_scale(n, angle.sin()));
    }
    let theta = dot.acos();
    let sin_theta = theta.sin();
    let wa = ((1.0 - t) * theta).sin() / sin_theta;
    let wb = (t * theta).sin() / sin_theta;
    vec3_add(vec3_scale(a, wa), vec3_scale(b, wb))
}

/// 2つの視点の間の視点（`t` は 0..1。0 で `from`、1 で `to`）。
///
/// 注視点・距離・向きを分けて補間するので、回り込みが不自然にならない。
/// - 注視点: 線形補間
/// - 注視点からの距離: 対数で補間（拡大・縮小の見え方が一定の速さになる）
/// - 注視点から視点への向き、up の向き: 球面補間（距離が 0 に近づいて注視点を横切ることがない）
/// - 画角: 線形補間
pub fn interpolate_camera(from: &CameraState, to: &CameraState, t: f32) -> CameraState {
    let t = t.clamp(0.0, 1.0);
    if t <= 0.0 {
        return from.clone();
    }
    if t >= 1.0 {
        return to.clone();
    }
    let lerp = |a: f32, b: f32| a + (b - a) * t;
    let target = [
        lerp(from.target[0], to.target[0]),
        lerp(from.target[1], to.target[1]),
        lerp(from.target[2], to.target[2]),
    ];

    let offset_from = vec3_sub(from.eye, from.target);
    let offset_to = vec3_sub(to.eye, to.target);
    let (len_from, len_to) = (vec3_length(offset_from), vec3_length(offset_to));
    if len_from < 1e-6 || len_to < 1e-6 {
        // 視点と注視点が重なっていて、向きが決まらない: 終わりの視点へ
        return to.clone();
    }
    let distance = (lerp(len_from.ln(), len_to.ln())).exp();
    let dir = slerp(
        vec3_scale(offset_from, 1.0 / len_from),
        vec3_scale(offset_to, 1.0 / len_to),
        t,
    );
    let up = {
        let (ua, ub) = (vec3_normalize(from.up), vec3_normalize(to.up));
        let u = slerp(ua, ub, t);
        if vec3_length(u) < 1e-6 { to.up } else { u }
    };

    CameraState {
        eye: vec3_add(target, vec3_scale(dir, distance)),
        target,
        up,
        fov: lerp(from.fov, to.fov),
    }
}

/// 首振りの、開始時の向きからの回転角（度）。振れ幅 `amplitude_deg`、周期 `period_s` の正弦波。
pub fn rock_offset_deg(elapsed_s: f32, amplitude_deg: f32, period_s: f32) -> f32 {
    if period_s <= 0.0 {
        return 0.0;
    }
    amplitude_deg * (std::f32::consts::TAU * elapsed_s / period_s).sin()
}

/// 動いているアニメーションの種類
#[derive(Debug, Clone, PartialEq)]
pub enum AnimKind {
    /// 画面の縦軸（ドラッグの左右と同じ軸）のまわりを、一定の速さで回し続ける
    Spin { deg_per_sec: f32 },
    /// 開始時の視点（`base`）を中心に、左右に往復する
    Rock { base: CameraState, elapsed: f32 },
    /// 視点から視点へ、`duration` 秒で滑らかに動く
    Move {
        from: CameraState,
        to: CameraState,
        elapsed: f32,
        duration: f32,
    },
}

/// 1 つの分子図のカメラのアニメーション
#[derive(Debug, Clone, PartialEq)]
pub struct CameraAnimation {
    pub kind: AnimKind,
}

impl CameraAnimation {
    pub fn spin(deg_per_sec: f32) -> Self {
        Self {
            kind: AnimKind::Spin { deg_per_sec },
        }
    }

    /// 今の視点を中心に、首振りを始める
    pub fn rock(base: CameraState) -> Self {
        Self {
            kind: AnimKind::Rock { base, elapsed: 0.0 },
        }
    }

    pub fn move_between(from: CameraState, to: CameraState) -> Self {
        Self {
            kind: AnimKind::Move {
                from,
                to,
                elapsed: 0.0,
                duration: MOVE_DURATION_S,
            },
        }
    }

    pub fn is_spin(&self) -> bool {
        matches!(self.kind, AnimKind::Spin { .. })
    }

    pub fn is_rock(&self) -> bool {
        matches!(self.kind, AnimKind::Rock { .. })
    }

    /// 視点を使い続ける動き（回転と首振り）か。移動は、決まった視点へ行って終わる。
    pub fn is_continuous(&self) -> bool {
        !matches!(self.kind, AnimKind::Move { .. })
    }

    /// `dt` 秒だけ進めた視点と、終わったか（移動だけが終わる）を返す。
    /// 回転は今の視点（`current`）から続けるので、外から視点が変わっても、そこから回る。
    pub fn advance(&mut self, current: &CameraState, dt: f32) -> (CameraState, bool) {
        let dt = dt.clamp(0.0, MAX_STEP_S);
        match &mut self.kind {
            AnimKind::Spin { deg_per_sec } => {
                let mut cam = current.clone();
                orbit_camera(&mut cam, (*deg_per_sec * dt).to_radians(), 0.0);
                (cam, false)
            }
            AnimKind::Rock { base, elapsed } => {
                *elapsed += dt;
                let mut cam = base.clone();
                let yaw = rock_offset_deg(*elapsed, ROCK_AMPLITUDE_DEG, ROCK_PERIOD_S);
                orbit_camera(&mut cam, yaw.to_radians(), 0.0);
                (cam, false)
            }
            AnimKind::Move {
                from,
                to,
                elapsed,
                duration,
            } => {
                *elapsed += dt;
                let t = if *duration > 0.0 {
                    *elapsed / *duration
                } else {
                    1.0
                };
                (
                    interpolate_camera(from, to, smoothstep(t)),
                    t >= 1.0 || from == to,
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam(eye: [f32; 3], target: [f32; 3]) -> CameraState {
        CameraState {
            eye,
            target,
            up: [0.0, 1.0, 0.0],
            fov: 45.0,
        }
    }

    /// 注視点から見た視点の方位角（度）。`orbit_camera` が回す軸と同じ。
    fn azimuth_deg(c: &CameraState) -> f32 {
        let d = vec3_sub(c.eye, c.target);
        d[2].atan2(d[0]).to_degrees()
    }

    fn angle_diff(a: f32, b: f32) -> f32 {
        let mut d = (a - b) % 360.0;
        if d > 180.0 {
            d -= 360.0;
        }
        if d < -180.0 {
            d += 360.0;
        }
        d
    }

    fn dist(c: &CameraState) -> f32 {
        vec3_length(vec3_sub(c.eye, c.target))
    }

    fn assert_cam_eq(a: &CameraState, b: &CameraState) {
        for k in 0..3 {
            assert!((a.eye[k] - b.eye[k]).abs() < 1e-3, "eye {a:?} {b:?}");
            assert!((a.target[k] - b.target[k]).abs() < 1e-3, "target");
            assert!((a.up[k] - b.up[k]).abs() < 1e-3, "up");
        }
        assert!((a.fov - b.fov).abs() < 1e-3);
    }

    #[test]
    fn test_spin_angle_is_speed_times_time() {
        let start = cam([30.0, 5.0, 40.0], [1.0, 2.0, 3.0]);
        let mut anim = CameraAnimation::spin(20.0);
        let mut c = start.clone();
        // 0.05 秒 × 60 回 = 3 秒で、20 度/秒 → 60 度
        for _ in 0..60 {
            c = anim.advance(&c, 0.05).0;
        }
        let turned = angle_diff(azimuth_deg(&c), azimuth_deg(&start)).abs();
        assert!((turned - 60.0).abs() < 0.05, "turned {turned}");
        // 距離と高さ（仰角）は変わらない
        assert!((dist(&c) - dist(&start)).abs() < 1e-3);
        assert!((c.eye[1] - c.target[1] - (start.eye[1] - start.target[1])).abs() < 1e-3);
    }

    #[test]
    fn test_spin_does_not_depend_on_how_time_is_split() {
        let start = cam([0.0, 0.0, 50.0], [0.0, 0.0, 0.0]);
        let (mut a, mut b) = (start.clone(), start.clone());
        let (mut sa, mut sb) = (CameraAnimation::spin(40.0), CameraAnimation::spin(40.0));
        for _ in 0..10 {
            a = sa.advance(&a, 0.1).0;
        }
        for _ in 0..100 {
            b = sb.advance(&b, 0.01).0;
        }
        assert!(angle_diff(azimuth_deg(&a), azimuth_deg(&b)).abs() < 0.05);
    }

    #[test]
    fn test_spin_step_is_capped_after_a_stall() {
        let start = cam([0.0, 0.0, 50.0], [0.0, 0.0, 0.0]);
        let mut anim = CameraAnimation::spin(80.0);
        let c = anim.advance(&start, 3600.0).0;
        let turned = angle_diff(azimuth_deg(&c), azimuth_deg(&start)).abs();
        assert!((turned - 80.0 * MAX_STEP_S).abs() < 0.05, "turned {turned}");
    }

    #[test]
    fn test_rock_stays_in_range_and_turns_around_at_the_ends() {
        let base = cam([30.0, 0.0, 40.0], [0.0, 0.0, 0.0]);
        let mut anim = CameraAnimation::rock(base.clone());
        let mut c = base.clone();
        let (mut max, mut min) = (f32::MIN, f32::MAX);
        let mut signs_changed = 0;
        let mut last_delta = 0.0f32;
        let mut last_sign = 0.0f32;
        // 2 周期ぶん、0.02 秒ずつ
        for _ in 0..((2.0 * ROCK_PERIOD_S / 0.02) as usize) {
            c = anim.advance(&c, 0.02).0;
            let off = angle_diff(azimuth_deg(&c), azimuth_deg(&base));
            max = max.max(off);
            min = min.min(off);
            let delta = off - last_delta;
            if delta.abs() > 1e-6 {
                let s = delta.signum();
                if last_sign != 0.0 && s != last_sign {
                    signs_changed += 1;
                }
                last_sign = s;
            }
            last_delta = off;
            assert!(off.abs() <= ROCK_AMPLITUDE_DEG + 0.05, "{off}");
        }
        assert!((max - ROCK_AMPLITUDE_DEG).abs() < 0.5, "max {max}");
        assert!((min + ROCK_AMPLITUDE_DEG).abs() < 0.5, "min {min}");
        // 2 周期で、向きは 4 回変わる（両端で 2 回ずつ）。数え漏れを許して 3 回以上。
        assert!(signs_changed >= 3, "turned around {signs_changed} times");
        // 距離は変わらない
        assert!((dist(&c) - dist(&base)).abs() < 1e-3);
    }

    #[test]
    fn test_rock_offset_is_zero_at_start_and_follows_the_period() {
        assert_eq!(rock_offset_deg(0.0, 30.0, 4.0), 0.0);
        assert!((rock_offset_deg(1.0, 30.0, 4.0) - 30.0).abs() < 1e-3);
        assert!((rock_offset_deg(3.0, 30.0, 4.0) + 30.0).abs() < 1e-3);
        assert!(rock_offset_deg(4.0, 30.0, 4.0).abs() < 1e-3);
        assert_eq!(rock_offset_deg(1.0, 30.0, 0.0), 0.0);
    }

    #[test]
    fn test_interpolation_hits_the_ends() {
        let a = cam([0.0, 0.0, 50.0], [0.0, 0.0, 0.0]);
        let mut b = cam([20.0, 10.0, -30.0], [5.0, 5.0, 5.0]);
        b.fov = 30.0;
        assert_cam_eq(&interpolate_camera(&a, &b, 0.0), &a);
        assert_cam_eq(&interpolate_camera(&a, &b, 1.0), &b);
        assert_cam_eq(&interpolate_camera(&a, &b, -3.0), &a);
        assert_cam_eq(&interpolate_camera(&a, &b, 7.0), &b);
    }

    #[test]
    fn test_interpolation_keeps_the_distance_between_the_ends() {
        // 注視点が同じで、向きが違う（90 度、135 度、ほぼ反対）。途中で距離が極端に変わらない。
        let a = cam([0.0, 0.0, 40.0], [0.0, 0.0, 0.0]);
        for eye in [
            [40.0, 0.0, 0.0],
            [-28.284_271, 0.0, -28.284_271],
            [0.0, 0.0, -40.0],
            [0.0, 1.0e-3, -40.0],
        ] {
            let b = cam(eye, [0.0, 0.0, 0.0]);
            for i in 0..=20 {
                let c = interpolate_camera(&a, &b, i as f32 / 20.0);
                let d = dist(&c);
                assert!(
                    (39.9..=40.1).contains(&d),
                    "eye {eye:?} step {i}: distance {d}"
                );
            }
        }
    }

    #[test]
    fn test_interpolation_zooms_smoothly_in_log_space() {
        let a = cam([0.0, 0.0, 100.0], [0.0, 0.0, 0.0]);
        let b = cam([0.0, 0.0, 10.0], [0.0, 0.0, 0.0]);
        let mid = interpolate_camera(&a, &b, 0.5);
        // 対数の中点は幾何平均 sqrt(100 × 10)
        assert!((dist(&mid) - (100.0f32 * 10.0).sqrt()).abs() < 1e-2);
        // 距離は単調に縮む
        let mut last = f32::MAX;
        for i in 0..=10 {
            let d = dist(&interpolate_camera(&a, &b, i as f32 / 10.0));
            assert!(d <= last + 1e-4);
            last = d;
        }
    }

    #[test]
    fn test_interpolation_moves_the_target_linearly() {
        let a = cam([0.0, 0.0, 50.0], [0.0, 0.0, 0.0]);
        let b = cam([10.0, 0.0, 50.0], [10.0, 0.0, 0.0]);
        let mid = interpolate_camera(&a, &b, 0.5);
        assert!((mid.target[0] - 5.0).abs() < 1e-4);
        assert!((mid.eye[0] - 5.0).abs() < 1e-3);
    }

    #[test]
    fn test_interpolation_with_an_overlapping_eye_goes_to_the_end() {
        let a = cam([1.0, 1.0, 1.0], [1.0, 1.0, 1.0]);
        let b = cam([0.0, 0.0, 50.0], [0.0, 0.0, 0.0]);
        assert_cam_eq(&interpolate_camera(&a, &b, 0.5), &b);
    }

    #[test]
    fn test_smoothstep_is_slow_at_the_ends() {
        assert_eq!(smoothstep(0.0), 0.0);
        assert_eq!(smoothstep(1.0), 1.0);
        assert!((smoothstep(0.5) - 0.5).abs() < 1e-6);
        assert!(smoothstep(0.1) < 0.1);
        assert!(smoothstep(0.9) > 0.9);
        assert_eq!(smoothstep(-1.0), 0.0);
        assert_eq!(smoothstep(2.0), 1.0);
    }

    #[test]
    fn test_move_finishes_exactly_at_the_target_after_the_duration() {
        let a = cam([0.0, 0.0, 50.0], [0.0, 0.0, 0.0]);
        let b = cam([20.0, 10.0, -30.0], [5.0, 5.0, 5.0]);
        let mut anim = CameraAnimation::move_between(a.clone(), b.clone());
        let mut c = a.clone();
        let mut finished = false;
        let mut steps = 0;
        while !finished {
            let (next, done) = anim.advance(&c, 0.05);
            c = next;
            finished = done;
            steps += 1;
            assert!(steps < 100, "never finishes");
        }
        // 0.4 秒を 0.05 秒ずつ: 8 回（浮動小数点の誤差で 9 回目になることがある）
        assert!((8..=9).contains(&steps), "{steps}");
        assert_cam_eq(&c, &b);
    }

    #[test]
    fn test_moving_to_the_same_camera_finishes_at_once() {
        let a = cam([0.0, 0.0, 50.0], [0.0, 0.0, 0.0]);
        let mut anim = CameraAnimation::move_between(a.clone(), a.clone());
        assert!(anim.advance(&a, 0.016).1);
    }

    #[test]
    fn test_spin_speed_cycles_through_the_choices() {
        let mut s = SPIN_SPEEDS[0];
        let mut seen = vec![s];
        for _ in 0..SPIN_SPEEDS.len() {
            s = next_spin_speed(s);
            seen.push(s);
        }
        assert_eq!(seen.first(), seen.last());
        assert_eq!(&seen[..SPIN_SPEEDS.len()], &SPIN_SPEEDS);
        assert_eq!(next_spin_speed(33.0), DEFAULT_SPIN_SPEED);
    }

    #[test]
    fn test_kinds_report_themselves() {
        let c = cam([0.0, 0.0, 50.0], [0.0, 0.0, 0.0]);
        assert!(CameraAnimation::spin(20.0).is_spin());
        assert!(CameraAnimation::spin(20.0).is_continuous());
        assert!(CameraAnimation::rock(c.clone()).is_rock());
        assert!(CameraAnimation::rock(c.clone()).is_continuous());
        assert!(!CameraAnimation::move_between(c.clone(), c).is_continuous());
    }
}
