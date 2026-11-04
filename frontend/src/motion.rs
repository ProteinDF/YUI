//! 動きのファイル（`schema: yui-motion/1`、architecture.md 8.13 節）の読み込みと、
//! 時刻ごとのカメラの計算。UI には関わらない（書き出しは、後の版で組み込む）。
//!
//! 手順は上から順に実行する。手順の中は等速で動き、手順どうしのつなぎ目で動きが途切れないよう、
//! 次の手順は、前の手順が終わったカメラから始まる。

use crate::viewport::camera::{TurnAxis, turn_camera, zoom_camera};
use serde::Deserialize;
use yui_core::state::CameraState;

/// 対応する `schema` の値
pub const SCHEMA: &str = "yui-motion/1";
/// 1 秒あたりのフレーム数の上限・下限
pub const FPS_RANGE: std::ops::RangeInclusive<u32> = 1..=120;
/// 動き全体の長さの上限（秒）。書き出しの枚数を抑えるため。
pub const MAX_TOTAL_SECONDS: f32 = 600.0;

/// 読み込んだ動き
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Motion {
    pub schema: String,
    pub fps: u32,
    pub steps: Vec<Step>,
}

/// 手順の 1 つ。YAML では `turn: {...}`、`zoom: {...}`、`wait: 秒` と書く（1 キーの対応）。
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// 注視点を通る軸のまわりに、`angle` 度回す（正は ChimeraX の `turn` と同じ向き）
    Turn {
        axis: AxisSpec,
        angle: f32,
        seconds: f32,
    },
    /// 注視点からの距離を、`factor` 倍にする（途中は対数で変わる）
    Zoom { factor: f32, seconds: f32 },
    /// 何もしないで、待つ（秒）
    Wait(f32),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TurnBody {
    axis: AxisSpec,
    angle: f32,
    seconds: f32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ZoomBody {
    factor: f32,
    seconds: f32,
}

impl<'de> Deserialize<'de> for Step {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let value = serde_yaml::Value::deserialize(d)?;
        let map = value
            .as_mapping()
            .filter(|m| m.len() == 1)
            .ok_or_else(|| D::Error::custom("a step must be one key, such as `turn: {...}`"))?;
        let (key, body) = map.iter().next().expect("one entry");
        let body = body.clone();
        let conv = |e: serde_yaml::Error| D::Error::custom(e.to_string());
        match key.as_str() {
            Some("turn") => {
                let t: TurnBody = serde_yaml::from_value(body).map_err(conv)?;
                Ok(Step::Turn {
                    axis: t.axis,
                    angle: t.angle,
                    seconds: t.seconds,
                })
            }
            Some("zoom") => {
                let z: ZoomBody = serde_yaml::from_value(body).map_err(conv)?;
                Ok(Step::Zoom {
                    factor: z.factor,
                    seconds: z.seconds,
                })
            }
            Some("wait") => Ok(Step::Wait(serde_yaml::from_value(body).map_err(conv)?)),
            other => Err(D::Error::custom(format!("unknown step {other:?}"))),
        }
    }
}

/// 回転の軸。`"x"` / `"y"` / `"z"` は画面の軸、`[x, y, z]` は任意のベクトル。
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum AxisSpec {
    Name(String),
    Vector([f32; 3]),
}

impl Step {
    /// この手順の長さ（秒）
    pub fn seconds(&self) -> f32 {
        match self {
            Step::Turn { seconds, .. } | Step::Zoom { seconds, .. } => *seconds,
            Step::Wait(seconds) => *seconds,
        }
    }
}

impl Motion {
    /// 動き全体の長さ（秒）
    pub fn total_seconds(&self) -> f32 {
        self.steps.iter().map(Step::seconds).sum()
    }

    /// 書き出すフレームの数（`fps` × 全体の長さ、四捨五入。1 以上）
    pub fn frame_count(&self) -> usize {
        ((self.fps as f32) * self.total_seconds()).round().max(1.0) as usize
    }
}

/// YAML の文字列から動きを読み、全体を検査する。
/// 1 か所でも誤りがあれば `Err` を返す（途中で止まらないよう、動きを作る前に検査する）。
pub fn parse_motion(text: &str) -> Result<Motion, String> {
    let motion: Motion =
        serde_yaml::from_str(text).map_err(|e| format!("not a motion file: {e}"))?;
    validate(&motion)?;
    Ok(motion)
}

