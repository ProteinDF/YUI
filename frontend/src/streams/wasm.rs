use crate::Message;
use crate::log_viewer;
use crate::terminal_pane;
use iced::futures::{SinkExt, StreamExt};

/// 秘密を付けて、WebSocket に接続する。ブラウザは、WebSocket にヘッダーを付けられないので、サブプロトコル
/// `yui.token.<秘密>` で渡す（URL には入れない）。`backend` は、受け付けたら、そのサブプロトコルを選び返す。
fn open_authenticated(url: &str) -> Result<gloo_net::websocket::futures::WebSocket, ()> {
    use gloo_net::websocket::futures::WebSocket;
    match crate::backend_auth::ws_protocol() {
        Some(protocol) => WebSocket::open_with_protocol(url, &protocol),
        None => WebSocket::open(url),
    }
    .map_err(|_| ())
}

/// バックグラウンドの非同期WebSocket接続ストリーム（WASM用）
pub(crate) fn collab_subscription(url: String) -> iced::Subscription<Message> {
    iced::Subscription::run_with(url, connect_collab_stream)
}

#[allow(clippy::ptr_arg)]
fn connect_collab_stream(url: &String) -> iced::futures::stream::BoxStream<'static, Message> {
    let url = url.clone();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Message>(100);

    wasm_bindgen_futures::spawn_local(async move {
        use gloo_net::websocket::Message as WsMessage;
        use yui_core::packing::unpack_collab_packet;

        web_sys::console::log_1(&format!("[YUI WASM WS] Connecting collab stream to {url}").into());
        let Ok(ws_stream) = open_authenticated(&url) else {
            web_sys::console::log_1(
                &format!("[YUI WASM WS] Failed to connect collab stream to {url}").into(),
            );
            let _ = tx.send(Message::CollabDisconnected).await;
            return;
        };

        web_sys::console::log_1(&format!("[YUI WASM WS] Collab stream connected to {url}").into());
        let _ = tx.send(Message::CollabConnected).await;
        let (_write, mut read) = ws_stream.split();

        while let Some(msg_res) = read.next().await {
            if let Ok(msg) = msg_res {
                if let WsMessage::Bytes(bin) = msg
                    && let Ok((_header, delta)) = unpack_collab_packet(&bin)
                    && tx.send(Message::CollabDeltaReceived(delta)).await.is_err()
                {
                    break;
                }
            } else {
                break;
            }
        }

        web_sys::console::log_1(&format!("[YUI WASM WS] Collab stream disconnected: {url}").into());
        let _ = tx.send(Message::CollabDisconnected).await;
    });

    iced::stream::channel(
        100,
        |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
            while let Some(msg) = rx.recv().await {
                if output.send(msg).await.is_err() {
                    break;
                }
            }
        },
    )
    .boxed()
}

/// バックグラウンドのログファイル監視 WebSocket 接続ストリーム（WASM用）
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
    let (tx, mut rx) = tokio::sync::mpsc::channel::<log_viewer::LogViewerMessage>(100);

    wasm_bindgen_futures::spawn_local(async move {
        use gloo_net::websocket::Message as WsMessage;

        web_sys::console::log_1(&format!("[YUI WASM WS] Connecting log stream to {url}").into());
        let Ok(ws_stream) = open_authenticated(&url) else {
            web_sys::console::log_1(
                &format!("[YUI WASM WS] Failed to connect log stream to {url}").into(),
            );
            let _ = tx
                .send(log_viewer::LogViewerMessage::StreamMessageReceived(
                    log_viewer::LogStreamMessage::Error(format!(
                        "Failed to connect to log stream at {url}"
                    )),
                ))
                .await;
            let _ = tx
                .send(log_viewer::LogViewerMessage::ConnectionClosed)
                .await;
            return;
        };

        web_sys::console::log_1(&format!("[YUI WASM WS] Log stream connected to {url}").into());
        let (_write, mut read) = ws_stream.split();

        while let Some(msg_res) = read.next().await {
            if let Ok(msg) = msg_res {
                if let WsMessage::Text(text) = msg
                    && let Ok(stream_msg) =
                        serde_json::from_str::<log_viewer::LogStreamMessage>(&text)
                    && tx
                        .send(log_viewer::LogViewerMessage::StreamMessageReceived(
                            stream_msg,
                        ))
                        .await
                        .is_err()
                {
                    break;
                }
            } else {
                break;
            }
        }

        web_sys::console::log_1(&format!("[YUI WASM WS] Log stream closed: {url}").into());
        let _ = tx
            .send(log_viewer::LogViewerMessage::ConnectionClosed)
            .await;
    });

    iced::stream::channel(
        100,
        |mut output: iced::futures::channel::mpsc::Sender<log_viewer::LogViewerMessage>| async move {
            while let Some(msg) = rx.recv().await {
                if output.send(msg).await.is_err() {
                    break;
                }
            }
        },
    )
    .boxed()
}

