# The YUI plugin protocol

> [!NOTE]
> [Japanese version (日本語版)](plugin-protocol.ja.md) is also available.

This page is for writing a plugin in a language other than Python, or a Python plugin that does not use the runner that YUI ships. A plugin that is a Python package (`register(app)`, see [plugin-authoring.md](plugin-authoring.md)) does not need it. The design is in [architecture.md](../../architecture.md) §6.4.

> [!WARNING]
> A plugin runs with the same rights as any program you run. It is not sandboxed. Enable only plugins you trust.

## Overview

YUI starts your program as a child process and talks to it over the standard input and the standard output of that process. Every message is a [msgpack](https://msgpack.org) map. You need a msgpack library for your language (most languages have one) and nothing else.

There are two modes, chosen by `mode` in `plugin.yaml`:

- `resident` (the default): your program starts when YUI starts and keeps running. It declares its commands and menu items by sending messages, and answers the commands YUI sends. This page describes this mode first.
- `oneshot`: your program is started anew for each command call and talks through two files. See "The oneshot mode".

## Starting the process

In `plugin.yaml`, give the program and its arguments as a list in `command` (no shell is used):

```yaml
name: raw-python
version: 0.1.0
api_version: 1
command: [python3, plugin.py]
```

- The working directory is the plugin folder. A program name without a directory (`python3`) is searched in the `PATH`; a relative path with a directory (`bin/run`) is taken from the plugin folder.
- The standard output is for the protocol only. Do not print anything else there.
- Whatever you write to the standard error is appended to `plugin.log` in the plugins folder, one line at a time, as `<name>: <line>`. Use it for debug output. When your program ends before it has registered, the last line is shown as the reason.
- Within five seconds of the start you must send `hello` and then `register` (see below). Otherwise the plugin fails.

## Message format

A message is its length in 4 bytes (big-endian, the size of the msgpack data that follows), then one msgpack map. A message may be at most 16 MiB; send big data as a file path instead.

Every map has two keys besides its own fields:

- `v`: the protocol version, an integer, currently `1`. YUI rejects any other value.
- `type`: the kind of message: `hello`, `register`, `invoke`, `result` or `log`.

Unknown extra keys are ignored. A message with an unknown `type`, a missing field, a wrong version or a bad length is a protocol error: YUI writes it to `plugin.log` and stops reading from the plugin. The plugin then fails: at once if it has not registered yet, otherwise at its next command.

### hello (plugin to YUI)

The first message. Fields: `name` (string; must equal `name` in `plugin.yaml`, which is the folder name), `version` (string) and `api_version` (integer; must equal the API version of the app, currently `1`).

### register (plugin to YUI)

Sent once, after `hello`. Fields:

- `commands`: a list of command ids (strings). Each id must start with `plugin.<name>.`.
- `menu_items`: a list of maps with `location`, `order`, `label`, `command` and `args` (optional; any msgpack value without binary data, with string keys in maps). `location` is one of `menu/file`, `menu/view`, `menu/help`, `context/viewport`, `context/structure_tree`, `context/representation`, `context/file`. `order` is an integer that fits in 32 bits; items are sorted by it (smaller first; built-in items use multiples of 100). `command` must be one of your `commands`. `args` is given back to you in `invoke`.

If anything is wrong, the plugin fails and none of its items appear. The menu items and commands cannot change afterwards.

### invoke (YUI to plugin)

Sent when the user chooses one of your menu items. Fields: `id` (an integer that you give back), `command`, `target` and `args`. `target` is what was right-clicked, or nil when the item is in a menu bar. It is a map with a `kind`:

| `kind` | Other keys |
|--------|------------|
| `atom` | `atom_id` (the atom number) |
| `tree_node` | `key` (the node key in the structure tree) |
| `representation` | `index` (the row index in the representation list) |
| `file` | `path` (the file in the project tree) |

Later versions may add more keys. YUI may send a new `invoke` before you have answered the last one; answer each by its `id`, in any order.

### result (plugin to YUI)

The answer to an `invoke`. Fields: `id` (the same as in the `invoke`), `ok` (true or false), `error` (a string when `ok` is false, otherwise nil) and `result` (any msgpack value that has no binary data, or nil; YUI does not use it yet). When `ok` is false, the reason is shown in the status bar and the plugin is disabled for the rest of the session, as for a Python plugin that raises.

### log (plugin to YUI)

A line for `plugin.log`. Field: `text` (a string). It is written as `<name>: <text>`.

## Stopping

When YUI quits, it closes the standard input of your program. End your program when you read the end of the input (a read that returns no data). YUI waits a short time and then stops the process if it is still running. YUI also stops the process when the plugin fails.

## The oneshot mode

With `mode: oneshot` there is no `hello`, `register` or long-running process. The menu items are written in `plugin.yaml` (`menu_items`, and `commands` for commands that have no item), and `command` must contain `{input}` and `{output}`:

```yaml
name: say-once
version: 0.1.0
api_version: 1
mode: oneshot
command: [./run, "{input}", "{output}"]
menu_items:
  - {location: menu/view, order: 500, label: "Say once", command: plugin.say-once.say, args: {greeting: hi}}
```

For each call YUI creates a fresh temporary folder, writes the input file there, replaces `{input}` and `{output}` in the arguments by the two paths, runs the program, reads the output file and deletes the folder. Both files are one msgpack map, with no length in front:

- Input: `v`, `command`, `target`, `args` (the same as in `invoke`).
- Output: `v`, `ok`, `error`, `result` (the same as in `result`).

A call fails when `ok` is false, or when the program exits with a non-zero status (the reason is then the `error` of the output file, or the last line of the standard error). A program that exits with status 0 and writes no output file succeeds. What the program writes to the standard output or the standard error goes to `plugin.log`. One failed call does not disable the plugin. A call has no time limit; the processes still running are stopped when YUI quits.

## Example

`examples/protocol/raw-python/plugin.py` is a complete resident plugin that speaks the protocol directly (it needs the `msgpack` package). The core of it:

```python
def send(message):
    message["v"] = 1
    body = msgpack.packb(message, use_bin_type=True)
    stdout.write(struct.pack(">I", len(body)) + body)
    stdout.flush()

def receive():
    head = stdin.read(4)
    if len(head) < 4:
        return None          # YUI closed the standard input
    (size,) = struct.unpack(">I", head)
    return msgpack.unpackb(stdin.read(size), raw=False)

send({"type": "hello", "name": NAME, "version": "0.1.0", "api_version": 1})
send({"type": "register", "commands": [...], "menu_items": [...]})
while (message := receive()) is not None:
    if message.get("type") == "invoke":
        send({"type": "result", "id": message["id"], "ok": True, "error": None, "result": None})
```

Copy the folder `examples/protocol/raw-python/` to `<config>/yui/plugins/raw-python/`, enable it, restart YUI, and choose View → Echo (raw protocol). A line appears in `plugin.log`.

## Compatibility

The protocol version (`v`) and the plugin API version (`api_version`) are separate. `v` changes only when the message format changes in a way that breaks existing plugins. Adding a key to a message, or a new key in `target`, does not change it, so ignore keys that you do not know.
