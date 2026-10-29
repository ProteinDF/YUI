# YUI のプラグインのプロトコル

> [!NOTE]
> [英語版 (English canonical version)](plugin-protocol.md) が正本です。

この文書は、Python 以外の言語でプラグインを書く人と、YUI が同梱するランナーを使わずに Python でプラグインを書く人のためのものです。Python のパッケージとして書くプラグイン（`register(app)`。[plugin-authoring.ja.md](plugin-authoring.ja.md) を参照）には要りません。設計は [architecture.ja.md](../../architecture.ja.md) の 6.4 節にあります。

> [!WARNING]
> プラグインは、利用者が実行するどのプログラムとも同じ権限で動きます。サンドボックスはありません。信頼できるプラグインだけを有効にしてください。

## 概要

YUI は、あなたのプログラムを子プロセスとして起動し、そのプロセスの標準入力と標準出力でやり取りします。メッセージは、すべて [msgpack](https://msgpack.org) の連想配列です。必要なのは、その言語の msgpack のライブラリ（たいていの言語にあります）だけです。

方式は 2 つあり、`plugin.yaml` の `mode` で選びます。

- `resident`（既定）: YUI の起動とともにプログラムが起動し、動き続けます。コマンドとメニュー項目はメッセージで宣言し、YUI が送るコマンドに答えます。この文書は、まずこちらを説明します。
- `oneshot`: コマンドを呼ぶたびにプログラムを起動し、2 つのファイルでやり取りします。「oneshot の方式」を参照してください。

## プロセスの起動

`plugin.yaml` の `command` に、プログラムと引数をリストで書きます（シェルは通しません）。

```yaml
name: raw-python
version: 0.1.0
api_version: 1
command: [python3, plugin.py]
```

- 作業フォルダーは、プラグインのフォルダーです。ディレクトリを含まないプログラム名（`python3`）は `PATH` から探し、ディレクトリを含む相対パス（`bin/run`）は、プラグインのフォルダーからのパスとして扱います。
- 標準出力は、プロトコル専用です。ほかのものを出力しないでください。
- 標準エラーに書いたものは、プラグインのフォルダーの `plugin.log` に、1 行ずつ `<name>: <行>` の形で追記されます。デバッグの出力に使えます。登録の前にプログラムが終わったときは、最後の行が失敗の理由として表示されます。
- 起動から 5 秒以内に、`hello` と `register` をこの順に送らなければなりません（下を参照）。間に合わなければ、そのプラグインは失敗になります。

## メッセージの形式

メッセージは、長さ（4 バイト、big-endian。あとに続く msgpack のデータの大きさ）と、それに続く 1 つの msgpack の連想配列です。1 つのメッセージは 16 MiB までです。大きなデータは、ファイルのパスで渡してください。

どの連想配列も、それぞれの項目のほかに、次の 2 つの鍵を持ちます。

- `v`: プロトコルの版。整数で、今は `1`。ほかの値は、YUI が受け付けません。
- `type`: メッセージの種類。`hello`、`register`、`invoke`、`result`、`log` のどれか。

知らない鍵は無視されます。知らない `type`、足りない項目、違う版、おかしい長さは、プロトコルの誤りです。YUI は `plugin.log` に書き、そのプラグインからの読み取りをやめます。そのプラグインは、まだ登録していなければすぐに、登録していれば次のコマンドで、失敗になります。

### hello（プラグインから YUI へ）

最初に送るメッセージ。項目は、`name`（文字列。`plugin.yaml` の `name`、つまりフォルダーの名前と同じでなければならない）、`version`（文字列）、`api_version`（整数。アプリのプラグイン API の版、今は `1`、と同じでなければならない）。

### register（プラグインから YUI へ）

`hello` の後に、1 回だけ送ります。項目は次のとおりです。

- `commands`: コマンドの識別子（文字列）のリスト。どの識別子も `plugin.<name>.` で始まらなければなりません。
- `menu_items`: `location`、`order`、`label`、`command`、`args`（省略できる。バイナリを含まない任意の msgpack の値。連想配列の鍵は文字列）を持つ連想配列のリスト。`location` は `menu/file`、`menu/view`、`menu/help`、`context/viewport`、`context/structure_tree`、`context/representation`、`context/file` のどれか。`order` は 32 ビットに収まる整数で、項目はこの順（小さい順）に並ぶ（組み込みの項目は 100 刻み）。`command` は、`commands` のどれかでなければならない。`args` は、`invoke` で返される。

どこかが誤っていれば、そのプラグインは失敗になり、項目は何も出ません。メニュー項目とコマンドは、あとから変えられません。

### invoke（YUI からプラグインへ）

利用者が、あなたのメニュー項目を選んだときに送られます。項目は、`id`（返すための整数）、`command`、`target`、`args`。`target` は右クリックした対象で、メニューバーの項目のときは nil です。`kind` を持つ連想配列です。

| `kind` | ほかの鍵 |
|--------|----------|
| `atom` | `atom_id`（原子の番号） |
| `tree_node` | `key`（構造ツリーのノードの鍵） |
| `representation` | `index`（Representation の一覧の行の番号） |
| `file` | `path`（プロジェクトツリーのファイル） |

将来の版で、鍵が増えることがあります。YUI は、前の `invoke` に答える前に、次の `invoke` を送ることがあります。それぞれの `id` で答えてください。順番は問いません。

### result（プラグインから YUI へ）

`invoke` への答え。項目は、`id`（`invoke` と同じ）、`ok`（真か偽）、`error`（`ok` が偽のときは文字列、そうでなければ nil）、`result`（バイナリを含まない任意の msgpack の値、または nil。YUI は今は使わない）。`ok` が偽のときは、理由がステータスバーに出て、そのプラグインは、そのセッションの間は無効になります（例外を出した Python のプラグインと同じです）。

### log（プラグインから YUI へ）

`plugin.log` に書く 1 行。項目は `text`（文字列）。`<name>: <text>` の形で書かれます。

## 終了

YUI は終わるとき、プログラムの標準入力を閉じます。入力の終わり（データを返さない読み取り）を読んだら、プログラムを終えてください。YUI は少し待ってから、まだ動いているプロセスを止めます。プラグインが失敗したときも、YUI はプロセスを止めます。

## oneshot の方式

`mode: oneshot` では、`hello` も `register` も、動き続けるプロセスもありません。メニュー項目は `plugin.yaml`（`menu_items`。項目のないコマンドは `commands`）に書き、`command` には `{input}` と `{output}` を含めます。

```yaml
name: say-once
version: 0.1.0
api_version: 1
mode: oneshot
command: [./run, "{input}", "{output}"]
menu_items:
  - {location: menu/view, order: 500, label: "Say once", command: plugin.say-once.say, args: {greeting: hi}}
```

呼ぶたびに、YUI は新しい一時フォルダーを作り、そこに入力ファイルを書き、引数の `{input}` と `{output}` を 2 つのパスに置き換えてプログラムを実行し、出力ファイルを読んで、フォルダーを消します。どちらのファイルも、長さを前に付けない、1 つの msgpack の連想配列です。

- 入力: `v`、`command`、`target`、`args`（`invoke` と同じ）。
- 出力: `v`、`ok`、`error`、`result`（`result` と同じ）。

`ok` が偽のとき、またはプログラムが 0 以外で終わったときは、その呼び出しが失敗です（理由は、出力ファイルの `error`、なければ標準エラーの最後の行）。終了コードが 0 で出力ファイルがなければ、成功です。プログラムが標準出力と標準エラーに書いたものは、`plugin.log` に書かれます。1 回の失敗でプラグインは無効になりません。呼び出しに時間制限はなく、動いているプロセスは YUI を終えるときに止めます。

## 例

`examples/protocol/raw-python/plugin.py` は、プロトコルを直接話す、完全な常駐のプラグインです（`msgpack` のパッケージが要ります）。中心の部分は次のとおりです。

```python
def send(message):
    message["v"] = 1
    body = msgpack.packb(message, use_bin_type=True)
    stdout.write(struct.pack(">I", len(body)) + body)
    stdout.flush()

def receive():
    head = stdin.read(4)
    if len(head) < 4:
        return None          # YUI が標準入力を閉じた
    (size,) = struct.unpack(">I", head)
    return msgpack.unpackb(stdin.read(size), raw=False)

send({"type": "hello", "name": NAME, "version": "0.1.0", "api_version": 1})
send({"type": "register", "commands": [...], "menu_items": [...]})
while (message := receive()) is not None:
    if message.get("type") == "invoke":
        send({"type": "result", "id": message["id"], "ok": True, "error": None, "result": None})
```

フォルダー `examples/protocol/raw-python/` を `<config>/yui/plugins/raw-python/` にコピーし、有効にして、YUI を再起動してから、View → Echo (raw protocol) を選びます。`plugin.log` に 1 行が出ます。

## 互換性

プロトコルの版（`v`）と、プラグイン API の版（`api_version`）は別です。`v` が変わるのは、既存のプラグインを壊す形でメッセージの形式を変えるときだけです。メッセージに鍵を足すことや、`target` に新しい鍵が増えることでは変わりません。知らない鍵は、無視してください。
