# Writing a YUI plugin

> [!NOTE]
> [Japanese version (日本語版)](plugin-authoring.ja.md) is also available.

A YUI plugin adds commands and menu items to the native desktop app. A plugin is usually a Python package written against the public `app` object described below. The design is in [architecture.md](../../architecture.md) §6.4.

> [!WARNING]
> A plugin runs with the same rights as any Python script you run. It is not sandboxed. Enable only plugins you trust.

## How a plugin runs

YUI starts each enabled plugin as its own process and talks to it over the standard input and output of that process. A plugin that crashes or hangs cannot stop YUI.

- A plugin with an `__init__.py` and no `command` in its `plugin.yaml` is a Python package. YUI runs it with a runner that it ships, using the Python named by `plugins.python` in `settings.yaml` (a path or a command name), or `python3` on the `PATH` (`python` on Windows) when that is not set. Python 3.8 or later is needed.
- Nothing else has to be installed: the runner has its own small msgpack, and it uses the `msgpack` package when that is installed. Packages installed in the chosen Python (for example `numpy`) can be imported by the plugin.
- What the plugin prints, and anything else it writes to the standard output or the standard error, goes to `plugin.log`.
- A plugin with a `command` in its `plugin.yaml` is run as it is, so it can be written in any language. The protocol it must speak is in [architecture.md](../../architecture.md) §6.4; a separate guide for it has not been written yet.

```yaml
plugins:
  enabled: [hello-yui]
  python: /usr/bin/python3      # optional; without it, python3 on the PATH is used
```

## Folder layout

Each plugin is one folder under the plugins folder of your configuration folder:

```text
<config>/yui/plugins/<name>/
    plugin.yaml
    __init__.py
    (other modules of the package)
```

`<config>` is your per-user configuration folder (on Linux `~/.config`, on macOS `~/Library/Application Support`, on Windows `%APPDATA%`). The example plugin in `examples/plugins/hello-yui/` shows the smallest working plugin.

## plugin.yaml

| Key | Required | Meaning |
|-----|----------|---------|
| `name` | yes | Must equal the folder name. Letters, digits, `_` and `-`; starts with a letter; at most 64 characters. Also the plugin's namespace. |
| `version` | yes | A version string, for example `0.1.0`. |
| `api_version` | yes | An integer. Must equal the API version of the app (currently `1`). |
| `description` | no | One line shown in the plugin list. |
| `author` | no | Shown in the plugin list. |
| `command` | no | The program and its arguments, as a list (no shell). Leave it out for a Python package. |
| `mode` | no | `resident` (the default): one process that runs while YUI runs. `oneshot`: a new process for each command call (see "The oneshot mode"). |
| `menu_items` | oneshot only | The menu items, as a list of `{location, order, label, command, args}` (`args` is optional). |
| `commands` | no | oneshot only: ids of commands that have no menu item. |

A plugin whose `plugin.yaml` does not fit is not loaded; the reason is shown in the plugin list and in `plugin.log`.

## Enabling a plugin

A plugin is not run until you enable it. Either open View → Plugins..., or list its name in `settings.yaml`:

```yaml
plugins:
  enabled: [hello-yui]
```

Changes apply the next time YUI starts.

## The app object

`register(app)` is called once at startup. The `app` object has these methods.

### `app.log(message)`

Appends `<name>: <message>` to `plugin.log` in the plugins folder.

### `app.add_command(id, func)`

Registers a command. `id` must start with `plugin.<name>.`, so a plugin cannot replace a built-in command or another plugin's command. `func(ctx, args)` is called in the plugin's own process when the command runs. `func` must be callable.

### `app.add_menu_item(location, order, label, command, args=None)`

Adds a menu item that runs a command you registered with `add_command`.

- `location`: one of `menu/file`, `menu/view`, `menu/help`, `context/viewport` (right-click on the 3D view), `context/structure_tree`, `context/representation`, `context/file`.
- `order`: an integer. Items are sorted by `order` (smaller first). Built-in items use multiples of 100; for example `150` puts an item between two built-in items.
- `label`: the text of the item.
- `command`: the `id` of a command your plugin added.
- `args`: optional. A value that can be written as JSON (a dictionary, list, string, number, boolean or `None`). It is passed to the command as `args`.

Plugin items are always enabled and never checked.

## The ctx argument

`func(ctx, args)` receives two arguments:

- `args`: the `args` you gave to `add_menu_item`, or `{}`.
- `ctx.target`: what the user right-clicked, or `None` when the command comes from a menu bar item.

`ctx.target` is a dictionary with a `kind` key:

| `kind` | Other keys |
|--------|------------|
| `atom` | `atom_id` (the atom number) |
| `tree_node` | `key` (the node key in the structure tree) |
| `representation` | `index` (the row index in the representation list) |
| `file` | `path` (the file in the project tree) |

Later versions may add more keys to `ctx`. Do not rely on `ctx` having only these keys.

## The oneshot mode

With `mode: oneshot`, YUI starts a new process for each command call and does not keep one running. Use it for a command that is run rarely, or that should not keep memory. There is no `register`, so the menu items are written in `plugin.yaml`:

```yaml
name: say-once
version: 0.1.0
api_version: 1
mode: oneshot
menu_items:
  - {location: menu/view, order: 500, label: "Say once", command: plugin.say-once.say, args: {greeting: hi}}
```

For a Python package, write `register(app)` and `func(ctx, args)` as usual: `register` is called in the new process only to find the command, `app.add_menu_item` is ignored and `app.log` writes to the standard error. For any other program, give a `command` that contains `{input}` and `{output}`, for example `command: [./run, "{input}", "{output}"]`. YUI writes the input file (one msgpack map: `v`, `command`, `target`, `args`), runs the command with the two paths, and reads the output file (one msgpack map: `v`, `ok`, `error`, `result`). A call fails when `ok` is false or the program exits with a non-zero status; one failed call does not disable the plugin, and the reason is shown in the status bar. Whatever the program writes to the standard output or the standard error goes to `plugin.log`. A call has no time limit; the processes still running are stopped when YUI quits.

## Failures and plugin.log

Every load and every failure is written to `plugin.log` in the plugins folder. The reasons are also shown in the status bar and in the plugin list. A failure never stops YUI:

- A wrong `plugin.yaml`, or an unsupported `api_version`, means the plugin is not loaded.
- An exception in `register` means the plugin is not loaded. Nothing it added is kept.
- An exception in a command disables the plugin for the rest of the session: its menu items disappear and its commands refuse to run.
- If the plugin process cannot start (for example, Python is not found; set `plugins.python`) or ends before it registers, the plugin is not loaded, and the last line it wrote to the standard error is the reason.
- `register` must finish within five seconds at startup; otherwise the plugin fails.
- A command has no time limit. A command that never returns keeps that plugin busy until YUI quits; other plugins are not affected.
- YUI stops every plugin process when it quits.

## api_version

`api_version` is the version of the plugin API. It changes only when a change breaks existing plugins, for example when a function is removed or changes its meaning. Adding a method, an argument with a default value, or a new key in `ctx` does not change `api_version`. A plugin written for `api_version` 1 keeps working while the app supports 1.

## Example

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

Copy the folder to `<config>/yui/plugins/hello-yui/`, enable it, restart YUI, and choose View → Say hello. A line appears in `plugin.log`.

## Not yet available

- Installing plugins with `pip` or entry points. Plugins are folders for now.
- A guide to writing a plugin in a language other than Python.
- Enabled or checked state computed by Python for a menu item.
- Calling the app's data (the structure, the selection) from a plugin: a read-only view is planned but not yet defined.
