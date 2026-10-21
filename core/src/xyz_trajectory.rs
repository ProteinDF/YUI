//! 複数フレームの XYZ の軌跡（architecture.md 4.2節）。
//!
//! ファイルを最初に 1 回だけ走査して、各フレームの位置（バイトの範囲）の索引を作り、
//! 再生するときに、必要なフレームだけを読んで解析する。全フレームを一度にメモリへ展開しない。
//! 画面や GPU を使わず、WASM でも動く（ファイルから読むのは、ネイティブだけ）。
//!
//! 形式: 1 フレーム = 原子数の行、コメントの行、原子数ぶんの `元素 x y z` の行。フレームを続けて並べる。
//! 全フレームの原子数は同じでなければならない。コメントの `step=N` があれば、ステップとして使う。
//!
//! なお、`proteindf-bridge`（タグ 2026.10.1）の `Xyz` は、先頭の 1 フレームしか読めない。
//! 複数フレームと、DCD / XTC などの形式は、bridge 側への依頼事項（ROADMAP）とする。

use std::io::{BufRead, Cursor};
use std::sync::Arc;

use crate::state::FrameData;

/// 1 つのフレームの、ファイルの中での位置
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameSpan {
    /// フレームの先頭（原子数の行）のバイト位置
    pub start: u64,
    /// フレームのバイト数
    pub len: u64,
}

/// 軌跡の索引（ファイルを 1 回走査して作る）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XyzIndex {
    pub num_atoms: usize,
    pub frames: Vec<FrameSpan>,
}

/// 1 行ぶんを読む。ファイルの終わりなら `None`。`consumed` に、読んだバイト数を足す。
fn read_line(
    reader: &mut impl BufRead,
    buf: &mut String,
    consumed: &mut u64,
) -> Result<Option<()>, String> {
    buf.clear();
    let n = reader
        .read_line(buf)
        .map_err(|e| format!("Read error: {e}"))?;
    if n == 0 {
        return Ok(None);
    }
    *consumed += n as u64;
    Ok(Some(()))
}

/// 軌跡の全体を 1 回走査して、フレームの索引を作る。座標は解析しない（原子数の行と行数だけ見る）。
///
/// 次の場合は、エラーにする: フレームがない、原子数の行が数でない、フレームごとに原子数が違う、
/// 途中で終わっているフレームがある。末尾の空行は無視する。
pub fn index_xyz(mut reader: impl BufRead) -> Result<XyzIndex, String> {
    let mut frames: Vec<FrameSpan> = Vec::new();
    let mut num_atoms: Option<usize> = None;
    let mut offset: u64 = 0;
    let mut line = String::new();

    loop {
        // 原子数の行（フレームの間や、先頭・末尾の空行は読み飛ばす）
        let (count_text, frame_start) = loop {
            let mut consumed = 0;
            if read_line(&mut reader, &mut line, &mut consumed)?.is_none() {
                return match frames.is_empty() {
                    true => Err("The trajectory file has no frames".to_string()),
                    false => Ok(XyzIndex {
                        num_atoms: num_atoms.unwrap_or(0),
                        frames,
                    }),
                };
            }
            if line.trim().is_empty() {
                offset += consumed;
                continue;
            }
            let start = offset;
            offset += consumed;
            break (line.trim().to_string(), start);
        };
        let n: usize = count_text.parse().map_err(|_| {
            format!(
                "Frame {}: '{}' is not an atom count",
                frames.len() + 1,
                count_text
            )
        })?;
        match num_atoms {
            None => num_atoms = Some(n),
            Some(first) if first != n => {
                return Err(format!(
                    "Frame {} has {} atoms, but the first frame has {}",
                    frames.len() + 1,
                    n,
                    first
                ));
            }
            _ => {}
        }
        // コメントの行と、原子数ぶんの行
        for k in 0..(n + 1) {
            let mut consumed = 0;
            if read_line(&mut reader, &mut line, &mut consumed)?.is_none() {
                let what = if k == 0 { "comment line" } else { "atom line" };
                return Err(format!(
                    "Frame {} is incomplete: the file ends before the {}",
                    frames.len() + 1,
                    what
                ));
            }
            offset += consumed;
        }
        frames.push(FrameSpan {
            start: frame_start,
            len: offset - frame_start,
        });
    }
}

