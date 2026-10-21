use serde::{Deserialize, Serialize};

/// リアルタイム・コラボレーションにおけるアクセス制御の権限モデル
/// (architecture.md - 5.1 権限モデル)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum UserRole {
    /// Host (Professor):
    /// AppStateの書き換え権限を100%保持。
    /// 他ユーザーへの編集権の一時付与、全Viewerへの視点(Camera)強制同期(Follow Me機能)のトリガー権を持つ。
    Host,

    /// Editor (Student):
    /// アノテーションの追加、特定の残基の操作権を持つ。
    Editor,

    /// Viewer:
    /// 読み取り専用。ホストのカメラ移動や構造操作のパケットをリアルタイムに受信し、
    /// 自身のローカルGPUでレンダリングする。
    #[default]
    Viewer,
}

/// 秘密（トークン）から `UserRole` を解決するリゾルバ抽象（architecture.md 7.1節、7.4節）
///
/// トークンは、意味のない（推測できない）文字列。起動時に `backend` が作る（`TokenStore::generate_host`）。
pub trait RoleResolver: Send + Sync {
    fn resolve(&self, token: &str) -> Option<UserRole>;
}

/// 秘密の最小の長さ（`YUI_TOKEN` などで外から与えるときの下限。推測されにくくするため）
pub const MIN_TOKEN_LEN: usize = 16;

/// 2 つの文字列を、一致するまでの時間が内容によらないように比べる（長さは、漏れてもよいものとして扱う）
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// 秘密と `UserRole` の対応表。照合は、すべての項目を、定数時間で比べる。
#[derive(Debug, Clone, Default)]
pub struct TokenStore {
    tokens: Vec<(String, UserRole)>,
}

impl TokenStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// 秘密を登録する（同じ秘密があれば、ロールを上書きする）。短すぎる秘密は、登録しない（`false`）。
    pub fn register(&mut self, token: impl Into<String>, role: UserRole) -> bool {
        let token = token.into();
        if token.len() < MIN_TOKEN_LEN {
            return false;
        }
        match self.tokens.iter_mut().find(|(t, _)| *t == token) {
            Some(entry) => entry.1 = role,
            None => self.tokens.push((token, role)),
        }
        true
    }
}

impl RoleResolver for TokenStore {
    fn resolve(&self, token: &str) -> Option<UserRole> {
        // 見つかっても、途中でやめない（登録の数が、時間に出ないように）
        let mut found = None;
        for (t, role) in &self.tokens {
            if constant_time_eq(t, token) {
                found = Some(*role);
            }
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_store_resolves_only_registered_secrets() {
        let mut store = TokenStore::new();
        assert!(store.register("host-secret-0123456789", UserRole::Host));
        assert!(store.register("viewer-secret-0123456789", UserRole::Viewer));
        assert_eq!(
            store.resolve("host-secret-0123456789"),
            Some(UserRole::Host)
        );
        assert_eq!(
            store.resolve("viewer-secret-0123456789"),
            Some(UserRole::Viewer)
        );
        // 以前の固定の番号・空・近い文字列は、解決されない
        for bad in [
            "",
            "1",
            "2",
            "3",
            "host-secret-012345678",
            "host-secret-01234567890",
        ] {
            assert_eq!(store.resolve(bad), None, "{bad:?}");
        }
        // 同じ秘密を登録し直すと、ロールが変わる
        assert!(store.register("viewer-secret-0123456789", UserRole::Editor));
        assert_eq!(
            store.resolve("viewer-secret-0123456789"),
            Some(UserRole::Editor)
        );
    }

    #[test]
    fn test_short_secrets_are_refused() {
        let mut store = TokenStore::new();
        assert!(!store.register("short", UserRole::Host));
        assert!(!store.register("a".repeat(MIN_TOKEN_LEN - 1), UserRole::Host));
        assert!(store.register("a".repeat(MIN_TOKEN_LEN), UserRole::Host));
    }

    #[test]
    fn test_constant_time_eq() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "abcd"));
    }
}
