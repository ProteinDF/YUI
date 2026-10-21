//! ログファイルの tail -f 風ファイル監視および差分行ストリーミングモジュール
//!
//! architecture.md 13.3節に基づき、`notify` クレートによるファイル変更検知と
//! バイトオフセット管理による差分行抽出を提供します。

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::mpsc;

/// クライアントへ送信するログストリームメッセージ
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum LogStreamMessage {
    /// 接続成功およびファイル存在状態
    Connected { path: String, file_exists: bool },
    /// 新規追記された行リスト（差分のみ）
    Lines(Vec<String>),
    /// ファイル未作成による待機状態
    Waiting { path: String },
    /// エラー通知
    Error(String),
}

/// ログファイル監視および末尾差分読み出しエンジン
pub struct LogFileTailer {
    path: PathBuf,
    current_offset: u64,
    initial_tail_lines: usize,
    partial_line: String,
}

impl LogFileTailer {
    pub fn new(path: impl Into<PathBuf>, initial_tail_lines: usize) -> Self {
        Self {
            path: path.into(),
            current_offset: 0,
            initial_tail_lines,
            partial_line: String::new(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 初期状態の読み出し（ファイルが存在すれば直近 N 行を取得）
    pub fn read_initial(&mut self) -> Result<(bool, Vec<String>), std::io::Error> {
        if !self.path.exists() {
            return Ok((false, Vec::new()));
        }

        let mut file = std::fs::File::open(&self.path)?;
        let file_len = file.metadata()?.len();

        if file_len == 0 {
            self.current_offset = 0;
            return Ok((true, Vec::new()));
        }

        // ファイル全体または末尾付近を読み込み
        let read_size = file_len.min(64 * 1024); // 最大64KBから末尾行を検索
        let start_pos = file_len.saturating_sub(read_size);
        file.seek(SeekFrom::Start(start_pos))?;

        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;

        let text = String::from_utf8_lossy(&buffer);
        let all_lines: Vec<&str> = text.lines().collect();

        let tail_lines: Vec<String> = if all_lines.len() > self.initial_tail_lines {
            all_lines[all_lines.len() - self.initial_tail_lines..]
                .iter()
                .map(|s| s.to_string())
                .collect()
        } else {
            all_lines.iter().map(|s| s.to_string()).collect()
        };

        self.current_offset = file_len;
        self.partial_line.clear();

        Ok((true, tail_lines))
    }

    /// 前回読み込み位置からの追記差分行を読み出す
    pub fn read_new_lines(&mut self) -> Result<Vec<String>, std::io::Error> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let mut file = std::fs::File::open(&self.path)?;
        let file_len = file.metadata()?.len();

        // ログローテーションや切り詰め検知
        if file_len < self.current_offset {
            self.current_offset = 0;
            self.partial_line.clear();
        }

        if file_len == self.current_offset {
            return Ok(Vec::new());
        }

        file.seek(SeekFrom::Start(self.current_offset))?;
        let bytes_to_read = (file_len - self.current_offset) as usize;
        let mut buffer = vec![0u8; bytes_to_read];
        file.read_exact(&mut buffer)?;

        self.current_offset = file_len;

        let content = String::from_utf8_lossy(&buffer);
        let combined = format!("{}{}", self.partial_line, content);

        let ends_with_newline = combined.ends_with('\n') || combined.ends_with("\r\n");
        let mut lines: Vec<String> = combined.lines().map(|s| s.to_string()).collect();

        if !ends_with_newline && !lines.is_empty() {
            // 末尾行が途中の場合は次回の読み込みまで保持
            self.partial_line = lines.pop().unwrap_or_default();
        } else {
            self.partial_line.clear();
        }

        Ok(lines)
    }
}

/// バックグラウンドタスクでファイル変更を監視し、非同期チャンネルへ `LogStreamMessage` を配信する
pub fn spawn_log_stream(
    path: PathBuf,
    initial_tail_lines: usize,
) -> mpsc::Receiver<LogStreamMessage> {
    let (tx, rx) = mpsc::channel(100);

    tokio::spawn(async move {
        let mut tailer = LogFileTailer::new(path.clone(), initial_tail_lines);

        // 初期読み出し
        match tailer.read_initial() {
            Ok((true, lines)) => {
                let _ = tx
                    .send(LogStreamMessage::Connected {
                        path: path.to_string_lossy().to_string(),
                        file_exists: true,
                    })
                    .await;
                if !lines.is_empty() {
                    let _ = tx.send(LogStreamMessage::Lines(lines)).await;
                }
            }
            Ok((false, _)) => {
                let _ = tx
                    .send(LogStreamMessage::Waiting {
                        path: path.to_string_lossy().to_string(),
                    })
                    .await;
            }
            Err(e) => {
                let _ = tx
                    .send(LogStreamMessage::Error(format!("Failed to open file: {e}")))
                    .await;
                return;
            }
        }

        // notify によるファイル・ディレクトリ監視のセットアップ
        let (notify_tx, mut notify_rx) = mpsc::channel(100);
        let watcher_res = RecommendedWatcher::new(
            move |res| {
                let _ = notify_tx.blocking_send(res);
            },
            notify::Config::default().with_poll_interval(Duration::from_millis(200)),
        );

        let mut watcher = match watcher_res {
            Ok(w) => w,
            Err(e) => {
                let _ = tx
                    .send(LogStreamMessage::Error(format!(
                        "Failed to initialize file watcher: {e}"
                    )))
                    .await;
                return;
            }
        };

        // 監視対象パスまたは親ディレクトリを登録
        let watch_target = if path.exists() {
            path.clone()
        } else {
            path.parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf()
        };

        let _ = watcher.watch(&watch_target, RecursiveMode::NonRecursive);

        // 監視ループ（非同期イベント駆動 + フォールバック定期ポーリング）
        let mut interval = tokio::time::interval(Duration::from_millis(300));

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    // 定期ポーリングによる検知（ファイル未存在から作成された場合や安全弁）
                    if tailer.path().exists() {
                        match tailer.read_new_lines() {
                            Ok(lines) => {
                                if !lines.is_empty() && tx.send(LogStreamMessage::Lines(lines)).await.is_err() {
                                    break;
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(LogStreamMessage::Error(e.to_string())).await;
                                break;
                            }
                        }
                    }
                }
                Some(_event) = notify_rx.recv() => {
                    // notify からのファイル更新・作成通知
                    if tailer.path().exists() {
                        match tailer.read_new_lines() {
                            Ok(lines) => {
                                if !lines.is_empty() && tx.send(LogStreamMessage::Lines(lines)).await.is_err() {
                                    break;
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(LogStreamMessage::Error(e.to_string())).await;
                                break;
                            }
                        }
                    }
                }
            }

            if tx.is_closed() {
                break;
            }
        }
    });

    rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_log_file_tailer_initial_read_and_append() {
        let temp_dir = tempfile::tempdir().unwrap();
        let log_file = temp_dir.path().join("test_job.log");

        // 1. ファイルが存在しない場合
        let mut tailer = LogFileTailer::new(&log_file, 5);
        let (exists, lines) = tailer.read_initial().unwrap();
        assert!(!exists);
        assert!(lines.is_empty());

        // 2. ファイルを作成し初期行を書き込み
        {
            let mut f = std::fs::File::create(&log_file).unwrap();
            writeln!(f, "Line 1: Job started").unwrap();
            writeln!(f, "Line 2: Iteration 1").unwrap();
            writeln!(f, "Line 3: Iteration 2").unwrap();
        }

        let (exists, initial_lines) = tailer.read_initial().unwrap();
        assert!(exists);
        assert_eq!(initial_lines.len(), 3);
        assert_eq!(initial_lines[0], "Line 1: Job started");
        assert_eq!(initial_lines[2], "Line 3: Iteration 2");

        // 3. ファイルへの追記
        {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&log_file)
                .unwrap();
            writeln!(f, "Line 4: Iteration 3").unwrap();
            writeln!(f, "Line 5: Converged").unwrap();
        }

        let new_lines = tailer.read_new_lines().unwrap();
        assert_eq!(new_lines.len(), 2);
        assert_eq!(new_lines[0], "Line 4: Iteration 3");
        assert_eq!(new_lines[1], "Line 5: Converged");

        // 4. 追加追記がない場合は空
        let empty_lines = tailer.read_new_lines().unwrap();
        assert!(empty_lines.is_empty());
    }

