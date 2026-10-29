# YUI プラグインの書き方

> [!NOTE]
> [英語版 (English canonical version)](plugin-authoring.md) が正本です。

YUI のプラグインは、ネイティブのデスクトップ版に、コマンドとメニューの項目を追加します。多くは Python のパッケージで、下で説明する公開の `app` オブジェクトを使って書きます。設計は [architecture.ja.md](../../architecture.ja.md) の 6.4 節にあります。

> [!WARNING]
> プラグインは、利用者が実行するどの Python のスクリプトとも同じ権限で動きます。サンドボックスはありません。信頼できるプラグインだけを有効にしてください。

## プラグインの動き方

YUI は、有効なプラグインを、1 つずつ別のプロセスとして起動し、そのプロセスの標準入出力でやり取りします。プラグインが落ちても、止まっても、YUI は止まりません。

- `plugin.yaml` に `command` がなく、`__init__.py` があるプラグインは、Python のパッケージです。YUI は、同梱のランナーで動かします。使う Python は、`settings.yaml` の `plugins.python`（パス、またはコマンド名）で、設定がなければ `PATH` の `python3`（Windows では `python`）です。Python 3.8 以降が必要です。
- ほかに入れるものはありません。ランナーは小さな msgpack を自前で持ち、`msgpack` のパッケージが入っていればそれを使います。選んだ Python に入っているパッケージ（たとえば `numpy`）は、プラグインから import できます。
- プラグインの `print()` など、標準出力や標準エラーへの書き出しは、`plugin.log` に書かれます。
- `plugin.yaml` に `command` があるプラグインは、そのまま実行します。どの言語で書いてもかまいません。守るべきプロトコルは [architecture.ja.md](../../architecture.ja.md) の 6.4 節にあります。そのための手引きは、まだ書いていません。

```yaml
plugins:
  enabled: [hello-yui]
  python: /usr/bin/python3      # 省略できる。省くと PATH の python3 を使う
```

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
| `command` | いいえ | 起動するプログラムと引数のリスト（シェルは通さない）。Python のパッケージでは省く。 |
| `mode` | いいえ | `resident`（既定）: YUI が動いている間、動き続ける 1 つのプロセス。`oneshot`: コマンドを呼ぶたびに新しいプロセス（「oneshot の方式」を参照）。 |
| `menu_items` | oneshot のみ | メニュー項目の `{location, order, label, command, args}` のリスト（`args` は省ける）。 |
| `commands` | いいえ | oneshot のみ: メニュー項目のないコマンドの識別子。 |

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

プラグインのフォルダーの `plugin.log` に、`<name>: <message>` を追記します。

### `app.add_command(id, func)`

コマンドを登録します。`id` は `plugin.<name>.` で始まらなければならないため、組み込みのコマンドや他のプラグインのコマンドを置き換えられません。コマンドを実行すると、`func(ctx, args)` が、そのプラグインのプロセスの中で呼ばれます。`func` は呼び出せるものでなければなりません。

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

## oneshot の方式

`mode: oneshot` では、YUI はコマンドを呼ぶたびに新しいプロセスを起動し、動かし続けません。めったに実行しないコマンドや、メモリを持ち続けたくないコマンドに向きます。`register` がないので、メニュー項目は `plugin.yaml` に書きます。

```yaml
name: say-once
version: 0.1.0
api_version: 1
mode: oneshot
menu_items:
  - {location: menu/view, order: 500, label: "Say once", command: plugin.say-once.say, args: {greeting: hi}}
```

Python のパッケージは、これまでどおり `register(app)` と `func(ctx, args)` を書きます。`register` は新しいプロセスの中で、コマンドを探すためにだけ呼ばれ、`app.add_menu_item` は無視され、`app.log` は標準エラーに書かれます。Python 以外のプログラムは、`{input}` と `{output}` を含む `command` を書きます（例: `command: [./run, "{input}", "{output}"]`）。YUI は入力ファイル（msgpack の連想配列 1 つ: `v`、`command`、`target`、`args`）を書き、2 つのパスを渡してコマンドを実行し、出力ファイル（msgpack の連想配列 1 つ: `v`、`ok`、`error`、`result`）を読みます。`ok` が偽のとき、またはプログラムが 0 以外で終わったときは、その呼び出しが失敗です。1 回の失敗でプラグインは外されず、理由はステータスバーに出ます。プログラムが標準出力や標準エラーに書いたものは、`plugin.log` に書かれます。呼び出しに時間制限はなく、動いているプロセスは YUI を終えるときに止めます。

## 失敗と plugin.log

読み込みと失敗は、すべてプラグインのフォルダーの `plugin.log` に書かれます。理由は、ステータスバーと一覧にも出ます。失敗で YUI が止まることはありません。

- `plugin.yaml` が合わない、または対応しない `api_version` のときは、読み込まれません。
- `register` で例外が起きたときは、読み込まれません。そのプラグインが足したものは何も残りません。
- コマンドで例外が起きると、そのプラグインは、そのセッションの間は無効になります。項目は消え、コマンドは実行されません。
- プラグインのプロセスが起動できない（たとえば Python が見つからない。`plugins.python` を設定してください）、または登録の前に終わったときは、読み込まれません。理由は、標準エラーに書かれた最後の行です。
- 起動のとき、`register` は 5 秒以内に終わらなければなりません。終わらなければ、そのプラグインは失敗になります。
- コマンドには時間制限がありません。戻らないコマンドは、YUI を終えるまで、そのプラグインを占有します。ほかのプラグインには影響しません。
- YUI を終えるとき、すべてのプラグインのプロセスを止めます。

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
- Python 以外の言語でプラグインを書くための手引き。
- メニューの項目の、Python で計算する有効・チェックの状態。
- プラグインからの、アプリのデータ（構造、選択）の参照。読み取り専用の窓口は計画していますが、まだ定義していません。