/// バックグラウンドのターミナル PTY WebSocket 接続ストリーム（WASM用）
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
    let (tx, mut rx) = tokio::sync::mpsc::channel::<terminal_pane::TerminalPaneMessage>(100);

    wasm_bindgen_futures::spawn_local(async move {
        use gloo_net::websocket::Message as WsMessage;

        web_sys::console::log_1(
            &format!("[YUI WASM WS] Connecting terminal stream to {url}").into(),
        );
        let Ok(ws_stream) = open_authenticated(&url) else {
            web_sys::console::log_1(
                &format!("[YUI WASM WS] Failed to connect terminal stream to {url}").into(),
            );
            let _ = tx
                .send(terminal_pane::TerminalPaneMessage::Error(format!(
                    "Failed to connect to terminal session at {url}"
                )))
                .await;
            let _ = tx
                .send(terminal_pane::TerminalPaneMessage::Disconnected)
                .await;
            return;
        };

        web_sys::console::log_1(&format!("[YUI WASM WS] Terminal connected to {url}").into());
        let _ = tx.send(terminal_pane::TerminalPaneMessage::Connected).await;
        let (mut write, mut read) = ws_stream.split();

        // クライアント側からの入力送信用チャネル
        let (input_tx, mut input_rx) =
            tokio::sync::mpsc::unbounded_channel::<terminal_pane::TerminalOut>();
        let _ = tx
            .send(terminal_pane::TerminalPaneMessage::ChannelReady(input_tx))
            .await;

        // 入力送信タスク
        let write_fut = async move {
            while let Some(out) = input_rx.recv().await {
                let msg = match out {
                    terminal_pane::TerminalOut::Input(bytes) => WsMessage::Bytes(bytes),
                    terminal_pane::TerminalOut::Resize { rows, cols } => {
                        WsMessage::Text(terminal_pane::TerminalOut::resize_json(rows, cols))
                    }
                };
                if write.send(msg).await.is_err() {
                    break;
                }
            }
        };

        // 受信タスク
        let read_fut = async {
            while let Some(msg_res) = read.next().await {
                if let Ok(msg) = msg_res {
                    match msg {
                        WsMessage::Bytes(bin) => {
                            if tx
                                .send(terminal_pane::TerminalPaneMessage::PtyOutputReceived(bin))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        WsMessage::Text(text) => {
                            if tx
                                .send(terminal_pane::TerminalPaneMessage::PtyOutputReceived(
                                    text.into_bytes(),
                                ))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                } else {
                    break;
                }
            }
        };

        iced::futures::pin_mut!(write_fut, read_fut);
        iced::futures::future::select(write_fut, read_fut).await;

        web_sys::console::log_1(&format!("[YUI WASM WS] Terminal disconnected: {url}").into());
        let _ = tx
            .send(terminal_pane::TerminalPaneMessage::Disconnected)
            .await;
    });

    iced::stream::channel(
        100,
        |mut output: iced::futures::channel::mpsc::Sender<terminal_pane::TerminalPaneMessage>| async move {
            while let Some(msg) = rx.recv().await {
                if output.send(msg).await.is_err() {
                    break;
                }
            }
        },
    )
    .boxed()
}
