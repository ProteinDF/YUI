use crate::Message;
use crate::log_viewer;
use crate::terminal_pane;

#[allow(unused_imports)]
use iced::futures::{SinkExt, StreamExt};

/// 秘密（`Authorization: Bearer`）を付けて、WebSocket に接続する（URL には秘密を入れない）
async fn connect_authenticated(
    url: &str,
) -> Result<
    (
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        tokio_tungstenite::tungstenite::handshake::client::Response,
    ),
    tokio_tungstenite::tungstenite::Error,
> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let mut request = url.into_client_request()?;
    if let Some(auth) = crate::backend_auth::bearer()
        && let Ok(value) = auth.parse()
    {
        request.headers_mut().insert(
            tokio_tungstenite::tungstenite::http::header::AUTHORIZATION,
            value,
        );
    }
    tokio_tungstenite::connect_async(request).await
}

/// バックグラウンドの非同期WebSocket接続ストリーム（iced::Subscription）
pub(crate) fn collab_subscription(url: String) -> iced::Subscription<Message> {
    iced::Subscription::run_with(url, connect_collab_stream)
}

#[allow(clippy::ptr_arg)]
fn connect_collab_stream(url: &String) -> iced::futures::stream::BoxStream<'static, Message> {
    let url = url.clone();
    iced::stream::channel(
        100,
        |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
            use futures_util::SinkExt;
            use tokio_tungstenite::tungstenite::Message as WsMessage;
            use yui_core::packing::unpack_collab_packet;

            let Ok((ws_stream, _)) = connect_authenticated(&url).await else {
                let _ = output.send(Message::CollabDisconnected).await;
                return;
            };

            let _ = output.send(Message::CollabConnected).await;
            let (_write, mut read) = ws_stream.split();

            while let Some(Ok(msg)) = read.next().await {
                match msg {
                    WsMessage::Binary(bin) => {
                        if let Ok((_header, delta)) = unpack_collab_packet(&bin)
                            && output
                                .send(Message::CollabDeltaReceived(delta))
                                .await
                                .is_err()
                        {
                            break;
                        }
                    }
                    WsMessage::Close(_) => {
                        break;
                    }
                    _ => {}
                }
            }

            let _ = output.send(Message::CollabDisconnected).await;
        },
    )
    .boxed()
}

/// バックグラウンドのログファイル監視 WebSocket 接続ストリーム
pub(crate) fn log_stream_subscription(
    url: String,
) -> iced::Subscription<log_viewer::LogViewerMessage> {
    iced::Subscription::run_with(url, connect_log_stream)
}

#[allow(clippy::ptr_arg)]
fn connect_log_stream(
    url: &String,
) -> iced::futures::stream::BoxStream<'static, log_viewer::LogViewerMessage> {
    let url = url.clone();
    iced::stream::channel(
        100,
        |mut output: iced::futures::channel::mpsc::Sender<log_viewer::LogViewerMessage>| async move {
            use futures_util::SinkExt;
            use tokio_tungstenite::tungstenite::Message as WsMessage;

            let Ok((ws_stream, _)) = connect_authenticated(&url).await else {
                let _ = output
                    .send(log_viewer::LogViewerMessage::StreamMessageReceived(
                        log_viewer::LogStreamMessage::Error(format!(
                            "Failed to connect to log stream at {url}"
                        )),
                    ))
                    .await;
                let _ = output
                    .send(log_viewer::LogViewerMessage::ConnectionClosed)
                    .await;
                return;
            };

            let (_write, mut read) = ws_stream.split();

            while let Some(Ok(msg)) = read.next().await {
                match msg {
                    WsMessage::Text(text) => {
                        if let Ok(stream_msg) =
                            serde_json::from_str::<log_viewer::LogStreamMessage>(&text)
                            && output
                                .send(log_viewer::LogViewerMessage::StreamMessageReceived(
                                    stream_msg,
                                ))
                                .await
                                .is_err()
                        {
                            break;
                        }
                    }
                    WsMessage::Close(_) => {
                        break;
                    }
                    _ => {}
                }
            }

            let _ = output
                .send(log_viewer::LogViewerMessage::ConnectionClosed)
                .await;
        },
    )
    .boxed()
}

/// バックグラウンドのターミナル PTY WebSocket 接続ストリーム
pub(crate) fn terminal_stream_subscription(
    url: String,
) -> iced::Subscription<terminal_pane::TerminalPaneMessage> {
    iced::Subscription::run_with(url, connect_terminal_stream)
}

