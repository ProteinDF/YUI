"""YUI plugin runner (architecture.md section 6.4).

YUI starts this file as a child process, with the Python chosen for plugins:

    python -X utf8 -c <this file> <plugin folder> <plugin name> <plugin version> [--builtin-msgpack]
    python -X utf8 -c <this file> <plugin folder> <plugin name> <plugin version> --oneshot <input> <output>

It imports the plugin package (`<folder>/__init__.py`), calls `register(app)`, and then talks to YUI over its
standard input and output: every message is msgpack, preceded by its length in 4 bytes (big-endian), and carries
`v` (the protocol version) and `type` (`hello`, `register`, `invoke`, `result`, `log`).

The plugin author's API is the same as before: `register(app)`, `app.log(message)`,
`app.add_command(id, func)`, `app.add_menu_item(location, order, label, command, args=None)`, and
`func(ctx, args)` with `ctx.target`.

The `msgpack` package is used when it is installed. Otherwise a small built-in implementation is used, so a plugin
needs nothing to be installed. `--builtin-msgpack` always uses the built-in one (for tests).

With `--oneshot` (a plugin whose `mode` is `oneshot`) it handles one command and ends: it reads the input file
(one msgpack map: `v`, `command`, `target`, `args`), imports the package, calls `register(app)` to find the command,
runs it, and writes the output file (`v`, `ok`, `error`, `result`). `app.add_menu_item` is ignored then, because the
menu items of such a plugin are written in `plugin.yaml`, and `app.log` writes to the standard error.

In the default mode the real standard output is kept for the protocol only. `print()` and anything else the plugin writes to the
standard output goes to the standard error, which YUI writes to `plugin.log`.
"""

import importlib.util
import os
import signal
import struct
import sys
import traceback

PROTOCOL_VERSION = 1
API_VERSION = 1
MAX_MESSAGE_BYTES = 16 * 1024 * 1024


# ---------------------------------------------------------------------------
# A small msgpack (maps, arrays, str, bytes, integers, floats, booleans, nil).
# ---------------------------------------------------------------------------

def _pack_int(n, out):
    if 0 <= n < 128:
        out.append(struct.pack("B", n))
    elif -32 <= n < 0:
        out.append(struct.pack("b", n))
    elif n >= 0:
        if n < 1 << 8:
            out.append(b"\xcc" + struct.pack(">B", n))
        elif n < 1 << 16:
            out.append(b"\xcd" + struct.pack(">H", n))
        elif n < 1 << 32:
            out.append(b"\xce" + struct.pack(">I", n))
        elif n < 1 << 64:
            out.append(b"\xcf" + struct.pack(">Q", n))
        else:
            raise OverflowError("integer too large for msgpack")
    else:
        if n >= -(1 << 7):
            out.append(b"\xd0" + struct.pack(">b", n))
        elif n >= -(1 << 15):
            out.append(b"\xd1" + struct.pack(">h", n))
        elif n >= -(1 << 31):
            out.append(b"\xd2" + struct.pack(">i", n))
        elif n >= -(1 << 63):
            out.append(b"\xd3" + struct.pack(">q", n))
        else:
            raise OverflowError("integer too small for msgpack")


def _pack_sized(n, out, fixed, fixed_limit, codes):
    """Write the header of a str / array / map of `n` items (`codes` are the 8-, 16- and 32-bit markers)."""
    if n < fixed_limit and fixed is not None:
        out.append(struct.pack("B", fixed | n))
    elif codes[0] is not None and n < 1 << 8:
        out.append(codes[0] + struct.pack(">B", n))
    elif n < 1 << 16:
        out.append(codes[1] + struct.pack(">H", n))
    elif n < 1 << 32:
        out.append(codes[2] + struct.pack(">I", n))
    else:
        raise OverflowError("too long for msgpack")


