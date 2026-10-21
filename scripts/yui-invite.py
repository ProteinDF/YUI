#!/usr/bin/env python3
"""backend の招待（Editor / Viewer の、期限つきの秘密）を、作る・一覧する・取り消す（Host だけ）。

  scripts/yui-invite.py create viewer [--ttl 3600]   # 秘密と、渡す先ごとの使い方を表示する
  scripts/yui-invite.py list
  scripts/yui-invite.py revoke <id>

Host の秘密と URL は、`backend` が書く接続ファイル（<設定フォルダ>/yui/backend.json）から読む。別のコンピュータの
backend には、`--url http://host:8080 --token <Host の秘密>`（か環境変数 YUI_URL / YUI_TOKEN）を付ける。
標準ライブラリだけで動く（architecture.md 7.4節）。
"""
import argparse
import json
import os
import sys
import urllib.error
import urllib.request


def config_dir():
    """`dirs::config_dir()`（backend / frontend が使うもの）と同じ場所"""
    if sys.platform == "win32":
        return os.environ.get("APPDATA") or os.path.expanduser("~")
    if sys.platform == "darwin":
        return os.path.join(os.path.expanduser("~"), "Library", "Application Support")
    return os.environ.get("XDG_CONFIG_HOME") or os.path.join(os.path.expanduser("~"), ".config")


def load_connection(args):
    url, token = args.url or os.environ.get("YUI_URL"), args.token or os.environ.get("YUI_TOKEN")
    if not (url and token):
        path = os.path.join(config_dir(), "yui", "backend.json")
        try:
            with open(path) as f:
                info = json.load(f)
        except (OSError, ValueError) as e:
            sys.exit(f"yui-invite: cannot read {path} ({e}); start the backend, or pass --url and --token")
        url, token = url or info.get("url"), token or info.get("token")
    if not (url and token):
        sys.exit("yui-invite: no URL / token")
    return url.rstrip("/"), token


def call(method, url, token, body=None):
    data = json.dumps(body).encode() if body is not None else None
    request = urllib.request.Request(url, data=data, method=method)
    request.add_header("Authorization", f"Bearer {token}")
    if data is not None:
        request.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(request, timeout=10) as response:
            text = response.read().decode()
            return response.status, json.loads(text) if text else None
    except urllib.error.HTTPError as e:
        sys.exit(f"yui-invite: {method} {url} -> {e.code} {e.read().decode().strip()}")
    except urllib.error.URLError as e:
        sys.exit(f"yui-invite: cannot reach {url} ({e.reason})")


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--url", help="backend URL (default: backend.json)")
    p.add_argument("--token", help="the Host secret (default: backend.json)")
    sub = p.add_subparsers(dest="command", required=True)
    c = sub.add_parser("create", help="create an invitation")
    c.add_argument("role", choices=["editor", "viewer"])
    c.add_argument("--ttl", type=int, default=3600, help="seconds the invitation is valid (60 - 604800, default 3600)")
    sub.add_parser("list", help="list the active invitations (no secrets)")
    r = sub.add_parser("revoke", help="revoke an invitation (its open connections close within seconds)")
    r.add_argument("id")
    args = p.parse_args()
    url, token = load_connection(args)

    if args.command == "create":
        _, made = call("POST", f"{url}/api/invites", token, {"role": args.role, "ttl_secs": args.ttl})
        print(f"invitation {made['id']} ({made['role']}), valid until unix time {made['expires_at']}")
        print(f"  token:    {made['token']}")
        print(f"  browser:  {url}/#token={made['token']}")
        print(f"  desktop:  yui-frontend --collab {url.replace('http', 'ws', 1)}/api/collab --token {made['token']}")
        print("The token is shown only now. Send it over a private channel; revoke it with:")
        print(f"  scripts/yui-invite.py revoke {made['id']}")
    elif args.command == "list":
        _, items = call("GET", f"{url}/api/invites", token)
        if not items:
            print("no active invitations")
        for i in items:
            print(f"{i['id']}  {i['role']:6}  expires {i['expires_at']}")
    else:
        call("DELETE", f"{url}/api/invites/{args.id}", token)
        print(f"revoked {args.id}")


if __name__ == "__main__":
    main()