fn validate(motion: &Motion) -> Result<(), String> {
    if motion.schema != SCHEMA {
        return Err(format!(
            "unknown schema {:?}, expected {SCHEMA:?}",
            motion.schema
        ));
    }
    if !FPS_RANGE.contains(&motion.fps) {
        return Err(format!(
            "fps must be {}..={}, got {}",
            FPS_RANGE.start(),
            FPS_RANGE.end(),
            motion.fps
        ));
    }
    if motion.steps.is_empty() {
        return Err("the motion has no steps".to_string());
    }
    for (i, step) in motion.steps.iter().enumerate() {
        let n = i + 1;
        let seconds = step.seconds();
        if !(seconds.is_finite() && seconds > 0.0) {
            return Err(format!("step {n}: seconds must be a positive number"));
        }
        match step {
            Step::Turn { axis, angle, .. } => {
                if !angle.is_finite() {
                    return Err(format!("step {n}: angle must be a number"));
                }
                axis_to_turn(axis).map_err(|e| format!("step {n}: {e}"))?;
            }
            Step::Zoom { factor, .. } => {
                if !(factor.is_finite() && *factor > 0.0) {
                    return Err(format!("step {n}: factor must be a positive number"));
                }
            }
            Step::Wait(_) => {}
        }
    }
    let total = motion.total_seconds();
    if total > MAX_TOTAL_SECONDS {
        return Err(format!(
            "the motion is {total} s long; the limit is {MAX_TOTAL_SECONDS} s"
        ));
    }
    Ok(())
}

/// 軸の指定を `TurnAxis` にする。名前が `x` / `y` / `z` でないとき、ベクトルが 0 や無限大のときはエラー。
fn axis_to_turn(axis: &AxisSpec) -> Result<TurnAxis, String> {
    match axis {
        AxisSpec::Name(name) => match name.as_str() {
            "x" => Ok(TurnAxis::ScreenX),
            "y" => Ok(TurnAxis::ScreenY),
            "z" => Ok(TurnAxis::ScreenZ),
            other => Err(format!("axis must be x, y, z or [x, y, z], got {other:?}")),
        },
        AxisSpec::Vector(v) => {
            let len2 = v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
            if v.iter().all(|x| x.is_finite()) && len2 > 0.0 && len2.is_finite() {
                Ok(TurnAxis::World(*v))
            } else {
                Err("axis vector must be finite and not zero".to_string())
            }
        }
    }
}

/// 手順 `step` の、進み具合 `p`（0 から 1）までを、カメラに適用する
fn apply_step(cam: &mut CameraState, step: &Step, p: f32) {
    match step {
        Step::Turn { axis, angle, .. } => {
            // 検査済みなので、軸は必ず変換できる
            if let Ok(axis) = axis_to_turn(axis) {
                turn_camera(cam, axis, (angle * p).to_radians());
            }
        }
        Step::Zoom { factor, .. } => zoom_camera(cam, factor.powf(p)),
        Step::Wait(_) => {}
    }
}

/// 時刻 `time_s`（秒。動きの最初を 0 とする）のカメラ。`base` は動きを始める前のカメラ。
/// 時刻が動きの長さを過ぎたら、最後の状態のまま。
pub fn camera_at(base: &CameraState, motion: &Motion, time_s: f32) -> CameraState {
    let mut cam = base.clone();
    let t = time_s.max(0.0);
    let mut start = 0.0_f32;
    for step in &motion.steps {
        let len = step.seconds();
        if t >= start + len {
            apply_step(&mut cam, step, 1.0);
            start += len;
            continue;
        }
        let p = ((t - start) / len).clamp(0.0, 1.0);
        apply_step(&mut cam, step, p);
        return cam;
    }
    cam
}