def _pack_into(obj, out):
    if obj is None:
        out.append(b"\xc0")
    elif obj is True:
        out.append(b"\xc3")
    elif obj is False:
        out.append(b"\xc2")
    elif isinstance(obj, int):
        _pack_int(obj, out)
    elif isinstance(obj, float):
        out.append(b"\xcb" + struct.pack(">d", obj))
    elif isinstance(obj, str):
        data = obj.encode("utf-8")
        _pack_sized(len(data), out, 0xA0, 32, (b"\xd9", b"\xda", b"\xdb"))
        out.append(data)
    elif isinstance(obj, (bytes, bytearray)):
        _pack_sized(len(obj), out, None, 0, (b"\xc4", b"\xc5", b"\xc6"))
        out.append(bytes(obj))
    elif isinstance(obj, (list, tuple)):
        _pack_sized(len(obj), out, 0x90, 16, (None, b"\xdc", b"\xdd"))
        for item in obj:
            _pack_into(item, out)
    elif isinstance(obj, dict):
        _pack_sized(len(obj), out, 0x80, 16, (None, b"\xde", b"\xdf"))
        for key, value in obj.items():
            _pack_into(key, out)
            _pack_into(value, out)
    else:
        raise TypeError("cannot pack a %s" % type(obj).__name__)


def _builtin_packb(obj):
    out = []
    _pack_into(obj, out)
    return b"".join(out)


def _take(data, pos, n):
    if pos + n > len(data):
        raise ValueError("msgpack data ended early")
    return data[pos:pos + n], pos + n


def _length(data, pos, fmt):
    size = struct.calcsize(fmt)
    raw, pos = _take(data, pos, size)
    return struct.unpack(fmt, raw)[0], pos


_FIXED = {
    0xCA: ">f", 0xCB: ">d",
    0xCC: ">B", 0xCD: ">H", 0xCE: ">I", 0xCF: ">Q",
    0xD0: ">b", 0xD1: ">h", 0xD2: ">i", 0xD3: ">q",
}
_BIN = {0xC4: ">B", 0xC5: ">H", 0xC6: ">I"}
_STR = {0xD9: ">B", 0xDA: ">H", 0xDB: ">I"}
_ARRAY = {0xDC: ">H", 0xDD: ">I"}
_MAP = {0xDE: ">H", 0xDF: ">I"}


def _unpack_array(data, pos, n):
    items = []
    for _ in range(n):
        item, pos = _unpack_from(data, pos)
        items.append(item)
    return items, pos


def _unpack_map(data, pos, n):
    result = {}
    for _ in range(n):
        key, pos = _unpack_from(data, pos)
        value, pos = _unpack_from(data, pos)
        result[key] = value
    return result, pos


def _unpack_from(data, pos):
    raw, pos = _take(data, pos, 1)
    b = raw[0]
    if b <= 0x7F:
        return b, pos
    if b >= 0xE0:
        return b - 256, pos
    if 0x80 <= b <= 0x8F:
        return _unpack_map(data, pos, b & 0x0F)
    if 0x90 <= b <= 0x9F:
        return _unpack_array(data, pos, b & 0x0F)
    if 0xA0 <= b <= 0xBF:
        raw, pos = _take(data, pos, b & 0x1F)
        return bytes(raw).decode("utf-8"), pos
    if b == 0xC0:
        return None, pos
    if b == 0xC2:
        return False, pos
    if b == 0xC3:
        return True, pos
    if b in _FIXED:
        fmt = _FIXED[b]
        raw, pos = _take(data, pos, struct.calcsize(fmt))
        return struct.unpack(fmt, raw)[0], pos
    if b in _BIN:
        n, pos = _length(data, pos, _BIN[b])
        raw, pos = _take(data, pos, n)
        return bytes(raw), pos
    if b in _STR:
        n, pos = _length(data, pos, _STR[b])
        raw, pos = _take(data, pos, n)
        return bytes(raw).decode("utf-8"), pos
    if b in _ARRAY:
        n, pos = _length(data, pos, _ARRAY[b])
        return _unpack_array(data, pos, n)
    if b in _MAP:
        n, pos = _length(data, pos, _MAP[b])
        return _unpack_map(data, pos, n)
    raise ValueError("unsupported msgpack type 0x%02x" % b)


