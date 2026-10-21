//! 再生の基盤（architecture.md 8.10節）。
//!
//! 「フレーム」の列を、時間に合わせて進める仕組み。フレームが何であるか（今は構造のモデル、後で軌跡のコマ）は知らず、
//! 「フレームの総数」と「今のフレームの番号」だけを扱う。フレームを画面に出す側（`app/playback_ops.rs`）が、番号を
//! 表示するモデルに結び付ける。再生は利用者ごとのもので、`AppState` にも協調編集にも入れない。
//!
//! ここには、画面や時計を使わずに試せる処理（次のフレームの計算、経過時間から進める量の計算、再生を始めるフレーム）を置く。

use std::time::Duration;

use iced::time::Instant;

/// 速さ（フレーム / 秒）の範囲
pub const MIN_FPS: u32 = 1;
pub const MAX_FPS: u32 = 30;
/// 速さの既定値。NMR のアンサンブルのモデルを見るのに、ちょうどよい遅さ。
pub const DEFAULT_FPS: u32 = 5;
/// 1 回のタイマーで進める量の上限。長く止まったあとに、延々と進めないための安全弁。
const MAX_STEPS_PER_TICK: usize = 1000;
/// タイマーの間隔の下限
const MIN_TICK: Duration = Duration::from_millis(16);

/// 最後のフレームに着いたあとの動き
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LoopMode {
    /// 最後で止まる
    Once,
    /// 先頭に戻って続ける
    #[default]
    Repeat,
    /// 向きを反転して、行ったり来たりする
    PingPong,
}

impl LoopMode {
    pub fn label(self) -> &'static str {
        match self {
            LoopMode::Once => "Once",
            LoopMode::Repeat => "Repeat",
            LoopMode::PingPong => "Ping-pong",
        }
    }

    /// コマンドの引数に使う名前
    pub fn as_str(self) -> &'static str {
        match self {
            LoopMode::Once => "once",
            LoopMode::Repeat => "repeat",
            LoopMode::PingPong => "ping_pong",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "once" => Some(LoopMode::Once),
            "repeat" => Some(LoopMode::Repeat),
            "ping_pong" | "ping-pong" | "pingpong" => Some(LoopMode::PingPong),
            _ => None,
        }
    }

    /// ボタンで順に切り替えるときの、次の方法
    pub fn next(self) -> Self {
        match self {
            LoopMode::Once => LoopMode::Repeat,
            LoopMode::Repeat => LoopMode::PingPong,
            LoopMode::PingPong => LoopMode::Once,
        }
    }
}

/// 再生の向き（往復のときだけ後ろ向きになる）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Direction {
    #[default]
    Forward,
    Backward,
}

/// 1 回（または数回）進めた結果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub frame: usize,
    pub direction: Direction,
    /// 「1回」で最後に着いて、止まるべきか
    pub finished: bool,
}

/// 1 フレーム進める。`total` はフレームの総数、`frame` は今のフレーム（0 始まり）。
///
/// - 前向きで最後に着いたあと: 1回 = 止まる、繰り返し = 先頭へ、往復 = 向きを反転して 1 つ手前へ。
/// - 後ろ向き（往復だけ）で先頭に着いたあと: 往復 = 向きを反転して 1 つ先へ。
///   往復では、端のフレームを 2 回続けて表示しない。
pub fn step(frame: usize, total: usize, direction: Direction, mode: LoopMode) -> Step {
    if total < 2 {
        return Step {
            frame: 0,
            direction: Direction::Forward,
            finished: true,
        };
    }
    let frame = frame.min(total - 1);
    match direction {
        Direction::Forward => {
            if frame + 1 < total {
                Step {
                    frame: frame + 1,
                    direction,
                    finished: false,
                }
            } else {
                match mode {
                    LoopMode::Once => Step {
                        frame,
                        direction,
                        finished: true,
                    },
                    LoopMode::Repeat => Step {
                        frame: 0,
                        direction,
                        finished: false,
                    },
                    LoopMode::PingPong => Step {
                        frame: total - 2,
                        direction: Direction::Backward,
                        finished: false,
                    },
                }
            }
        }
        Direction::Backward => {
            if frame > 0 {
                Step {
                    frame: frame - 1,
                    direction,
                    finished: false,
                }
            } else {
                match mode {
                    LoopMode::Once => Step {
                        frame,
                        direction,
                        finished: true,
                    },
                    LoopMode::Repeat => Step {
                        frame: total - 1,
                        direction,
                        finished: false,
                    },
                    LoopMode::PingPong => Step {
                        frame: 1,
                        direction: Direction::Forward,
                        finished: false,
                    },
                }
            }
        }
    }
}

