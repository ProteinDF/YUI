"""Example plugin: adds View > "Say hello", which writes a line to plugin.log."""


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
