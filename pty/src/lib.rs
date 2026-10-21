use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tracing::{info, warn};

/// クライアントからターミナルセッションへの指示
#[derive(Debug)]
pub enum TerminalCommand {
    /// キー入力・送信バイト列
    Input(Vec<u8>),
    /// ターミナルサイズ変更
    Resize { rows: u16, cols: u16 },
}

/// ターミナルPTYセッションのハンドル
pub struct PtySessionHandle {
    /// クライアントからの入力送信チャンネル
    pub input_tx: mpsc::Sender<TerminalCommand>,
    /// PTYからの出力受信チャンネル
    pub output_rx: mpsc::Receiver<Vec<u8>>,
    /// マスターPTYの共有参照（リサイズ操作用）
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
}

impl PtySessionHandle {
    /// ターミナルのリサイズ要求
    pub fn resize(
        &self,
        rows: u16,
        cols: u16,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let master = self.master.lock().unwrap();
        master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }
}

/// PTYセッションを開始する
///
/// システムのデフォルトシェル（$SHELL、または /bin/sh）を子プロセスとして起動し、
/// 入出力ストリームを非同期チャンネルと接続する。
pub fn spawn_pty_session(
    initial_rows: u16,
    initial_cols: u16,
) -> Result<PtySessionHandle, Box<dyn std::error::Error + Send + Sync>> {
    let pty_system = native_pty_system();

    let pty_size = PtySize {
        rows: initial_rows.max(1),
        cols: initial_cols.max(1),
        pixel_width: 0,
        pixel_height: 0,
    };

    let pair = pty_system.openpty(pty_size)?;

    // 使用するシェルを決定
    let shell = std::env::var("SHELL").unwrap_or_else(|_| {
        #[cfg(target_os = "windows")]
        {
            "powershell.exe".to_string()
        }
        #[cfg(not(target_os = "windows"))]
        {
            "/bin/sh".to_string()
        }
    });

    let mut cmd = CommandBuilder::new(&shell);
    #[cfg(not(target_os = "windows"))]
    {
        // TERM環境変数を設定
        cmd.env("TERM", "xterm-256color");
    }

    let mut child = pair.slave.spawn_command(cmd)?;
    // スレーブ側はプロセス起動後にクローズしてMaster側のみ保持
    drop(pair.slave);

    let master: Arc<Mutex<Box<dyn MasterPty + Send>>> = Arc::new(Mutex::new(pair.master));

    // PTY出力読み取り用チャンネル
    let (output_tx, output_rx) = mpsc::channel::<Vec<u8>>(256);
    // クライアント入力受信チャンネル
    let (input_tx, mut input_rx) = mpsc::channel::<TerminalCommand>(256);

    // 1. 出力読み取りスレッド (PTY Master -> output_tx)
    let master_reader = {
        let master_guard = master.lock().unwrap();
        master_guard.try_clone_reader()?
    };

    std::thread::Builder::new()
        .name("pty-reader".to_string())
        .spawn(move || {
            let mut reader = master_reader;
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        info!("PTY reader reached EOF");
                        break;
                    }
                    Ok(n) => {
                        let data = buf[..n].to_vec();
                        if output_tx.blocking_send(data).is_err() {
                            // 受信側が切断された場合
                            break;
                        }
                    }
                    Err(e) => {
                        // 子プロセス終了時など
                        warn!("PTY reader error or closed: {}", e);
                        break;
                    }
                }
            }
        })?;

    // 2. 入力書き込みスレッド (input_rx -> PTY Master)
    let master_writer = {
        let master_guard = master.lock().unwrap();
        master_guard.take_writer()?
    };
    let master_for_resize = Arc::clone(&master);

    std::thread::Builder::new()
        .name("pty-writer".to_string())
        .spawn(move || {
            let mut writer = master_writer;
            while let Some(cmd) = input_rx.blocking_recv() {
                match cmd {
                    TerminalCommand::Input(data) => {
                        if let Err(e) = writer.write_all(&data) {
                            warn!("PTY writer write failed: {}", e);
                            break;
                        }
                        if let Err(e) = writer.flush() {
                            warn!("PTY writer flush failed: {}", e);
                            break;
                        }
                    }
                    TerminalCommand::Resize { rows, cols } => {
                        if let Ok(master_guard) = master_for_resize.lock() {
                            let _ = master_guard.resize(PtySize {
                                rows: rows.max(1),
                                cols: cols.max(1),
                                pixel_width: 0,
                                pixel_height: 0,
                            });
                        }
                    }
                }
            }

            // セッション終了時に子プロセスを終了
            let _ = child.kill();
        })?;

    Ok(PtySessionHandle {
        input_tx,
        output_rx,
        master,
    })
}