def _builtin_unpackb(data):
    value, pos = _unpack_from(data, 0)
    if pos != len(data):
        raise ValueError("extra bytes after a msgpack value")
    return value


def _choose_msgpack(builtin):
    """(packb, unpackb) of the `msgpack` package when it is installed, otherwise the built-in ones."""
    if not builtin:
        try:
            import msgpack
        except ImportError:
            pass
        else:
            return (
                lambda obj: msgpack.packb(obj, use_bin_type=True),
                lambda data: msgpack.unpackb(data, raw=False),
            )
    return _builtin_packb, _builtin_unpackb


# ---------------------------------------------------------------------------
# The protocol: a length (4 bytes, big-endian), then one msgpack map.
# ---------------------------------------------------------------------------

def _read_exact(stream, n):
    """`n` bytes, or None when the stream ended before the first byte; EOFError when it ended in the middle."""
    parts = []
    remaining = n
    while remaining:
        chunk = stream.read(remaining)
        if not chunk:
            if remaining == n:
                return None
            raise EOFError("the message ended early")
        parts.append(chunk)
        remaining -= len(chunk)
    return b"".join(parts)


def _read_frame(stream):
    head = _read_exact(stream, 4)
    if head is None:
        return None
    (size,) = struct.unpack(">I", head)
    if size > MAX_MESSAGE_BYTES:
        raise ValueError("a message of %d bytes is too large" % size)
    if size == 0:
        return b""
    body = _read_exact(stream, size)
    if body is None:
        raise EOFError("the message ended early")
    return body


class _Channel:
    def __init__(self, stream_in, stream_out, packb, unpackb):
        self._in = stream_in
        self._out = stream_out
        self._packb = packb
        self._unpackb = unpackb

    def send(self, message):
        message = dict(message)
        message["v"] = PROTOCOL_VERSION
        body = self._packb(message)
        self._out.write(struct.pack(">I", len(body)))
        self._out.write(body)
        self._out.flush()

    def receive(self):
        """The next message (a dict), or None when YUI closed the standard input."""
        body = _read_frame(self._in)
        if body is None:
            return None
        message = self._unpackb(body)
        if not isinstance(message, dict):
            raise ValueError("a message must be a map")
        if message.get("v") != PROTOCOL_VERSION:
            raise ValueError("protocol version %r is not supported" % (message.get("v"),))
        return message


# ---------------------------------------------------------------------------
# The plugin side.
# ---------------------------------------------------------------------------

class _App:
    def __init__(self, channel):
        self._channel = channel
        self.commands = {}
        self.items = []

    def log(self, message):
        self._channel.send({"type": "log", "text": str(message)})

    def add_command(self, command_id, func):
        if not callable(func):
            raise TypeError("add_command needs a function")
        self.commands[str(command_id)] = func

    def add_menu_item(self, location, order, label, command, args=None):
        self.items.append({
            "location": str(location),
            "order": int(order),
            "label": str(label),
            "command": str(command),
            "args": {} if args is None else args,
        })


class _Context:
    def __init__(self, target):
        self.target = target


def _load(folder, name):
    module_name = "yui_plugin_" + name.replace("-", "_")
    spec = importlib.util.spec_from_file_location(
        module_name,
        os.path.join(folder, "__init__.py"),
        submodule_search_locations=[folder],
    )
    module = importlib.util.module_from_spec(spec)
    sys.modules[module_name] = module
    spec.loader.exec_module(module)
    return module


def _invoke(app, message):
    request_id = message.get("id")
    command = message.get("command")
    func = app.commands.get(command)
    if func is None:
        return {"type": "result", "id": request_id, "ok": False,
                "error": "unknown command '%s'" % (command,), "result": None}
    args = message.get("args")
    try:
        value = func(_Context(message.get("target")), {} if args is None else args)
        return {"type": "result", "id": request_id, "ok": True, "error": None, "result": value}
    except Exception as e:
        traceback.print_exc()
        return {"type": "result", "id": request_id, "ok": False,
                "error": "%s: %s" % (type(e).__name__, e), "result": None}


