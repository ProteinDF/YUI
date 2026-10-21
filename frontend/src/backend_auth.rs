//! `backend` に渡す秘密（トークン）の保管と、使い方（architecture.md 7.4節、フェーズ6j-ii）
//!
//! 秘密は、REST では `Authorization: Bearer <秘密>` で、WebSocket では同じヘッダー（デスクトップ）か、
//! サブプロトコル `yui.token.<秘密>`（ブラウザは、WebSocket にヘッダーを付けられない）で渡す。URL には入れない。
//!
//! 秘密の入手:
//! - デスクトップ: `--token <秘密>`、環境変数 `YUI_TOKEN`、`backend` が書く接続ファイル（`<設定フォルダ>/yui/backend.json`）の順
//! - ブラウザ: `backend` が表示する URL の `#token=<秘密>`（フラグメントは、サーバーに送られない）。読んだら、
//!   `sessionStorage` に移し、アドレスバーから消す。

use std::sync::Mutex;

/// 起動時に決めた秘密（実行中、変わらない）
static TOKEN: Mutex<Option<String>> = Mutex::new(None);

/// WebSocket のサブプロトコルに秘密を載せるときの接頭辞（`backend` と同じ）
pub const WS_PROTOCOL_PREFIX: &str = "yui.token.";

pub fn set_token(token: Option<String>) {
    if let Ok(mut t) = TOKEN.lock() {
        *t = token.filter(|t| !t.trim().is_empty());
    }
}

pub fn token() -> Option<String> {
    TOKEN.lock().ok().and_then(|t| t.clone())
}

/// `Authorization` ヘッダーの値
pub fn bearer() -> Option<String> {
    token().map(|t| format!("Bearer {t}"))
}

/// WebSocket のサブプロトコル（ブラウザ用）
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn ws_protocol() -> Option<String> {
    token().map(|t| format!("{WS_PROTOCOL_PREFIX}{t}"))
}

/// `backend` の接続ファイル（JSON: `{"url": ..., "token": ...}`）から、秘密を取り出す
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn token_from_connection_file(text: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    v.get("token")?
        .as_str()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
}

/// コマンドライン（`--token <秘密>`）、環境変数、接続ファイルの順に、秘密を決める
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn pick_token(
    args: &[String],
    env_token: Option<String>,
    connection_file: Option<String>,
) -> Option<String> {
    let from_args = args
        .iter()
        .position(|a| a == "--token")
        .and_then(|i| args.get(i + 1))
        .cloned();
    from_args
        .or(env_token)
        .or_else(|| {
            connection_file
                .as_deref()
                .and_then(token_from_connection_file)
        })
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// URL のフラグメント（`#token=<秘密>&...`）から、秘密を取り出す
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn token_from_fragment(fragment: &str) -> Option<String> {
    fragment
        .trim_start_matches('#')
        .split('&')
        .find_map(|kv| kv.strip_prefix("token="))
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
}

/// 起動時に 1 回、秘密を決める（デスクトップ）
#[cfg(not(target_arch = "wasm32"))]
pub fn init() {
    let args: Vec<String> = std::env::args().collect();
    let file = dirs::config_dir()
        .map(|d| d.join("yui").join("backend.json"))
        .and_then(|p| std::fs::read_to_string(p).ok());
    set_token(pick_token(&args, std::env::var("YUI_TOKEN").ok(), file));
}

/// 起動時に 1 回、秘密を決める（ブラウザ）。`#token=` を読んで `sessionStorage` に移し、アドレスバーから消す。
/// 次に開いたとき（再読み込みなど）は、`sessionStorage` から読む。
#[cfg(target_arch = "wasm32")]
pub fn init() {
    const KEY: &str = "yui.token";
    let Some(window) = web_sys::window() else {
        return;
    };
    let storage = window.session_storage().ok().flatten();
    let hash = window.location().hash().unwrap_or_default();
    let mut token = token_from_fragment(&hash);
    if token.is_some() {
        if let Some(s) = &storage {
            let _ = s.set_item(KEY, token.as_deref().unwrap_or_default());
        }
        // 秘密を、アドレスバー（履歴、ブックマーク、画面の共有）から消す
        if let Ok(history) = window.history() {
            let path = window.location().pathname().unwrap_or_default();
            let search = window.location().search().unwrap_or_default();
            let _ = history.replace_state_with_url(
                &web_sys::wasm_bindgen::JsValue::NULL,
                "",
                Some(&format!("{path}{search}")),
            );
        }
    } else if let Some(s) = &storage {
        token = s.get_item(KEY).ok().flatten();
    }
    set_token(token);
}

/// 秘密つきの GET（`reqwest::get` の代わり）
pub async fn get(url: &str) -> reqwest::Result<reqwest::Response> {
    let mut request = reqwest::Client::new().get(url);
    if let Some(auth) = bearer() {
        request = request.header(reqwest::header::AUTHORIZATION, auth);
    }
    request.send().await
}

/// 秘密つきの要求（招待の API など、GET 以外にも使う）。応答の本文を、文字列で返す。
/// 成功（2xx）なら `Ok(本文)`、そうでなければ `Err("<状態> <本文>")`。
pub async fn send_text(
    method: reqwest::Method,
    url: &str,
    json_body: Option<String>,
) -> Result<String, String> {
    let mut request = reqwest::Client::new().request(method, url);
    if let Some(auth) = bearer() {
        request = request.header(reqwest::header::AUTHORIZATION, auth);
    }
    if let Some(body) = json_body {
        request = request
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body);
    }
    let response = request
        .send()
        .await
        .map_err(|e| format!("cannot reach the backend: {e}"))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if status.is_success() {
        Ok(text)
    } else {
        Err(describe_failure(status, &text))
    }
}