/// `k` 枚目（0 始まり）のフレームのカメラ。時刻は `k / fps` 秒。
pub fn camera_for_frame(base: &CameraState, motion: &Motion, k: usize) -> CameraState {
    camera_at(base, motion, k as f32 / motion.fps as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
schema: yui-motion/1
fps: 10
steps:
  - turn: {axis: y, angle: 360, seconds: 2}
  - wait: 1
  - turn: {axis: [1, 0, 0], angle: 90, seconds: 1}
  - zoom: {factor: 2, seconds: 1}
";

    fn base() -> CameraState {
        CameraState {
            eye: [0.0, 0.0, 30.0],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            fov: 45.0,
        }
    }

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        (0..3).all(|k| (a[k] - b[k]).abs() < 1e-3)
    }

    #[test]
    fn parses_the_sample_and_counts_frames() {
        let m = parse_motion(SAMPLE).unwrap();
        assert_eq!(m.fps, 10);
        assert_eq!(m.steps.len(), 4);
        assert!((m.total_seconds() - 5.0).abs() < 1e-6);
        assert_eq!(m.frame_count(), 50);
    }

    #[test]
    fn rejects_bad_files_with_a_message() {
        let cases = [
            (
                "schema: yui-motion/2\nfps: 10\nsteps: [{wait: 1}]",
                "schema",
            ),
            ("schema: yui-motion/1\nfps: 0\nsteps: [{wait: 1}]", "fps"),
            ("schema: yui-motion/1\nfps: 500\nsteps: [{wait: 1}]", "fps"),
            ("schema: yui-motion/1\nfps: 10\nsteps: []", "no steps"),
            (
                "schema: yui-motion/1\nfps: 10\nsteps: [{wait: 0}]",
                "step 1",
            ),
            (
                "schema: yui-motion/1\nfps: 10\nsteps: [{wait: -2}]",
                "step 1",
            ),
            (
                "schema: yui-motion/1\nfps: 10\nsteps: [{turn: {axis: w, angle: 1, seconds: 1}}]",
                "axis",
            ),
            (
                "schema: yui-motion/1\nfps: 10\nsteps: [{turn: {axis: [0, 0, 0], angle: 1, seconds: 1}}]",
                "axis vector",
            ),
            (
                "schema: yui-motion/1\nfps: 10\nsteps: [{zoom: {factor: 0, seconds: 1}}]",
                "factor",
            ),
            (
                "schema: yui-motion/1\nfps: 10\nsteps: [{turn: {axis: y, angle: 1, seconds: 1, speed: 2}}]",
                "not a motion file",
            ),
            (
                "schema: yui-motion/1\nfps: 10\nsteps: [{spin: {seconds: 1}}]",
                "not a motion file",
            ),
            (
                "schema: yui-motion/1\nfps: 10\nsteps: [{wait: 601}]",
                "limit",
            ),
        ];
        for (text, expect) in cases {
            let err = parse_motion(text).expect_err(text);
            assert!(
                err.contains(expect),
                "{text:?} -> {err:?} should mention {expect:?}"
            );
        }
    }

    #[test]
    fn time_zero_is_the_base_camera() {
        let m = parse_motion(SAMPLE).unwrap();
        assert_eq!(camera_at(&base(), &m, 0.0), base());
    }

    #[test]
    fn a_full_turn_comes_back_to_the_start() {
        let m = parse_motion(
            "schema: yui-motion/1\nfps: 10\nsteps: [{turn: {axis: y, angle: 360, seconds: 2}}]",
        )
        .unwrap();
        let end = camera_at(&base(), &m, 2.0);
        assert!(close(end.eye, base().eye), "{:?}", end.eye);
        // 途中（半周）は、反対側に来ている
        let half = camera_at(&base(), &m, 1.0);
        assert!(close(half.eye, [0.0, 0.0, -30.0]), "{:?}", half.eye);
    }

    #[test]
    fn a_step_is_linear_in_its_own_time() {
        // 90° を 1 秒で回すので、0.5 秒では 45°。直接 turn_camera で 45° 回したものと同じ。
        let m = parse_motion(
            "schema: yui-motion/1\nfps: 10\nsteps: [{turn: {axis: y, angle: 90, seconds: 1}}]",
        )
        .unwrap();
        let mut expect = base();
        turn_camera(&mut expect, TurnAxis::ScreenY, std::f32::consts::FRAC_PI_4);
        let got = camera_at(&base(), &m, 0.5);
        assert!(close(got.eye, expect.eye) && close(got.up, expect.up));
    }

    #[test]
    fn wait_keeps_the_camera_and_the_next_step_starts_where_the_last_ended() {
        let m = parse_motion(SAMPLE).unwrap();
        // 1 つ目の手順の終わり（2 秒）と、待つ手順の途中（2.5 秒）は同じカメラ
        let at_end_of_turn = camera_at(&base(), &m, 2.0);
        let during_wait = camera_at(&base(), &m, 2.5);
        assert_eq!(at_end_of_turn, during_wait);
        // 次の手順の始め（3 秒）は、待っている間のカメラと同じ
        assert_eq!(camera_at(&base(), &m, 3.0), during_wait);
    }

    #[test]
    fn zoom_doubles_the_distance_at_its_end() {
        let m =
            parse_motion("schema: yui-motion/1\nfps: 10\nsteps: [{zoom: {factor: 2, seconds: 1}}]")
                .unwrap();
        let end = camera_at(&base(), &m, 1.0);
        assert!(close(end.eye, [0.0, 0.0, 60.0]), "{:?}", end.eye);
        // 途中は、対数で一様（0.5 秒で √2 倍）
        let half = camera_at(&base(), &m, 0.5);
        assert!(
            (half.eye[2] - 30.0 * 2f32.sqrt()).abs() < 1e-2,
            "{:?}",
            half.eye
        );
    }

    #[test]
    fn times_past_the_end_stay_at_the_last_state() {
        let m = parse_motion(SAMPLE).unwrap();
        assert_eq!(camera_at(&base(), &m, 99.0), camera_at(&base(), &m, 5.0));
    }

    #[test]
    fn frames_run_from_time_zero_at_the_fps_rate() {
        let m = parse_motion(SAMPLE).unwrap();
        assert_eq!(
            camera_for_frame(&base(), &m, 0),
            camera_at(&base(), &m, 0.0)
        );
        assert_eq!(
            camera_for_frame(&base(), &m, 10),
            camera_at(&base(), &m, 1.0)
        );
    }
}