/// コメントの行から、ステップの番号を取り出す（`step=N`、`step = N`、`Step: N`）
pub fn parse_step(comment: &str) -> Option<u32> {
    let lower = comment.to_ascii_lowercase();
    let at = lower.find("step")?;
    let rest = lower[at + 4..].trim_start_matches([' ', '=', ':', '\t']);
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// 1 フレームぶんの文字列から、座標を解析する。`num_atoms` は、索引の原子数。
/// `frame_index` は、ステップが書かれていないときの、ステップの代わりの番号。
pub fn parse_xyz_frame(
    text: &str,
    num_atoms: usize,
    frame_index: u32,
) -> Result<FrameData, String> {
    let mut lines = text.lines();
    let count: usize = lines
        .next()
        .map(str::trim)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| "The frame does not start with an atom count".to_string())?;
    if count != num_atoms {
        return Err(format!("The frame has {count} atoms, expected {num_atoms}"));
    }
    let step = lines.next().and_then(parse_step).unwrap_or(frame_index);
    let mut positions = Vec::with_capacity(num_atoms);
    for i in 0..num_atoms {
        let line = lines
            .next()
            .ok_or_else(|| format!("The frame ends before atom {}", i + 1))?;
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            return Err(format!(
                "Atom {}: a line must contain 'element x y z', got '{}'",
                i + 1,
                line.trim()
            ));
        }
        let coord = |k: usize| {
            parts[k]
                .parse::<f32>()
                .map_err(|e| format!("Atom {}: bad coordinate '{}' ({e})", i + 1, parts[k]))
        };
        positions.push([coord(1)?, coord(2)?, coord(3)?]);
    }
    Ok(FrameData { step, positions })
}

/// 軌跡のデータの置き場所
#[derive(Clone)]
enum Source {
    /// すべてメモリにある（ブラウザ版、読み込み済みのバイト列）
    Memory(Arc<Vec<u8>>),
    /// ファイルから、必要なフレームだけ読む（ネイティブ）
    #[cfg(not(target_arch = "wasm32"))]
    File(std::path::PathBuf),
}

/// 読み込んだ軌跡。フレームは、`read_frame` で 1 つずつ読む。
#[derive(Clone)]
pub struct XyzTrajectory {
    name: String,
    source: Source,
    index: XyzIndex,
}

impl std::fmt::Debug for XyzTrajectory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("XyzTrajectory")
            .field("name", &self.name)
            .field("num_atoms", &self.index.num_atoms)
            .field("frames", &self.index.frames.len())
            .finish()
    }
}

impl XyzTrajectory {
    /// メモリ上のバイト列から（ブラウザ版、または、読み込み済みのファイル）
    pub fn from_bytes(name: impl Into<String>, bytes: Vec<u8>) -> Result<Self, String> {
        let index = index_xyz(Cursor::new(&bytes))?;
        Ok(Self {
            name: name.into(),
            source: Source::Memory(Arc::new(bytes)),
            index,
        })
    }

