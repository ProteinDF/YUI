"""Example plugin: adds a viewport right-click item that logs what was clicked.

ctx.target is a dict whose "kind" says what the user right-clicked, or None
when the command comes from the menu bar. Read keys with .get(), because
later versions may add keys to ctx.
"""


def register(app):
    def show_target(ctx, args):
        target = ctx.target
        if target is None:
            app.log("no target: the command was not run from a right-click")
            return
        kind = target.get("kind")
        if kind == "atom":
            app.log(f"atom {target.get('atom_id')}")
        else:
            app.log(f"target kind {kind}: {target}")

    app.add_command("plugin.atom-info.show", show_target)
    app.add_menu_item("context/viewport", 100, "Show target info", "plugin.atom-info.show")
