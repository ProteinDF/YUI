"""A plugin that speaks the YUI plugin protocol directly, without the runner that YUI ships.

It needs the `msgpack` package (pip install msgpack). See docs/plugins/plugin-protocol.md.
To try it, copy this folder to <config>/yui/plugins/raw-python/ and enable "raw-python".
"""

import struct
import sys

import msgpack

NAME = "raw-python"
stdin, stdout = sys.stdin.buffer, sys.stdout.buffer


def send(message):
    """One message: its length (4 bytes, big-endian), then a msgpack map that has "v" (the protocol version)."""
    message["v"] = 1
    body = msgpack.packb(message, use_bin_type=True)
    stdout.write(struct.pack(">I", len(body)) + body)
    stdout.flush()


def receive():
    """The next message, or None when YUI has closed the standard input (time to end)."""
    head = stdin.read(4)
    if len(head) < 4:
        return None
    (size,) = struct.unpack(">I", head)
    return msgpack.unpackb(stdin.read(size), raw=False)


# Say who you are, then declare the commands and the menu items. Do both within five seconds.
send({"type": "hello", "name": NAME, "version": "0.1.0", "api_version": 1})
send({
    "type": "register",
    "commands": [f"plugin.{NAME}.echo"],
    "menu_items": [{
        "location": "menu/view",
        "order": 700,
        "label": "Echo (raw protocol)",
        "command": f"plugin.{NAME}.echo",
        "args": {"greeting": "hi"},
    }],
})

# Answer every "invoke" with a "result" that has the same id, until YUI closes the standard input.
while True:
    message = receive()
    if message is None:
        break
    if message.get("type") != "invoke":
        continue
    args = message.get("args")
    send({"type": "log", "text": f"echo {args!r} for {message.get('target')!r}"})
    send({"type": "result", "id": message["id"], "ok": True, "error": None, "result": args})
