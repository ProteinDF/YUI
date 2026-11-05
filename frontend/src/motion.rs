//! 動きのファイル（`schema: yui-motion/1`、architecture.md 8.13 節）の読み込みと、
//! 時刻ごとのカメラの計算。UI には関わらない（書き出しは、後の版で組み込む）。
//!
//! 手順は上から順に実行する。手順の中は等速で動き、手順どうしのつなぎ目で動きが途切れないよう、
//! 次の手順は、前の手順が終わったカメラから始まる。

use crate::viewport::camera::{TurnAxis, turn_camera, zoom_camera};
use serde::Deserialize;
use yui_core::representation::{Representation, RepresentationKind};
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
    /// マルチモデル構造のモデル、または XYZ の軌跡のフレームを、`from` から `to` まで（1 始まり、両端を含む。
    /// `from` が `to` より大きければ逆戻り）、`seconds` かけて進める。
    ShowFrame {
        from: usize,
        to: usize,
        seconds: f32,
    },
    /// ワークスペースの表現（`id` で指す）の `visible` と `opacity` を変える。`seconds` があれば、
    /// 不透明度をなめらかに変える（フェード）。なければ、すぐに変える（長さは 0）。
    Set {
        id: String,
        visible: Option<bool>,
        opacity: Option<f32>,
        seconds: Option<f32>,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetBody {
    id: String,
    visible: Option<bool>,
    opacity: Option<f32>,
    seconds: Option<f32>,
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShowFrameBody {
    from: usize,
    to: usize,
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
            Some("show_frame") => {
                let s: ShowFrameBody = serde_yaml::from_value(body).map_err(conv)?;
                Ok(Step::ShowFrame {
                    from: s.from,
                    to: s.to,
                    seconds: s.seconds,
                })
            }
            Some("set") => {
                let b: SetBody = serde_yaml::from_value(body).map_err(conv)?;
                Ok(Step::Set {
                    id: b.id,
                    visible: b.visible,
                    opacity: b.opacity,
                    seconds: b.seconds,
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
            Step::Turn { seconds, .. }
            | Step::Zoom { seconds, .. }
            | Step::ShowFrame { seconds, .. } => *seconds,
            Step::Wait(seconds) => *seconds,
            Step::Set { seconds, .. } => seconds.unwrap_or(0.0),
        }
    }
}

impl Motion {
    /// 動き全体の長さ（秒）
    pub fn total_seconds(&self) -> f32 {
        self.steps.iter().map(Step::seconds).sum()
    }

    /// モデルや軌跡のフレームを進める手順があるか
    pub fn uses_frames(&self) -> bool {
        self.steps
            .iter()
            .any(|s| matches!(s, Step::ShowFrame { .. }))
    }

    /// 表現を変える手順（`set`）があるか
    pub fn uses_set(&self) -> bool {
        self.steps.iter().any(|s| matches!(s, Step::Set { .. }))
    }

    /// 手順が指す、いちばん大きいフレームの番号（1 始まり）。フレームを進める手順がなければ 0。
    /// 構造のフレームの数がこれ以上あるかは、書き出しを始める前に、呼び出し側が確かめる。
    pub fn max_frame(&self) -> usize {
        self.steps
            .iter()
            .filter_map(|s| match s {
                Step::ShowFrame { from, to, .. } => Some(*from.max(to)),
                _ => None,
            })
            .max()
            .unwrap_or(0)
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
        // `set` の秒数は省略できる（省略は 0 秒）。書いたときは、ほかの手順と同じく正の数
        let seconds_ok = match step {
            Step::Set { seconds: None, .. } => true,
            _ => seconds.is_finite() && seconds > 0.0,
        };
        if !seconds_ok {
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
            Step::ShowFrame { from, to, .. } => {
                if *from == 0 || *to == 0 {
                    return Err(format!("step {n}: frame numbers start at 1"));
                }
            }
            Step::Set {
                id,
                visible,
                opacity,
                ..
            } => {
                if id.is_empty() {
                    return Err(format!("step {n}: id must not be empty"));
                }
                if visible.is_none() && opacity.is_none() {
                    return Err(format!("step {n}: set needs visible or opacity"));
                }
                if let Some(o) = opacity
                    && !(o.is_finite() && (0.0..=1.0).contains(o))
                {
                    return Err(format!("step {n}: opacity must be between 0 and 1"));
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
        // フレームを進める手順は、視点を変えない（`frame_at` が決める）
        Step::Wait(_) | Step::ShowFrame { .. } | Step::Set { .. } => {}
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

/// 時刻 `time_s` に表示するモデル（軌跡のフレーム）の番号（0 始まり）。フレームを進める手順がまだ
/// 始まっていなければ `None`（今の表示のまま）。手順が終わったあとは、その最後のフレームのまま。
pub fn frame_at(motion: &Motion, time_s: f32) -> Option<usize> {
    let t = time_s.max(0.0);
    let mut start = 0.0_f32;
    let mut shown = None;
    for step in &motion.steps {
        let len = step.seconds();
        if let Step::ShowFrame { from, to, .. } = step {
            if t >= start + len {
                shown = Some(to - 1);
            } else if t >= start {
                let p = ((t - start) / len).clamp(0.0, 1.0);
                let i = *from as f32 + (*to as f32 - *from as f32) * p;
                return Some(i.round().max(1.0) as usize - 1);
            }
        }
        if t < start + len {
            return shown;
        }
        start += len;
    }
    shown
}

/// `k` 枚目（0 始まり）のフレームで表示するモデル（軌跡のフレーム）の番号。時刻は `k / fps` 秒。
pub fn frame_for_frame(motion: &Motion, k: usize) -> Option<usize> {
    frame_at(motion, k as f32 / motion.fps as f32)
}

/// `k` 枚目（0 始まり）のフレームのカメラ。時刻は `k / fps` 秒。
pub fn camera_for_frame(base: &CameraState, motion: &Motion, k: usize) -> CameraState {
    camera_at(base, motion, k as f32 / motion.fps as f32)
}

/// 不透明度が見た目に効く表現の種類（分子表面と相互作用）。ほかの種類は、`visible` だけが効く。
fn uses_opacity(kind: RepresentationKind) -> bool {
    matches!(
        kind,
        RepresentationKind::Surface | RepresentationKind::Interaction
    )
}

/// `set` が指す表現がワークスペースにあるか、不透明度の効かない種類に `opacity` を指していないかを
/// 確かめる。動きのファイルだけでは分からないので、書き出しを始める前に、ワークスペースの表現で調べる。
pub fn check_sets(motion: &Motion, reps: &[Representation]) -> Result<(), String> {
    for (i, step) in motion.steps.iter().enumerate() {
        let Step::Set { id, opacity, .. } = step else {
            continue;
        };
        let n = i + 1;
        let matching: Vec<&Representation> = reps.iter().filter(|r| &r.id == id).collect();
        if matching.is_empty() {
            return Err(format!("step {n}: no representation with id {id:?}"));
        }
        if opacity.is_some() && matching.iter().any(|r| !uses_opacity(r.kind)) {
            return Err(format!(
                "step {n}: representation {id:?} does not use opacity (only Surface and Interaction do)"
            ));
        }
    }
    Ok(())
}

/// 1 つの表現に `set` を適用する。`p` は手順の進み具合（0 から 1。すぐに変える手順は 1）。
/// `fading` は、秒数のある手順か。フェードは不透明度の効く種類だけで、ほかの種類は手順の頭で切り替える。
fn apply_set(
    rep: &mut Representation,
    visible: Option<bool>,
    opacity: Option<f32>,
    fading: bool,
    p: f32,
) {
    let (from_visible, from_opacity) = (rep.visible, rep.opacity);
    let to_visible = visible.unwrap_or(from_visible);
    let to_opacity = opacity.unwrap_or(from_opacity);
    if !uses_opacity(rep.kind) || !fading {
        rep.visible = to_visible;
        if uses_opacity(rep.kind) {
            rep.opacity = to_opacity;
        }
        return;
    }
    let lerp = |a: f32, b: f32| a + (b - a) * p;
    match (from_visible, to_visible) {
        // 現れる: 見える状態で、0 から目標の不透明度へ
        (false, true) => {
            rep.visible = true;
            rep.opacity = lerp(0.0, to_opacity);
        }
        // 消える: 0 へ。終わったら隠し、不透明度はもとの値に戻しておく（あとで現すときのため）
        (true, false) => {
            if p >= 1.0 {
                rep.visible = false;
                rep.opacity = opacity.unwrap_or(from_opacity);
            } else {
                rep.opacity = lerp(from_opacity, 0.0);
            }
        }
        (true, true) => rep.opacity = lerp(from_opacity, to_opacity),
        // 見えないまま: 見た目は変わらないので、不透明度だけ値を移す
        (false, false) => rep.opacity = to_opacity,
    }
}

/// 時刻 `time_s` の表現のようす。`base` は動きを始める前のワークスペースの表現。`set` の手順を
/// 順に適用して返す（手順がまだ始まっていなければ、`base` のまま）。
pub fn representations_at(
    base: &[Representation],
    motion: &Motion,
    time_s: f32,
) -> Vec<Representation> {
    let mut reps = base.to_vec();
    let t = time_s.max(0.0);
    let mut start = 0.0_f32;
    for step in &motion.steps {
        let len = step.seconds();
        if t < start {
            break;
        }
        if let Step::Set {
            id,
            visible,
            opacity,
            seconds,
        } = step
        {
            let p = if t >= start + len {
                1.0
            } else {
                ((t - start) / len).clamp(0.0, 1.0)
            };
            for rep in reps.iter_mut().filter(|r| &r.id == id) {
                apply_set(rep, *visible, *opacity, seconds.is_some(), p);
            }
        }
        start += len;
    }
    reps
}

/// `k` 枚目（0 始まり）のフレームの表現。時刻は `k / fps` 秒。
pub fn representations_for_frame(
    base: &[Representation],
    motion: &Motion,
    k: usize,
) -> Vec<Representation> {
    representations_at(base, motion, k as f32 / motion.fps as f32)
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

    /// `examples/motions/` のサンプルが、いつでも読めること
    #[test]
    fn the_example_motion_files_are_valid() {
        let text = include_str!("../../examples/motions/turn-around.yaml");
        let m = parse_motion(text).expect("turn-around.yaml");
        assert_eq!(m.fps, 30);
        assert_eq!(m.frame_count(), 360, "12 s at 30 fps");
    }

    fn frames_motion() -> Motion {
        parse_motion(
            "schema: yui-motion/1\nfps: 10\nsteps:\n  - wait: 1\n  - show_frame: {from: 1, to: 5, seconds: 4}\n  - wait: 1\n  - show_frame: {from: 5, to: 1, seconds: 4}\n",
        )
        .unwrap()
    }

    #[test]
    fn show_frame_steps_pick_the_model_at_each_time() {
        let m = frames_motion();
        assert!(m.uses_frames());
        assert_eq!(m.max_frame(), 5);
        // 始まる前は、今の表示のまま
        assert_eq!(frame_at(&m, 0.0), None);
        assert_eq!(frame_at(&m, 0.99), None);
        // 1 → 5 を 4 秒で。始まりは 1 番（添字 0）、等間隔で進む
        assert_eq!(frame_at(&m, 1.0), Some(0));
        assert_eq!(frame_at(&m, 2.0), Some(1));
        assert_eq!(frame_at(&m, 3.0), Some(2));
        assert_eq!(frame_at(&m, 4.0), Some(3));
        assert_eq!(frame_at(&m, 4.99), Some(4));
        // 待っている間は、最後のフレームのまま
        assert_eq!(frame_at(&m, 5.5), Some(4));
        // 5 → 1 の逆戻り
        assert_eq!(frame_at(&m, 6.0), Some(4));
        assert_eq!(frame_at(&m, 8.0), Some(2));
        assert_eq!(frame_at(&m, 9.99), Some(0));
        // 終わったあとは、最後のフレームのまま
        assert_eq!(frame_at(&m, 99.0), Some(0));
        assert_eq!(frame_for_frame(&m, 20), Some(1));
    }

    #[test]
    fn show_frame_does_not_move_the_camera() {
        let m = frames_motion();
        assert_eq!(camera_at(&base(), &m, 3.0), base());
        assert!(!parse_motion(SAMPLE).unwrap().uses_frames());
        assert_eq!(parse_motion(SAMPLE).unwrap().max_frame(), 0);
        assert_eq!(frame_at(&parse_motion(SAMPLE).unwrap(), 3.0), None);
    }

    #[test]
    fn show_frame_rejects_bad_numbers() {
        for (body, expect) in [
            ("{from: 0, to: 3, seconds: 1}", "start at 1"),
            ("{from: 1, to: 0, seconds: 1}", "start at 1"),
            ("{from: 1, to: 3, seconds: 0}", "seconds"),
            ("{from: -1, to: 3, seconds: 1}", "not a motion file"),
            ("{from: 1, seconds: 1}", "not a motion file"),
        ] {
            let text = format!("schema: yui-motion/1\nfps: 10\nsteps: [{{show_frame: {body}}}]");
            let err = parse_motion(&text).expect_err(&text);
            assert!(err.contains(expect), "{text:?} -> {err:?}");
        }
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

    fn rep(id: &str, kind: RepresentationKind, visible: bool, opacity: f32) -> Representation {
        Representation::new(
            id,
            yui_core::selector::Selector::All,
            kind,
            yui_core::representation::ColorScheme::Chain,
        )
        .with_visible(visible)
        .with_opacity(opacity)
    }

    fn motion_of(steps: &str) -> Motion {
        parse_motion(&format!("schema: yui-motion/1\nfps: 10\nsteps:\n{steps}")).expect("valid")
    }

    #[test]
    fn set_steps_are_checked_when_the_file_is_read() {
        let bad = |steps: &str| {
            parse_motion(&format!("schema: yui-motion/1\nfps: 10\nsteps:\n{steps}"))
                .expect_err("should be refused")
        };
        assert!(bad("  - set: {id: a}").contains("visible or opacity"));
        assert!(bad("  - set: {id: a, opacity: 1.5}").contains("between 0 and 1"));
        assert!(bad("  - set: {id: a, opacity: -0.1}").contains("between 0 and 1"));
        assert!(bad("  - set: {id: '', visible: true}").contains("id"));
        assert!(bad("  - set: {id: a, visible: true, seconds: 0}").contains("seconds"));
        assert!(bad("  - set: {visible: true}").contains("id"));
        // 秒数がなくてもよい。長さは 0
        let m = motion_of("  - set: {id: a, visible: true}\n  - wait: 1");
        assert_eq!(m.total_seconds(), 1.0);
        assert!(m.uses_set());
    }

    #[test]
    fn set_must_name_an_existing_representation_that_can_take_the_change() {
        let reps = vec![
            rep("cart", RepresentationKind::Cartoon, true, 1.0),
            rep("surf", RepresentationKind::Surface, true, 0.8),
        ];
        let ok = motion_of(
            "  - set: {id: surf, opacity: 0.2, seconds: 1}\n  - set: {id: cart, visible: false}",
        );
        assert!(check_sets(&ok, &reps).is_ok());
        let missing = motion_of("  - set: {id: nope, visible: false}");
        let e = check_sets(&missing, &reps).unwrap_err();
        assert!(e.contains("step 1") && e.contains("nope"), "{e}");
        let unsuitable = motion_of("  - wait: 1\n  - set: {id: cart, opacity: 0.5}");
        let e = check_sets(&unsuitable, &reps).unwrap_err();
        assert!(
            e.contains("step 2") && e.contains("does not use opacity"),
            "{e}"
        );
    }

    #[test]
    fn an_immediate_set_applies_at_its_time_and_the_base_is_untouched() {
        let base = vec![
            rep("cart", RepresentationKind::Cartoon, true, 1.0),
            rep("surf", RepresentationKind::Surface, true, 0.8),
        ];
        let m = motion_of(
            "  - wait: 1\n  - set: {id: cart, visible: false}\n  - set: {id: surf, opacity: 0.3}\n  - wait: 1",
        );
        let before = representations_at(&base, &m, 0.9);
        assert_eq!(before, base);
        let after = representations_at(&base, &m, 1.0);
        assert!(!after[0].visible);
        assert_eq!(after[1].opacity, 0.3);
        assert!(base[0].visible && base[1].opacity == 0.8);
        // 動きが終わったあとも、そのまま
        assert_eq!(representations_at(&base, &m, 99.0), after);
    }

    #[test]
    fn a_fade_changes_the_opacity_evenly() {
        let base = vec![rep("surf", RepresentationKind::Surface, true, 0.8)];
        let m = motion_of("  - set: {id: surf, opacity: 0.2, seconds: 2}");
        let at = |t: f32| representations_at(&base, &m, t)[0].opacity;
        assert!((at(0.0) - 0.8).abs() < 1e-6);
        assert!((at(1.0) - 0.5).abs() < 1e-6);
        assert!((at(2.0) - 0.2).abs() < 1e-6);
        assert!((at(5.0) - 0.2).abs() < 1e-6);
    }

    #[test]
    fn fading_in_and_out_changes_visibility_at_the_right_moments() {
        // 隠れていた表現が現れる: 見える状態で 0 から目標へ
        let hidden = vec![rep("surf", RepresentationKind::Surface, false, 0.6)];
        let m = motion_of("  - set: {id: surf, visible: true, seconds: 2}");
        let r = &representations_at(&hidden, &m, 1.0)[0];
        assert!(r.visible && (r.opacity - 0.3).abs() < 1e-6);
        let r = &representations_at(&hidden, &m, 2.0)[0];
        assert!(r.visible && (r.opacity - 0.6).abs() < 1e-6);

        // 見えていた表現が消える: 終わるまで見え、終わったら隠れて、不透明度はもとの値
        let shown = vec![rep("surf", RepresentationKind::Surface, true, 0.6)];
        let m = motion_of("  - set: {id: surf, visible: false, seconds: 2}");
        let r = &representations_at(&shown, &m, 1.0)[0];
        assert!(r.visible && (r.opacity - 0.3).abs() < 1e-6);
        let r = &representations_at(&shown, &m, 2.0)[0];
        assert!(!r.visible && (r.opacity - 0.6).abs() < 1e-6);
    }

    #[test]
    fn a_kind_without_opacity_switches_at_the_start_of_the_step() {
        let base = vec![rep("cart", RepresentationKind::Cartoon, false, 1.0)];
        let m = motion_of("  - set: {id: cart, visible: true, seconds: 2}");
        let r = &representations_at(&base, &m, 0.0)[0];
        assert!(r.visible, "no fade: it appears at once");
        assert_eq!(r.opacity, 1.0);
    }

    #[test]
    fn set_does_not_move_the_camera_or_the_frames() {
        let m = motion_of("  - set: {id: a, visible: true, seconds: 1}");
        assert_eq!(frame_at(&m, 0.5), None);
        let base = CameraState::default();
        assert_eq!(camera_at(&base, &m, 0.5), base);
    }
}
