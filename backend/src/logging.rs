//! バックエンド自身の動作ログのファイル出力（標準出力にも従来どおり出す）。
//!
//! 場所: `YUI_LOG_DIR` があればそれ。なければ OS の標準（Linux: `$XDG_STATE_HOME/yui/logs`、
//! macOS: `~/Library/Logs/yui`、Windows: `%LOCALAPPDATA%\yui\logs`）。日ごとに切り替え、7 日分を残す。
//! 書くのは今までと同じ内容（トークンや URL のクエリは書かない。パスだけ）。

use std::path::PathBuf;

/// 残す日数
const KEEP_DAYS: usize = 7;

/// ログのフォルダ。`override_dir` は `YUI_LOG_DIR`。決められなければ `None`
pub fn log_dir(override_dir: Option<&str>) -> Option<PathBuf> {
    if let Some(d) = override_dir.map(str::trim).filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    #[cfg(target_os = "macos")]
    {
        dirs::home_dir().map(|h| h.join("Library").join("Logs").join("yui"))
    }
    #[cfg(target_os = "windows")]
    {
        dirs::data_local_dir().map(|d| d.join("yui").join("logs"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        dirs::state_dir()
            .or_else(|| dirs::home_dir().map(|h| h.join(".local").join("state")))
            .map(|d| d.join("yui").join("logs"))
    }
}

/// フォルダを作る。Unix では利用者だけが入れる（0700）ようにする
fn create_dir(dir: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(dir)
    }
}

/// 標準出力とファイルに出すログを始める。返す値は、終わるまで持っておく（落とすと残りが書かれない）。
/// ファイルを作れなくても、標準出力だけで動き続ける（理由は標準エラー出力に出す）。
pub fn init(
    override_dir: Option<&str>,
) -> Option<(tracing_appender::non_blocking::WorkerGuard, PathBuf)> {
    use tracing_subscriber::{Layer, layer::SubscriberExt, util::SubscriberInitExt};

    // 既定は info。RUST_LOG で変えられる
    let filter = || {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
    };
    let opened = log_dir(override_dir)
        .ok_or_else(|| "no place for the log files".to_string())
        .and_then(|dir| {
            create_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            let appender = tracing_appender::rolling::Builder::new()
                .rotation(tracing_appender::rolling::Rotation::DAILY)
                .filename_prefix("backend")
                .filename_suffix("log")
                .max_log_files(KEEP_DAYS)
                .build(&dir)
                .map_err(|e| format!("{}: {e}", dir.display()))?;
            Ok((tracing_appender::non_blocking(appender), dir))
        });
    match opened {
        Ok(((writer, guard), dir)) => {
            tracing_subscriber::registry()
                .with(tracing_subscriber::fmt::layer().with_filter(filter()))
                .with(
                    tracing_subscriber::fmt::layer()
                        .with_ansi(false)
                        .with_writer(writer)
                        .with_filter(filter()),
                )
                .init();
            Some((guard, dir))
        }
        Err(e) => {
            tracing_subscriber::registry()
                .with(tracing_subscriber::fmt::layer().with_filter(filter()))
                .init();
            eprintln!("Log files are disabled: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_override_wins() {
        assert_eq!(log_dir(Some("/x/logs")), Some(PathBuf::from("/x/logs")));
        assert_ne!(log_dir(Some("  ")), Some(PathBuf::from("  ")));
    }

    #[test]
    fn the_default_ends_with_yui() {
        let d = log_dir(None).unwrap();
        assert!(d.to_string_lossy().contains("yui"), "{d:?}");
    }

    #[cfg(unix)]
    #[test]
    fn the_folder_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        let d = t.path().join("a").join("logs");
        create_dir(&d).unwrap();
        let mode = std::fs::metadata(&d).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }
}