/// 失敗の応答の文。`backend` の本文が「Forbidden: ...」のように理由で始まっていると、
/// 状態（`403 Forbidden`）と重なるので、その頭は落とす
fn describe_failure(status: reqwest::StatusCode, body: &str) -> String {
    let body = body.trim();
    let reason = status.canonical_reason().unwrap_or("");
    let rest = body
        .strip_prefix(reason)
        .and_then(|r| r.strip_prefix(':'))
        .filter(|_| !reason.is_empty())
        .map_or(body, str::trim);
    if rest.is_empty() {
        status.to_string()
    } else {
        format!("{status}: {rest}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_does_not_repeat_the_reason() {
        use reqwest::StatusCode;
        assert_eq!(
            describe_failure(
                StatusCode::FORBIDDEN,
                "Forbidden: this action requires the Host role\n"
            ),
            "403 Forbidden: this action requires the Host role"
        );
        assert_eq!(
            describe_failure(StatusCode::TOO_MANY_REQUESTS, "too many failed attempts"),
            "429 Too Many Requests: too many failed attempts"
        );
        assert_eq!(
            describe_failure(StatusCode::UNAUTHORIZED, ""),
            "401 Unauthorized"
        );
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_token_comes_from_args_then_env_then_the_connection_file() {
        let file = Some(r#"{"url":"http://127.0.0.1:8080","token":"from-file"}"#.to_string());
        assert_eq!(
            pick_token(
                &args(&["yui", "--token", "from-arg"]),
                Some("from-env".into()),
                file.clone()
            ),
            Some("from-arg".into())
        );
        assert_eq!(
            pick_token(&args(&["yui"]), Some("from-env".into()), file.clone()),
            Some("from-env".into())
        );
        assert_eq!(
            pick_token(&args(&["yui"]), None, file),
            Some("from-file".into())
        );
        assert_eq!(pick_token(&args(&["yui"]), None, None), None);
        // 壊れたファイル・空の値は、秘密なし
        assert_eq!(
            pick_token(&args(&["yui"]), None, Some("not json".into())),
            None
        );
        assert_eq!(pick_token(&args(&["yui"]), Some("  ".into()), None), None);
        assert_eq!(pick_token(&args(&["yui", "--token"]), None, None), None);
    }

    #[test]
    fn test_token_from_the_url_fragment() {
        assert_eq!(token_from_fragment("#token=abc123"), Some("abc123".into()));
        assert_eq!(
            token_from_fragment("#a=1&token=abc123&b=2"),
            Some("abc123".into())
        );
        assert_eq!(token_from_fragment("#token="), None);
        assert_eq!(token_from_fragment(""), None);
        assert_eq!(token_from_fragment("#other=1"), None);
    }

    #[test]
    fn test_headers_use_the_token() {
        set_token(None);
        assert_eq!(bearer(), None);
        assert_eq!(ws_protocol(), None);
        set_token(Some("sekret-0123456789".into()));
        assert_eq!(bearer().as_deref(), Some("Bearer sekret-0123456789"));
        assert_eq!(
            ws_protocol().as_deref(),
            Some("yui.token.sekret-0123456789")
        );
        set_token(Some("  ".into()));
        assert_eq!(token(), None);
        set_token(None);
    }
}
