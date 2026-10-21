#!/usr/bin/env python3
"""実際のウィンドウの起動確認（スモークテスト）。

仮想の画面（Xvfb）で、バックエンドとフロントエンドを実際に起動し、xdotool でクリックを送って、
画面（フレームバッファ）を PNG に保存する。見る人（や AI）が PNG を見て確かめるほか、次を自動で確かめる:
窓が開く、画面が空でない、操作で画面が変わる、プロセスが生きている、ログに panic がない。

使い方: python3 scripts/smoke-native.py [出力先]     （既定: target/smoke）
必要なもの: Xvfb、xdotool、python3（Pillow は不要）。ないときは、読み飛ばして 0 で終わる。
描画は、iced のソフトウェア描画（tiny-skia）を使う。Xvfb には GPU のサーフェスがないため、
3D の分子図は描かれない（3D は scripts/render-gallery.sh で確かめる）。OS のファイル・フォルダの
ダイアログも出ない。ファイルは、バックエンドのファイル一覧（Project タブ）から開く。
座標は、窓の大きさ 1024x768（既定）の配置に合わせてある。配置を変えたら、ここを直す。
"""
import os
import shutil
import signal
import socket
import struct
import subprocess
import sys
import time
import zlib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, "target", "smoke"))


def need(tool):
    return shutil.which(tool) is not None


def read_xwd(path):
    """XWD のフレームバッファを (幅, 高さ, RGB のバイト列の行リスト) にする"""
    with open(path, "rb") as f:
        data = f.read()
    h = struct.unpack(">25I", data[:100])
    header_size, width, height = h[0], h[4], h[5]
    bpp, bpl, ncolors = h[11], h[12], h[19]
    masks = (h[14], h[15], h[16])
    shifts = [next(s for s in range(32) if (m >> s) & 1) for m in masks]
    fmt = "<I" if h[7] == 0 else ">I"
    off = header_size + ncolors * 12
    px = bpp // 8
    rows = []
    for y in range(height):
        line = data[off + y * bpl: off + y * bpl + width * px]
        row = bytearray()
        for x in range(width):
            v = struct.unpack_from(fmt, line, x * px)[0]
            row += bytes(((v & masks[0]) >> shifts[0], (v & masks[1]) >> shifts[1], (v & masks[2]) >> shifts[2]))
        rows.append(bytes(row))
    return width, height, rows


def write_png(path, width, height, rows):
    def chunk(t, d):
        c = struct.pack(">I", len(d)) + t + d
        return c + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)

    raw = b"".join(b"\x00" + r for r in rows)
    png = (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
           + chunk(b"IDAT", zlib.compress(raw, 6)) + chunk(b"IEND", b""))
    with open(path, "wb") as f:
        f.write(png)


def distinct_colors(rows, limit=64):
    seen = set()
    for r in rows:
        for x in range(0, len(r), 3):
            seen.add(r[x:x + 3])
            if len(seen) >= limit:
                return len(seen)
    return len(seen)


