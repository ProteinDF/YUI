# Instructions

AI コーディングエージェント（と、それを使う貢献者）向けの、このリポジトリの規則。貢献の手順は `CONTRIBUTING.md`（日本語版 `CONTRIBUTING.ja.md`）を見ること。

- 利用者との対話、計画、解説は日本語で行う。
- 設計上の決定事項は `architecture.md` を正とする。書かれていない設計を決めるときは、独断で決めず、`architecture.md` への追記を提案する。
- `README.md` / `architecture.md` / `ROADMAP.md` / `TESTING.md` / `CHANGELOG.md` / `CONTRIBUTING.md` は英語を正本とし、日本語版（`*.ja.md`）を同時に更新する（見出しの数は `scripts/check-doc-sync.sh` で確かめる）。`AGENTS.md`（本ファイル）は日本語のみ。
- 利用者から見える変更は、`CHANGELOG.md` / `CHANGELOG.ja.md` の `Unreleased` に書く。
- コミットメッセージは英語で、Conventional Commits（`<type>(<scope>): <summary>`）に従う。
- ファイル名を参照するときは、相対パスを使う（例: `../ProteinDF_bridge`、`core/src/state.rs`）。絶対パスや `file:///` 形式のリンクは使わない。
- バージョンは `年.月.リリース番号`（例: `2026.10.11`）。ProteinDF / QCLObot / ProteinDF_bridge と同じ方式。
- ブランチ運用は git-flow（`ROADMAP.md` の 1 章「Ground Rules」）。作業は `develop` から作った `feature/*` ブランチで行い、PR は `develop` 宛てにする。`main` へ直接 push しない。
- PR を作る前に `scripts/check.sh --trunk` を実行し、8 項目すべてが PASS することを確かめ、その結果を PR に書く。確かめられなかったことは、そのまま書く。
- 秘密の値（`backend` の接続用の秘密など）を、URL・ログ・コミットに入れない。`backend` を不用意にネットワークへ公開しない（`architecture.md` 7.4 節）。