/// `steps` 回、続けて進める。「1回」で最後に着いたら、そこで止める（`finished`）。
pub fn advance(
    mut frame: usize,
    total: usize,
    mut direction: Direction,
    mode: LoopMode,
    steps: usize,
) -> Step {
    let mut finished = false;
    for _ in 0..steps {
        let s = step(frame, total, direction, mode);
        frame = s.frame;
        direction = s.direction;
        if s.finished {
            finished = true;
            break;
        }
    }
    Step {
        frame,
        direction,
        finished,
    }
}

/// 再生を始めるフレーム。
///
/// 全モデル表示（`current` が `None`）なら、先頭から。「1回」で最後に着いていたら、先頭からやり直す。
/// それ以外は、今のフレームから続ける。
pub fn start_frame(current: Option<usize>, total: usize, mode: LoopMode) -> usize {
    match current {
        None => 0,
        Some(f) if mode == LoopMode::Once && f + 1 >= total => 0,
        Some(f) => f.min(total.saturating_sub(1)),
    }
}

/// 再生の状態（利用者ごと。保存しない）
#[derive(Debug, Clone)]
pub struct Playback {
    pub playing: bool,
    /// つまみを動かしている間（分子表面を描かず、離したときに作り直す）
    pub scrubbing: bool,
    pub fps: u32,
    pub loop_mode: LoopMode,
    pub direction: Direction,
    /// 前回のタイマーの時刻
    last_tick: Option<Instant>,
    /// まだ進めていない、端数のフレーム
    carry: f64,
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            playing: false,
            scrubbing: false,
            fps: DEFAULT_FPS,
            loop_mode: LoopMode::default(),
            direction: Direction::Forward,
            last_tick: None,
            carry: 0.0,
        }
    }
}

impl Playback {
    /// 再生を始める（前向きで、時計を今に合わせる）
    pub fn start(&mut self, now: Instant) {
        self.playing = true;
        self.scrubbing = false;
        self.direction = Direction::Forward;
        self.last_tick = Some(now);
        self.carry = 0.0;
    }

    /// 止める
    pub fn pause(&mut self) {
        self.playing = false;
        self.last_tick = None;
        self.carry = 0.0;
    }

    pub fn set_fps(&mut self, fps: u32) {
        self.fps = fps.clamp(MIN_FPS, MAX_FPS);
    }

    /// 分子表面を描かないか。ブラウザ版は、再生中と、つまみを動かしている間は、モデルとずれるので描かない
    /// （生成が同期的で追従できない）。ネイティブ版は、バックグラウンドで作って追従させる（遅れて動く）。
    pub fn hides_surface(&self) -> bool {
        cfg!(target_arch = "wasm32") && (self.playing || self.scrubbing)
    }

    /// 前回から今までの経過時間と速さから、進めるフレーム数を求める（端数は次に持ち越す）。
    ///
    /// 数えたタイマーの回数ではなく時刻で決めるので、描画が遅れても再生は遅くならず、間のフレームを飛ばして追いつく。
    pub fn steps_due(&mut self, now: Instant) -> usize {
        let Some(last) = self.last_tick else {
            self.last_tick = Some(now);
            return 0;
        };
        let elapsed = now.saturating_duration_since(last);
        self.last_tick = Some(now);
        self.carry += elapsed.as_secs_f64() * f64::from(self.fps);
        let whole = self.carry.floor();
        self.carry -= whole;
        (whole as usize).min(MAX_STEPS_PER_TICK)
    }