def differing_pixels(a, b, region=None):
    """2 枚の画面で違う画素の数。`region` = (x0, y0, x1, y1) なら、その範囲だけ"""
    x0, y0, x1, y1 = region if region else (0, 0, 1 << 30, 1 << 30)
    n = 0
    for y, (ra, rb) in enumerate(zip(a, b)):
        if y < y0 or y >= y1 or ra == rb:
            continue
        for x in range(max(x0, 0), min(x1, len(ra) // 3)):
            if ra[3 * x:3 * x + 3] != rb[3 * x:3 * x + 3]:
                n += 1
    return n


class Smoke:
    def __init__(self):
        self.procs = []
        self.failures = []
        self.display = None
        self.fb = None
        self.last_rows = None
        self.env = None

    def start(self, args, env, log):
        f = open(log, "w")
        p = subprocess.Popen(args, env=env, stdout=f, stderr=subprocess.STDOUT, cwd=ROOT, start_new_session=True)
        self.procs.append(p)
        return p

    def stop(self):
        for p in reversed(self.procs):
            try:
                os.killpg(p.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
        for p in self.procs:
            try:
                p.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(p.pid, signal.SIGKILL)

    def check(self, ok, message):
        print(("  ok   " if ok else "  FAIL ") + message)
        if not ok:
            self.failures.append(message)

    def xdo(self, *args):
        subprocess.run(["xdotool", *args], env=self.env, check=False, timeout=20)

    def click(self, x, y, wait=1.5):
        self.xdo("mousemove", str(x), str(y), "click", "1")
        time.sleep(wait)

    def shot(self, name, expect_change=None, region=None):
        """画面を保存する。`expect_change` が真なら、前の画面と違うことを確かめる（`region` なら、その範囲だけ）"""
        src = os.path.join(self.fb, "Xvfb_screen0")
        width, height, rows = read_xwd(src)
        write_png(os.path.join(OUT, f"{name}.png"), width, height, rows)
        colors = distinct_colors(rows)
        self.check(colors > 8, f"{name}: the screen is not blank ({colors}+ colors)")
        if expect_change is not None and self.last_rows is not None:
            diff = differing_pixels(self.last_rows, rows, region)
            self.check((diff > 200) == expect_change,
                       f"{name}: the {'region' if region else 'screen'} {'changed' if expect_change else 'did not change'} ({diff} pixels differ)")
        self.last_rows = rows
        return rows


def wait_region_change(smoke, region, seconds):
    """前の画面から、`region` が変わるまで待つ（読み込みに時間がかかる操作のため）。変わったら真"""
    end = time.time() + seconds
    src = os.path.join(smoke.fb, "Xvfb_screen0")
    while time.time() < end:
        _w, _h, rows = read_xwd(src)
        if differing_pixels(smoke.last_rows, rows, region) > 100:
            return True
        time.sleep(2)
    return False


def free_display():
    for n in range(90, 120):
        if not os.path.exists(f"/tmp/.X11-unix/X{n}") and not os.path.exists(f"/tmp/.X{n}-lock"):
            return f":{n}"
    return None


def wait_port(port, seconds):
    end = time.time() + seconds
    while time.time() < end:
        with socket.socket() as s:
            s.settimeout(0.5)
            if s.connect_ex(("127.0.0.1", port)) == 0:
                return True
        time.sleep(0.3)
    return False


def main():
    missing = [t for t in ("Xvfb", "xdotool") if not need(t)]
    if missing:
        print(f"smoke: skipped (missing: {', '.join(missing)})")
        return 0
    with socket.socket() as s:
        if s.connect_ex(("127.0.0.1", 8080)) == 0:
            print("smoke: skipped (port 8080 is in use; the backend needs it)")
            return 0
    os.makedirs(OUT, exist_ok=True)
    print("smoke: building the binaries...")
    subprocess.run(["cargo", "build", "-p", "yui-frontend", "-p", "yui-backend"], cwd=ROOT, check=True)
    target = os.environ.get("CARGO_TARGET_DIR", os.path.join(ROOT, "target"))
    frontend = os.path.join(target, "debug", "yui-frontend")
    backend = os.path.join(target, "debug", "yui-backend")

    s = Smoke()
    display = free_display()
    if display is None:
        print("smoke: skipped (no free X display number)")
        return 0
    s.display = display
    s.fb = os.path.join(OUT, "fb")
    shutil.rmtree(s.fb, ignore_errors=True)
    os.makedirs(s.fb)
    # 実際の画面（Wayland / X11）に窓を出さないよう、仮想の画面だけを指す
    env = {k: v for k, v in os.environ.items() if k not in ("WAYLAND_DISPLAY", "XDG_SESSION_TYPE")}
    # backend は、起動のたびに、ランダムな秘密を `<設定フォルダ>/yui/backend.json` に書く。利用者の本物の設定フォルダ
    # （動いている backend の秘密や settings.yaml）に触れないよう、この実行だけの設定フォルダを使う
    config_home = os.path.join(OUT, "config")
    shutil.rmtree(config_home, ignore_errors=True)
    os.makedirs(config_home)
    env.update(DISPLAY=display, WINIT_UNIX_BACKEND="x11", ICED_BACKEND="tiny-skia", XDG_CONFIG_HOME=config_home)
    s.env = env
    try:
        s.start(["Xvfb", display, "-screen", "0", "1280x800x24", "-fbdir", s.fb, "-nolisten", "tcp"], os.environ, os.path.join(OUT, "xvfb.log"))
        time.sleep(2)
        benv = dict(env, YUI_WORKSPACE_ROOT=os.path.join(ROOT, "core", "tests", "data"))
        s.start([backend], benv, os.path.join(OUT, "backend.log"))
        s.check(wait_port(8080, 20), "the backend listens on 8080")
        app = s.start([frontend], env, os.path.join(OUT, "frontend.log"))
        time.sleep(12)
        win = subprocess.run(["xdotool", "search", "--onlyvisible", "--pid", str(app.pid)], env=env,
                             capture_output=True, text=True).stdout.split()
        s.check(bool(win), "the app window is open")
        s.check(app.poll() is None, "the app is running")
        s.shot("01_start")

        print("smoke: opening 1hls.pdb from the Project tab (backend file list)")
        s.click(153, 93, 3)                     # 左のペインの「Project」タブ（2 行目）
        s.shot("02_project_tab", expect_change=True)
        s.click(58, 165, 4)                     # 1hls.pdb
        # 右の Inspector に、表現のカードが出る
        s.shot("03_structure_loaded", expect_change=True, region=(820, 100, 1020, 460))

        print("smoke: File menu and the PNG dialog")
        s.click(30, 13)                         # File
        s.shot("04_file_menu", expect_change=True)
        s.click(79, 195)                        # Export Image (PNG)...
        s.shot("05_png_dialog", expect_change=True)
        s.click(383, 490)                       # Cancel
        s.shot("06_png_dialog_closed", expect_change=True)

        print("smoke: View menu")
        s.click(83, 13)                         # View
        s.shot("07_view_menu", expect_change=True)
        s.click(520, 400)                       # メニューの外をクリックして閉じる（3D の分子図の空き地）
        s.shot("07b_menu_closed", expect_change=True, region=(0, 0, 230, 480))

        print("smoke: a multi-model structure (1HLS.cif) and the playback bar")
        s.click(56, 146, 2)                     # 1HLS.cif（クリックで選び、続けて読み込む）
        s.shot("08a_selected")
        # 15,640 原子の読み込みは、デバッグビルドでは数秒かかる。下のステータスバーの文言が変わるまで待つ
        status = (0, 745, 700, 768)
        s.check(wait_region_change(s, status, 60), "1HLS.cif finishes loading (the status bar message changes)")
        s.shot("08_multi_model_loaded", expect_change=True, region=status)

        s.check(app.poll() is None, "the app is still running after all the steps")
        log = open(os.path.join(OUT, "frontend.log"), errors="replace").read()
        s.check("panicked" not in log, "no panic in the frontend log")
        blog = open(os.path.join(OUT, "backend.log"), errors="replace").read()
        s.check("panicked" not in blog, "no panic in the backend log")
    finally:
        s.stop()
    shutil.rmtree(s.fb, ignore_errors=True)
    print(f"smoke: screenshots and logs are in {OUT}")
    if s.failures:
        print(f"smoke: {len(s.failures)} check(s) failed")
        return 1
    print("smoke: all checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
