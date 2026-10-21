# YUI への貢献

> [!NOTE]
> [英語版 (English canonical version)](CONTRIBUTING.md) が正本です。

YUI の改善に協力いただき、ありがとうございます。このページでは、準備、変更の作り方、pull request の送り方を説明します。

## 始める前に

- `README.ja.md`（YUI とは何か、ビルドと起動の方法）を読み、`architecture.ja.md`（設計。これが正本）に目を通してください。
- `ROADMAP.ja.md` に、予定していることと、しないことがあります。
- Python で機能を足すためのプラグインの仕組みを設計中です。それができるまでは、機能は Rust のコードに足します。

## 準備

1. [rustup](https://rustup.rs/) で Rust（stable）を入れます。
2. ブラウザ版と、すべての検査のために、次も実行します。
   ```sh
   rustup target add wasm32-unknown-unknown
   cargo install trunk --locked
   ```
3. コードを取得して、デスクトップ版を起動します。
   ```sh
   git clone https://github.com/ProteinDF/YUI.git
   cd YUI
   git checkout develop
   cargo run -p yui-frontend --release
   ```

## 変更の作り方

1. `develop` からブランチを作ります（git-flow）: `git checkout -b feature/<短い名前> develop`。`main` や `develop` に直接コミットしないでください。書き込みの権限がなければ、リポジトリを fork して、fork の中でブランチを作ります。
2. 1 つの pull request は、1 つの話題にします。
3. 変更と一緒に、テストを足すか直します（`cargo test --workspace`）。
4. コードと一緒に、ドキュメントも直します。
   - 利用者から見える変更は、`CHANGELOG.md` と `CHANGELOG.ja.md`（`Unreleased`）に書きます。
   - 設計の決定は、`architecture.md` と `architecture.ja.md` に書きます。
   - 英語のドキュメントが正本です。日本語版（`*.ja.md`）も同じ内容に保ってください。
5. コミットメッセージは英語で、[Conventional Commits](https://www.conventionalcommits.org/) に従います: `<type>(<scope>): <summary>`。type は `feat`、`fix`、`perf`、`refactor`、`docs`、`test`、`build`、`ci`、`chore` のどれかです。

## 検査

pull request を作る前に、すべての検査を実行します。

```sh
scripts/check.sh --trunk
```

8 項目すべてが PASS する必要があります。GitHub でも、pull request ごとに同じ検査が動きます。

## pull request の作り方

- 宛先は `develop` にします。
- 何をなぜ変えたか、どう確かめたか（`scripts/check.sh --trunk` の結果の要約を貼る）、確かめられなかったことを書きます。
- メンテナーがレビューして、マージします。

## 問題の報告

<https://github.com/ProteinDF/YUI/issues> に issue を作り、再現の手順、期待したこと、起きたこと（OS、デスクトップ版かブラウザ版か）を書いてください。

## ライセンス

YUI のライセンスは GNU General Public License v3.0（`LICENSE`）です。貢献したものは、同じライセンスで提供されることに同意したものとします。