    /// タイマーの購読の間隔。止まっているときは `None`（購読しない）。
    ///
    /// 1 フレームの長さの半分とする（時刻から進める量を決めるので、細かいほど進みが均等になる）。
    pub fn tick_interval(&self) -> Option<Duration> {
        if !self.playing {
            return None;
        }
        let half_frame = Duration::from_secs_f64(0.5 / f64::from(self.fps.max(1)));
        Some(half_frame.max(MIN_TICK))
    }
}

/// 再生の操作（操作のバー、メニューのコマンド、キーボードが、同じものを使う）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackAction {
    First,
    Prev,
    PlayPause,
    Next,
    Last,
    /// スライダーで、フレームを直接選ぶ
    SetFrame(usize),
    /// スライダーを離した（分子表面を作り直す）
    ScrubEnd,
    SetFps(u32),
    SetLoopMode(LoopMode),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walk(start: usize, total: usize, mode: LoopMode, n: usize) -> (Vec<usize>, Direction, bool) {
        let mut frame = start;
        let mut dir = Direction::Forward;
        let mut seq = Vec::new();
        let mut finished = false;
        for _ in 0..n {
            let s = step(frame, total, dir, mode);
            frame = s.frame;
            dir = s.direction;
            seq.push(frame);
            if s.finished {
                finished = true;
                break;
            }
        }
        (seq, dir, finished)
    }

    #[test]
    fn test_once_stops_at_the_last_frame() {
        let (seq, _, finished) = walk(0, 3, LoopMode::Once, 10);
        assert_eq!(seq, vec![1, 2, 2]);
        assert!(finished);
    }

    #[test]
    fn test_repeat_wraps_to_the_first_frame() {
        let (seq, _, finished) = walk(0, 3, LoopMode::Repeat, 7);
        assert_eq!(seq, vec![1, 2, 0, 1, 2, 0, 1]);
        assert!(!finished);
    }

    #[test]
    fn test_ping_pong_reverses_without_repeating_the_end_frames() {
        let (seq, dir, finished) = walk(0, 4, LoopMode::PingPong, 9);
        assert_eq!(seq, vec![1, 2, 3, 2, 1, 0, 1, 2, 3]);
        assert_eq!(dir, Direction::Forward); // 先頭で反転して、また前向きになった
        assert!(!finished);
    }

    #[test]
    fn test_ping_pong_with_two_frames_alternates() {
        let (seq, _, _) = walk(0, 2, LoopMode::PingPong, 5);
        assert_eq!(seq, vec![1, 0, 1, 0, 1]);
    }

    #[test]
    fn test_fewer_than_two_frames_finish_immediately() {
        for total in [0, 1] {
            let s = step(0, total, Direction::Forward, LoopMode::Repeat);
            assert_eq!((s.frame, s.finished), (0, true));
        }
    }

    #[test]
    fn test_out_of_range_frame_is_clamped() {
        let s = step(99, 3, Direction::Forward, LoopMode::Once);
        assert_eq!((s.frame, s.finished), (2, true));
    }

    #[test]
    fn test_advance_many_steps_equals_stepping_one_by_one() {
        let total = 5;
        for mode in [LoopMode::Repeat, LoopMode::PingPong] {
            for n in 0..30 {
                let (seq, dir, _) = walk(1, total, mode, n);
                let a = advance(1, total, Direction::Forward, mode, n);
                assert_eq!(a.frame, seq.last().copied().unwrap_or(1), "{mode:?} {n}");
                if n > 0 {
                    assert_eq!(a.direction, dir, "{mode:?} {n}");
                }
            }
        }
    }

    #[test]
    fn test_advance_once_stops_and_reports_finished() {
        let a = advance(0, 4, Direction::Forward, LoopMode::Once, 100);
        assert_eq!((a.frame, a.finished), (3, true));
        let b = advance(0, 4, Direction::Forward, LoopMode::Once, 2);
        assert_eq!((b.frame, b.finished), (2, false));
    }

    #[test]
    fn test_start_frame_from_all_models_is_the_first() {
        assert_eq!(start_frame(None, 20, LoopMode::Repeat), 0);
        assert_eq!(start_frame(None, 20, LoopMode::Once), 0);
    }

    #[test]
    fn test_start_frame_continues_or_restarts_after_the_end() {
        assert_eq!(start_frame(Some(7), 20, LoopMode::Repeat), 7);
        assert_eq!(start_frame(Some(19), 20, LoopMode::Repeat), 19);
        assert_eq!(start_frame(Some(19), 20, LoopMode::Once), 0);
        assert_eq!(start_frame(Some(3), 20, LoopMode::Once), 3);
    }

    #[test]
    fn test_steps_due_follows_time_and_catches_up_when_late() {
        let t0 = Instant::now();
        let mut p = Playback::default();
        p.set_fps(10);
        p.start(t0);
        // 100 ms で 1 フレーム
        assert_eq!(p.steps_due(t0 + Duration::from_millis(100)), 1);
        // 遅れて 350 ms 後に届いても、間のフレーム（3 つ）を飛ばして追いつく
        assert_eq!(p.steps_due(t0 + Duration::from_millis(450)), 3);
        // 端数は持ち越す（0.5 フレームが残っている。20 ms で 0.7、さらに 50 ms で 1.2 になって 1 フレーム）
        assert_eq!(p.steps_due(t0 + Duration::from_millis(470)), 0);
        assert_eq!(p.steps_due(t0 + Duration::from_millis(520)), 1);
    }

    #[test]
    fn test_steps_due_is_capped_after_a_long_stall() {
        let t0 = Instant::now();
        let mut p = Playback::default();
        p.set_fps(MAX_FPS);
        p.start(t0);
        assert_eq!(
            p.steps_due(t0 + Duration::from_secs(3600)),
            MAX_STEPS_PER_TICK
        );
    }

    #[test]
    fn test_no_timer_while_paused() {
        let mut p = Playback::default();
        assert_eq!(p.tick_interval(), None);
        p.start(Instant::now());
        assert!(p.tick_interval().is_some());
        p.pause();
        assert_eq!(p.tick_interval(), None);
    }

    #[test]
    fn test_tick_interval_is_half_a_frame_with_a_floor() {
        let mut p = Playback::default();
        p.start(Instant::now());
        p.set_fps(1);
        assert_eq!(p.tick_interval(), Some(Duration::from_millis(500)));
        p.set_fps(MAX_FPS);
        let fastest = p.tick_interval().unwrap();
        assert!(
            fastest >= MIN_TICK && fastest < Duration::from_millis(17),
            "{fastest:?}"
        );
    }

    #[test]
    fn test_fps_is_clamped_to_the_range() {
        let mut p = Playback::default();
        p.set_fps(0);
        assert_eq!(p.fps, MIN_FPS);
        p.set_fps(500);
        assert_eq!(p.fps, MAX_FPS);
    }

    #[test]
    fn test_surface_is_hidden_while_playing_or_scrubbing_only_in_the_browser() {
        let browser = cfg!(target_arch = "wasm32");
        let mut p = Playback::default();
        assert!(!p.hides_surface());
        p.start(Instant::now());
        assert_eq!(p.hides_surface(), browser);
        p.pause();
        assert!(!p.hides_surface());
        p.scrubbing = true;
        assert_eq!(p.hides_surface(), browser);
    }

    #[test]
    fn test_loop_mode_names_round_trip() {
        for m in [LoopMode::Once, LoopMode::Repeat, LoopMode::PingPong] {
            assert_eq!(LoopMode::parse(m.as_str()), Some(m));
            assert_eq!(m.next().next().next(), m);
        }
        assert_eq!(LoopMode::parse("bounce"), None);
    }
}