    /// ファイルから（ネイティブ）。索引を作るために 1 回走査するが、座標は保持せず、再生のときに必要なフレームだけ読む。
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_file(path: impl AsRef<std::path::Path>) -> Result<Self, String> {
        let path = path.as_ref();
        let file = std::fs::File::open(path)
            .map_err(|e| format!("Failed to open trajectory file: {e}"))?;
        let index = index_xyz(std::io::BufReader::new(file))?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        Ok(Self {
            name,
            source: Source::File(path.to_path_buf()),
            index,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn num_atoms(&self) -> usize {
        self.index.num_atoms
    }

    pub fn frame_count(&self) -> usize {
        self.index.frames.len()
    }

    /// 指定のフレーム（0 始まり）を読んで解析する
    pub fn read_frame(&self, frame: usize) -> Result<FrameData, String> {
        let span = *self.index.frames.get(frame).ok_or_else(|| {
            format!(
                "Frame {} is out of range (the trajectory has {} frames)",
                frame + 1,
                self.index.frames.len()
            )
        })?;
        let bytes = match &self.source {
            Source::Memory(data) => {
                let (s, e) = (span.start as usize, (span.start + span.len) as usize);
                data.get(s..e)
                    .ok_or_else(|| "The trajectory data is shorter than its index".to_string())?
                    .to_vec()
            }
            #[cfg(not(target_arch = "wasm32"))]
            Source::File(path) => {
                use std::io::{Read, Seek, SeekFrom};
                let mut file = std::fs::File::open(path)
                    .map_err(|e| format!("Failed to open trajectory file: {e}"))?;
                file.seek(SeekFrom::Start(span.start))
                    .map_err(|e| format!("Seek error: {e}"))?;
                let mut buf = Vec::with_capacity(span.len as usize);
                file.take(span.len)
                    .read_to_end(&mut buf)
                    .map_err(|e| format!("Read error: {e}"))?;
                buf
            }
        };
        let text = std::str::from_utf8(&bytes)
            .map_err(|e| format!("Frame {} is not valid UTF-8: {e}", frame + 1))?;
        parse_xyz_frame(text, self.index.num_atoms, frame as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const THREE_FRAMES: &str = "\
2
step=0 first
C 0.0 0.0 0.0
O 1.0 0.0 0.0
2
step=10
C 0.0 0.5 0.0
O 1.0 0.5 0.0
2
no step here
C 0.0 1.0 0.0
O 1.0 1.0 0.0
";

    #[test]
    fn test_index_finds_every_frame_and_its_byte_range() {
        let idx = index_xyz(Cursor::new(THREE_FRAMES)).unwrap();
        assert_eq!(idx.num_atoms, 2);
        assert_eq!(idx.frames.len(), 3);
        // 各フレームの範囲を足すと、全体になる（空行なし）
        let total: u64 = idx.frames.iter().map(|f| f.len).sum();
        assert_eq!(total as usize, THREE_FRAMES.len());
        assert_eq!(idx.frames[0].start, 0);
        assert_eq!(idx.frames[1].start, idx.frames[0].len);
        let f1 = &THREE_FRAMES
            [idx.frames[1].start as usize..(idx.frames[1].start + idx.frames[1].len) as usize];
        assert!(f1.starts_with("2\nstep=10"));
    }

    #[test]
    fn test_trajectory_reads_each_frame_correctly() {
        let t = XyzTrajectory::from_bytes("t.xyz", THREE_FRAMES.as_bytes().to_vec()).unwrap();
        assert_eq!((t.frame_count(), t.num_atoms()), (3, 2));
        let f0 = t.read_frame(0).unwrap();
        assert_eq!(f0.step, 0);
        assert_eq!(f0.positions, vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]);
        let f1 = t.read_frame(1).unwrap();
        assert_eq!(f1.step, 10);
        assert_eq!(f1.positions, vec![[0.0, 0.5, 0.0], [1.0, 0.5, 0.0]]);
        // ステップがなければ、フレームの番号
        let f2 = t.read_frame(2).unwrap();
        assert_eq!(f2.step, 2);
        assert_eq!(f2.positions[1], [1.0, 1.0, 0.0]);
        assert!(t.read_frame(3).is_err());
    }

    #[test]
    fn test_blank_lines_between_and_after_frames_are_ignored() {
        let text = THREE_FRAMES.replace("2\nstep=10", "\n2\nstep=10") + "\n\n";
        let t = XyzTrajectory::from_bytes("t.xyz", text.into_bytes()).unwrap();
        assert_eq!(t.frame_count(), 3);
        assert_eq!(t.read_frame(1).unwrap().step, 10);
        assert_eq!(t.read_frame(2).unwrap().positions[0], [0.0, 1.0, 0.0]);
    }

    #[test]
    fn test_atom_count_mismatch_between_frames_is_an_error() {
        let text = "2\nc\nC 0 0 0\nO 1 0 0\n3\nc\nC 0 0 0\nO 1 0 0\nH 2 0 0\n";
        let err = index_xyz(Cursor::new(text)).unwrap_err();
        assert!(err.contains("Frame 2") && err.contains("3 atoms"), "{err}");
    }

    #[test]
    fn test_incomplete_or_malformed_files_are_errors() {
        assert!(index_xyz(Cursor::new("")).is_err(), "empty");
        assert!(
            index_xyz(Cursor::new("hello\nworld\n")).is_err(),
            "no count"
        );
        let truncated = "2\nc\nC 0 0 0\n";
        let err = index_xyz(Cursor::new(truncated)).unwrap_err();
        assert!(err.contains("incomplete"), "{err}");
        let no_comment = "2\n";
        assert!(index_xyz(Cursor::new(no_comment)).is_err());
        let bad_coord = "1\nc\nC 0 x 0\n";
        let t = XyzTrajectory::from_bytes("t", bad_coord.as_bytes().to_vec()).unwrap();
        assert!(t.read_frame(0).unwrap_err().contains("bad coordinate"));
        let few = "1\nc\nC 0 0\n";
        let t = XyzTrajectory::from_bytes("t", few.as_bytes().to_vec()).unwrap();
        assert!(t.read_frame(0).is_err());
    }

    #[test]
    fn test_parse_step_variants() {
        assert_eq!(parse_step("step=12"), Some(12));
        assert_eq!(parse_step("Step: 7 E=-1.0"), Some(7));
        assert_eq!(parse_step("i = 4, step = 30"), Some(30));
        assert_eq!(parse_step("no number"), None);
        assert_eq!(parse_step("step=abc"), None);
    }

    #[test]
    fn test_reads_the_bundled_multi_frame_file_from_memory_and_from_disk() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/multiframe.xyz");
        let bytes = std::fs::read(path).expect("multiframe.xyz");
        let mem = XyzTrajectory::from_bytes("multiframe.xyz", bytes).unwrap();
        assert_eq!((mem.frame_count(), mem.num_atoms()), (5, 3));
        for i in 0..5 {
            let f = mem.read_frame(i).unwrap();
            assert_eq!(f.step, i as u32 * 10);
            // 3 つの原子が、フレームごとに x 方向に 0.5 Å ずつ動く
            assert_eq!(f.positions[0], [0.5 * i as f32, 0.0, 0.0]);
            assert_eq!(f.positions[1], [1.5 + 0.5 * i as f32, 0.0, 0.0]);
            assert_eq!(f.positions[2], [0.5 * i as f32, 1.2, 0.0]);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let disk = XyzTrajectory::from_file(path).unwrap();
            assert_eq!(disk.name(), "multiframe.xyz");
            assert_eq!(disk.frame_count(), 5);
            for i in 0..5 {
                assert_eq!(disk.read_frame(i).unwrap(), mem.read_frame(i).unwrap());
            }
        }
    }

    #[test]
    fn test_from_file_reports_a_missing_file() {
        #[cfg(not(target_arch = "wasm32"))]
        assert!(XyzTrajectory::from_file("/no/such/dir/x.xyz").is_err());
    }
}
