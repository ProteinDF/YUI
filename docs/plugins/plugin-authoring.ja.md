# YUI プラグインの書き方

> [!NOTE]
> [英語版 (English canonical version)](plugin-authoring.md) が正本です。

YUI のプラグインは、ネイティブのデスクトップ版に、コマンドとメニューの項目を追加する Python のパッケージです。プラグインは、下で説明する公開の `app` オブジェクトを使って書きます。設計は [architecture.ja.md](../../architecture.ja.md) の 6.4 節にあります。

> [!WARNING]
> プラグインは、利用者が実行するどの Python のスクリプトとも同じ権限で動きます。サンドボックスはありません。信頼できるプラグインだけを有効にしてください。

## フォルダーの構成

プラグインは、利用者の設定フォルダーの中のプラグインのフォルダーに、1 つのフォルダーとして置きます。

```text
<config>/yui/plugins/<name>/
    plugin.yaml
    __init__.py
    （パッケージのほかのモジュール）
```

`<config>` は、利用者ごとの設定フォルダーです（Linux は `~/.config`、macOS は `~/Library/Application Support`、Windows は `%APPDATA%`）。`examples/plugins/hello-yui/` のサンプルが、最も小さい動くプラグインです。

## plugin.yaml

| 項目 | 必須 | 意味 |
|------|------|------|
| `name` | はい | フォルダーの名前と同じでなければならない。英字・数字・`_`・`-` だけ。英字で始まり、64 字まで。プラグインの名前空間にもなる。 |
| `version` | はい | 版の文字列（例: `0.1.0`）。 |
| `api_version` | はい | 整数。アプリのプラグイン API の版（今は `1`）と同じでなければならない。 |
| `description` | いいえ | 一覧に出す 1 行の説明。 |
| `author` | いいえ | 一覧に出す作者。 |

`plugin.yaml` が合わないプラグインは読み込まれません。理由は、一覧と `plugin.log` に出ます。

## プラグインを有効にする

有効にするまで、プラグインは動きません。View → Plugins... を開くか、`settings.yaml` に名前を書きます。

```yaml
plugins:
  enabled: [hello-yui]
```

変更は、次に YUI を起動したときから効きます。

## app オブジェクト

`register(app)` は、起動のときに 1 回呼ばれます。`app` オブジェクトには、次のメソッドがあります。

### `app.log(message)`

プラグインのフォルダーの `plugin.log` に、`[<name>] <message>` を追記します。

### `app.add_command(id, func)`

コマンドを登録します。`id` は `plugin.<name>.` で始まらなければならないため、組み込みのコマンドや他のプラグインのコマンドを置き換えられません。コマンドを実行すると、`func(ctx, args)` が Python のスレッドで呼ばれます。`func` は呼び出せるものでなければなりません。

### `app.add_menu_item(location, order, label, command, args=None)`

`add_command` で登録したコマンドを実行するメニューの項目を足します。

- `location`: `menu/file`、`menu/view`、`menu/help`、`context/viewport`（3D 表示の右クリック）、`context/structure_tree`、`context/representation`、`context/file` のどれか。
- `order`: 整数。項目は `order` の小さい順に並ぶ。組み込みの項目は 100 刻みで、たとえば `150` にすると、組み込みの 2 つの項目の間に入る。
- `label`: 項目に出す文字。
- `command`: そのプラグインが `add_command` で追加したコマンドの `id`。
- `args`: 省略できる。JSON で書ける値（辞書、リスト、文字列、数、真偽値、`None`）。コマンドには `args` として渡される。

プラグインの項目は、常に押せて、チェックは付かない。

## ctx 引数

`func(ctx, args)` は、2 つの引数を受け取ります。

- `args`: `add_menu_item` に渡した `args`。なければ `{}`。
- `ctx.target`: 利用者が右クリックした対象。メニューバーの項目から呼ばれたときは `None`。

`ctx.target` は、`kind` という鍵を持つ辞書です。

| `kind` | ほかの鍵 |
|--------|----------|
| `atom` | `atom_id`（原子の番号） |
| `tree_node` | `key`（構造ツリーのノードの鍵） |
| `representation` | `index`（Representation の一覧の行の番号） |
| `file` | `path`（プロジェクトツリーのファイル） |

将来の版で、`ctx` に鍵が増えることがあります。ここに挙げた鍵だけがあると、決めつけないでください。

## 失敗と plugin.log

読み込みと失敗は、すべてプラグインのフォルダーの `plugin.log` に書かれます。理由は、ステータスバーと一覧にも出ます。失敗で YUI が止まることはありません。

- `plugin.yaml` が合わない、または対応しない `api_version` のときは、読み込まれません。
- `register` で例外が起きたときは、読み込まれません。そのプラグインが足したものは何も残りません。
- コマンドで例外が起きると、そのプラグインは、そのセッションの間は無効になります。項目は消え、コマンドは実行されません。
- 起動のとき、`register` は 5 秒以内に戻らなければなりません。戻らなければ、そのプラグインは失敗になります。
- コマンドには時間制限がありません。戻らないコマンドは、YUI を終えるまで Python のスレッドを占有します。

## api_version

`api_version` は、プラグイン API の版です。既存のプラグインを壊す変更（たとえば、関数の削除や意味の変更）のときだけ上がります。メソッドの追加、既定値のある引数の追加、`ctx` の鍵の追加では上がりません。`api_version` 1 向けに書いたプラグインは、アプリが 1 に対応しているかぎり動きます。

## 例

`examples/plugins/hello-yui/__init__.py`:

```python
def register(app):
    def say_hello(ctx, args):
        greeting = args.get("greeting", "hello")
        if ctx.target is None:
            app.log(f"{greeting} from the View menu")
        else:
            app.log(f"{greeting} about {ctx.target}")

    app.add_command("plugin.hello-yui.say", say_hello)
    app.add_menu_item(
        "menu/view", 500, "Say hello", "plugin.hello-yui.say", {"greeting": "hello"}
    )
```

このフォルダーを `<config>/yui/plugins/hello-yui/` にコピーし、有効にして、YUI を再起動してから、View → Say hello を選びます。`plugin.log` に 1 行が出ます。

## まだ使えないもの

- `pip` やエントリポイントによるプラグインの導入。今はフォルダーだけです。
- `numpy` などの外部パッケージ。埋め込みの Python がどこを探すかは、まだ決めていません。
- メニューの項目の、Python で計算する有効・チェックの状態。
- プラグインからの、アプリのデータ（構造、選択）の参照。読み取り専用の窓口は計画していますが、まだ定義していません。