    #[tokio::test]
    async fn test_spawn_log_stream_streaming() {
        let temp_dir = tempfile::tempdir().unwrap();
        let log_file = temp_dir.path().join("stream.log");

        // 初期ファイル作成
        {
            let mut f = std::fs::File::create(&log_file).unwrap();
            writeln!(f, "Init line 1").unwrap();
        }

        let mut rx = spawn_log_stream(log_file.clone(), 10);

        // 最初のメッセージは Connected
        let msg1 = rx.recv().await.expect("Expected connected message");
        match msg1 {
            LogStreamMessage::Connected { file_exists, .. } => {
                assert!(file_exists);
            }
            other => panic!("Unexpected message: {:?}", other),
        }

        // 初期行の受信
        let msg2 = rx.recv().await.expect("Expected initial lines");
        match msg2 {
            LogStreamMessage::Lines(lines) => {
                assert_eq!(lines, vec!["Init line 1".to_string()]);
            }
            other => panic!("Unexpected message: {:?}", other),
        }

        // 追記テスト
        {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&log_file)
                .unwrap();
            writeln!(f, "Append line 2").unwrap();
            writeln!(f, "Append line 3").unwrap();
        }

        // ストリームから受信
        let msg3 = rx.recv().await.expect("Expected appended lines");
        match msg3 {
            LogStreamMessage::Lines(lines) => {
                assert_eq!(lines, vec!["Append line 2", "Append line 3"]);
            }
            other => panic!("Unexpected message: {:?}", other),
        }
    }
}
