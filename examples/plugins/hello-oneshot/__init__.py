"""Example plugin (oneshot mode): a new process runs for each call and writes a line to plugin.log.

The menu items are written in plugin.yaml. register() is called in the new process only to find the command.
"""

import os


def register(app):
    def say_hello(ctx, args):
        greeting = args.get("greeting", "hello")
        # The process id differs for every call: a new process is started each time.
        if ctx.target is None:
            app.log(f"{greeting} from the View menu (process {os.getpid()})")
        else:
            app.log(f"{greeting} about {ctx.target} (process {os.getpid()})")

    app.add_command("plugin.hello-oneshot.say", say_hello)
