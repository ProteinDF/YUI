//! ビルドのときに、git のコミット（短いハッシュ）と、作業ツリーに変更があるかを埋め込む。
//! Help → About で表示する（`YUI_GIT_HASH` と `YUI_GIT_DIRTY`）。git がない・管理外のときは `unknown`。

use std::process::Command;

/// `git -C <dir> <args>` の出力（前後の空白を除く）。失敗したら `None`。
fn git(dir: &str, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// `git rev-parse --git-path` の結果を、`dir` を基準にした絶対パスにする（worktree では `.git` がファイルになる）。
fn git_path(dir: &str, name: &str) -> Option<String> {
    let path = git(dir, &["rev-parse", "--git-path", name])?;
    if path.starts_with('/') {
        Some(path)
    } else {
        Some(format!("{dir}/{path}"))
    }
}

fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());

    // コミットや、ブランチの移動のあとに、この build script を読み直す
    if let Some(head) = git_path(&dir, "HEAD") {
        println!("cargo:rerun-if-changed={head}");
    }
    if let Some(path) = git(&dir, &["symbolic-ref", "-q", "HEAD"]).and_then(|r| git_path(&dir, &r))
    {
        println!("cargo:rerun-if-changed={path}");
    }
    if let Some(packed) = git_path(&dir, "packed-refs") {
        println!("cargo:rerun-if-changed={packed}");
    }

    let hash =
        git(&dir, &["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
    let dirty = git(&dir, &["status", "--porcelain", "--untracked-files=no"])
        .is_some_and(|changes| !changes.is_empty());

    println!("cargo:rustc-env=YUI_GIT_HASH={hash}");
    println!(
        "cargo:rustc-env=YUI_GIT_DIRTY={}",
        if dirty { "1" } else { "0" }
    );
}