class _OneshotApp:
    """`app` for one command: the menu items come from plugin.yaml, and there is no channel for the log."""

    def __init__(self):
        self.commands = {}

    def log(self, message):
        sys.stderr.write(str(message) + "\n")
        sys.stderr.flush()

    def add_command(self, command_id, func):
        if not callable(func):
            raise TypeError("add_command needs a function")
        self.commands[str(command_id)] = func

    def add_menu_item(self, location, order, label, command, args=None):
        pass


def main_oneshot(folder, name, in_path, out_path, packb, unpackb):
    try:
        signal.signal(signal.SIGINT, signal.SIG_IGN)
    except (ValueError, OSError):
        pass
    with open(in_path, "rb") as f:
        message = unpackb(f.read())
    if not isinstance(message, dict) or message.get("v") != PROTOCOL_VERSION:
        sys.stderr.write("the input file is not a version %d message\n" % PROTOCOL_VERSION)
        return 2
    app = _OneshotApp()
    try:
        module = _load(folder, name)
        module.register(app)
    except Exception as e:
        traceback.print_exc()
        sys.stderr.write("register failed: %s: %s\n" % (type(e).__name__, e))
        return 2

    reply = _invoke(app, message)
    del reply["type"], reply["id"]
    reply["v"] = PROTOCOL_VERSION
    try:
        body = packb(reply)
    except (TypeError, OverflowError, ValueError) as e:
        body = packb({"v": PROTOCOL_VERSION, "ok": False, "result": None,
                      "error": "the result cannot be sent (%s: %s)" % (type(e).__name__, e)})
    with open(out_path, "wb") as f:
        f.write(body)
    return 0


def main(argv):
    if len(argv) < 4:
        sys.stderr.write("usage: runner <plugin folder> <plugin name> <plugin version> [--builtin-msgpack]\n")
        return 2
    folder, name, version = argv[1], argv[2], argv[3]
    packb, unpackb = _choose_msgpack("--builtin-msgpack" in argv[4:])
    if "--oneshot" in argv[4:]:
        at = argv.index("--oneshot")
        if len(argv) < at + 3:
            sys.stderr.write("--oneshot needs an input file and an output file\n")
            return 2
        return main_oneshot(folder, name, argv[at + 1], argv[at + 2], packb, unpackb)

    # Ctrl+C in the terminal that runs YUI must not end the plugin; YUI ends it by closing the standard input.
    try:
        signal.signal(signal.SIGINT, signal.SIG_IGN)
    except (ValueError, OSError):
        pass

    # Keep the real standard input and output for the protocol only.
    stream_out = os.fdopen(os.dup(1), "wb")
    stream_in = os.fdopen(os.dup(0), "rb")
    os.dup2(2, 1)
    sys.stdout = sys.stderr
    sys.stdin = open(os.devnull, "r")

    channel = _Channel(stream_in, stream_out, packb, unpackb)
    app = _App(channel)
    try:
        module = _load(folder, name)
        module.register(app)
        channel.send({"type": "hello", "name": name, "version": version, "api_version": API_VERSION})
        channel.send({"type": "register", "commands": list(app.commands), "menu_items": app.items})
    except Exception as e:
        traceback.print_exc()
        sys.stderr.write("register failed: %s: %s\n" % (type(e).__name__, e))
        return 2

    while True:
        try:
            message = channel.receive()
        except Exception as e:
            sys.stderr.write("protocol error: %s: %s\n" % (type(e).__name__, e))
            return 1
        if message is None:
            return 0
        if message.get("type") == "invoke":
            reply = _invoke(app, message)
            try:
                channel.send(reply)
            except (TypeError, OverflowError, ValueError) as e:
                # The command returned something msgpack cannot hold.
                channel.send({"type": "result", "id": reply["id"], "ok": False,
                              "error": "the result cannot be sent (%s: %s)" % (type(e).__name__, e),
                              "result": None})


if __name__ == "__main__":
    sys.exit(main(sys.argv))