#[allow(clippy::ptr_arg)]
fn connect_terminal_stream(
    url: &String,
) -> iced::futures::stream::BoxStream<'static, terminal_pane::TerminalPaneMessage> {
    let url = url.clone();
    iced::stream::channel(
        100,
        |mut output: iced::futures::channel::mpsc::Sender<terminal_pane::TerminalPaneMessage>| async move {
            use futures_util::SinkExt;
            use tokio_tungstenite::tungstenite::Message as WsMessage;

            let Ok((ws_stream, _)) = connect_authenticated(&url).await else {
                let _ = output
                    .send(terminal_pane::TerminalPaneMessage::Error(format!(
                        "Failed to connect to terminal session at {url}"
                    )))
                    .await;
                let _ = output
                    .send(terminal_pane::TerminalPaneMessage::Disconnected)
                    .await;
                return;
            };

            let _ = output.send(terminal_pane::TerminalPaneMessage::Connected).await;
            let (mut write, mut read) = ws_stream.split();

            // クライアント側からの入力送信用チャネル
            let (input_tx, mut input_rx) =
                tokio::sync::mpsc::unbounded_channel::<terminal_pane::TerminalOut>();
            let _ = output
                .send(terminal_pane::TerminalPaneMessage::ChannelReady(input_tx))
                .await;

            // 送信タスク
            let write_task = tokio::spawn(async move {
                while let Some(out) = input_rx.recv().await {
                    let msg = match out {
                        terminal_pane::TerminalOut::Input(bytes) => WsMessage::Binary(bytes.into()),
                        terminal_pane::TerminalOut::Resize { rows, cols } => {
                            WsMessage::Text(terminal_pane::TerminalOut::resize_json(rows, cols).into())
                        }
                    };
                    if write.send(msg).await.is_err() {
                        break;
                    }
                }
            });

            // 受信ループ（PTY 出力バイト列）
            while let Some(Ok(msg)) = read.next().await {
                match msg {
                    WsMessage::Binary(bin) => {
                        if output
                            .send(terminal_pane::TerminalPaneMessage::PtyOutputReceived(
                                bin.to_vec(),
                            ))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    WsMessage::Text(text) => {
                        if output
                            .send(terminal_pane::TerminalPaneMessage::PtyOutputReceived(
                                text.as_bytes().to_vec(),
                            ))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    WsMessage::Close(_) => {
                        break;
                    }
                    _ => {}
                }
            }

            write_task.abort();
            let _ = output
                .send(terminal_pane::TerminalPaneMessage::Disconnected)
                .await;
        },
    )
    .boxed()
}

// =============================================================================

/// 同じマシンのシェル（standalone の端末）の購読。backend を使わず、`yui-pty` で PTY を開く。
pub(crate) fn local_terminal_subscription(
    rows: u16,
    cols: u16,
) -> iced::Subscription<terminal_pane::TerminalPaneMessage> {
    iced::Subscription::run_with((rows, cols), connect_local_terminal)
}

#[allow(clippy::ptr_arg)]
fn connect_local_terminal(
    size: &(u16, u16),
) -> iced::futures::stream::BoxStream<'static, terminal_pane::TerminalPaneMessage> {
    let (rows, cols) = *size;
    iced::stream::channel(
        100,
        move |mut output: iced::futures::channel::mpsc::Sender<
            terminal_pane::TerminalPaneMessage,
        >| async move {
            use futures_util::SinkExt;

            let mut session = match yui_pty::spawn_pty_session(rows, cols) {
                Ok(session) => session,
                Err(e) => {
                    let _ = output
                        .send(terminal_pane::TerminalPaneMessage::Error(format!(
                            "Failed to start the local shell: {e}"
                        )))
                        .await;
                    let _ = output
                        .send(terminal_pane::TerminalPaneMessage::Disconnected)
                        .await;
                    return;
                }
            };

            let _ = output
                .send(terminal_pane::TerminalPaneMessage::Connected)
                .await;

            // クライアント側からの入力（キー、大きさ）を、PTY の指示に変えて送る
            let (input_tx, mut input_rx) =
                tokio::sync::mpsc::unbounded_channel::<terminal_pane::TerminalOut>();
            let _ = output
                .send(terminal_pane::TerminalPaneMessage::ChannelReady(input_tx))
                .await;
            let pty_input = session.input_tx.clone();
            let write_task = tokio::spawn(async move {
                while let Some(out) = input_rx.recv().await {
                    if pty_input.send(terminal_pane_to_pty(out)).await.is_err() {
                        break;
                    }
                }
            });

            // PTY の出力を、端末の画面へ流す
            while let Some(bytes) = session.output_rx.recv().await {
                if output
                    .send(terminal_pane::TerminalPaneMessage::PtyOutputReceived(bytes))
                    .await
                    .is_err()
                {
                    break;
                }
            }

            write_task.abort();
            let _ = output
                .send(terminal_pane::TerminalPaneMessage::Disconnected)
                .await;
        },
    )
    .boxed()
}

/// 端末の画面からの送信（`TerminalOut`）を、PTY への指示（`TerminalCommand`）に変える
fn terminal_pane_to_pty(out: terminal_pane::TerminalOut) -> yui_pty::TerminalCommand {
    match out {
        terminal_pane::TerminalOut::Input(bytes) => yui_pty::TerminalCommand::Input(bytes),
        terminal_pane::TerminalOut::Resize { rows, cols } => {
            yui_pty::TerminalCommand::Resize { rows, cols }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::terminal_pane_to_pty;
    use crate::terminal_pane::TerminalOut;

    #[test]
    fn test_terminal_out_becomes_the_pty_command() {
        assert!(matches!(
            terminal_pane_to_pty(TerminalOut::Input(b"ls\r".to_vec())),
            yui_pty::TerminalCommand::Input(bytes) if bytes == b"ls\r"
        ));
        assert!(matches!(
            terminal_pane_to_pty(TerminalOut::Resize {
                rows: 30,
                cols: 100
            }),
            yui_pty::TerminalCommand::Resize {
                rows: 30,
                cols: 100
            }
        ));
    }
}
