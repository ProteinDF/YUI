# YUI のアイコン（packaging/icons/）を作る: 3 つの原子（球）が結合（線）で結ばれた図（「結」= むすぶ）。
# 標準ライブラリだけで PNG と ICO を書く。使い方: python3 packaging/make-icons.py packaging/icons
import math, struct, sys, zlib

def render(size, ss=4):
    n = size * ss
    px = [[(0, 0, 0, 0)] * n for _ in range(n)]
    def over(x, y, c, a=1.0):
        r, g, b, pa = px[y][x]
        pa_f = pa / 255.0
        oa = a + pa_f * (1 - a)
        if oa <= 0: return
        px[y][x] = (int((c[0]*a + r*pa_f*(1-a))/oa), int((c[1]*a + g*pa_f*(1-a))/oa), int((c[2]*a + b*pa_f*(1-a))/oa), int(oa*255))
    s = n
    # 背景: 角丸の四角（暗い青）
    bg, rad = (24, 32, 56), 0.22 * s
    for y in range(n):
        for x in range(n):
            cx = min(max(x, rad), s - 1 - rad); cy = min(max(y, rad), s - 1 - rad)
            if (x - cx) ** 2 + (y - cy) ** 2 <= rad * rad:
                # 上から下へ、わずかなグラデーション
                t = y / s
                px[y][x] = (int(bg[0] + 14 * (1 - t)), int(bg[1] + 20 * (1 - t)), int(bg[2] + 36 * (1 - t)), 255)
    atoms = [(0.30, 0.66, 0.15, (240, 90, 80)), (0.70, 0.66, 0.15, (90, 160, 245)), (0.50, 0.30, 0.17, (250, 210, 90))]
    # 結合（太い線）
    def line(a, b, w, c):
        ax, ay, bx, by = a[0]*s, a[1]*s, b[0]*s, b[1]*s
        dx, dy = bx - ax, by - ay; l2 = dx*dx + dy*dy
        x0, x1 = int(min(ax, bx) - w), int(max(ax, bx) + w) + 1
        y0, y1 = int(min(ay, by) - w), int(max(ay, by) + w) + 1
        for y in range(max(y0, 0), min(y1, n)):
            for x in range(max(x0, 0), min(x1, n)):
                t = max(0, min(1, ((x - ax) * dx + (y - ay) * dy) / l2))
                if (x - (ax + t*dx))**2 + (y - (ay + t*dy))**2 <= (w/2)**2 and px[y][x][3] > 0:
                    over(x, y, c)
    for i, j in ((0, 1), (0, 2), (1, 2)):
        line(atoms[i][:2], atoms[j][:2], 0.045 * s, (200, 208, 224))
    # 原子（陰影つきの球）
    for ax, ay, r, col in atoms:
        cx, cy, rr = ax * s, ay * s, r * s
        for y in range(int(cy - rr) - 1, int(cy + rr) + 2):
            for x in range(int(cx - rr) - 1, int(cx + rr) + 2):
                d2 = (x - cx) ** 2 + (y - cy) ** 2
                if d2 <= rr * rr and 0 <= x < n and 0 <= y < n and px[y][x][3] > 0:
                    nz = math.sqrt(max(0.0, 1 - d2 / (rr * rr)))
                    nx, ny = (x - cx) / rr, (y - cy) / rr
                    l = max(0.0, (-0.45 * nx - 0.55 * ny + 0.7 * nz))
                    k = 0.35 + 0.65 * min(1.0, l / 0.9)
                    over(x, y, tuple(min(255, int(c * k + (40 if l > 0.85 else 0))) for c in col))
    # 縮小（ボックス平均）
    out = bytearray()
    for y in range(size):
        out.append(0)
        for x in range(size):
            r = g = b = a = 0
            for dy in range(ss):
                for dx in range(ss):
                    pr, pg, pb, pa = px[y*ss+dy][x*ss+dx]
                    r += pr * pa; g += pg * pa; b += pb * pa; a += pa
            if a == 0: out += bytes((0, 0, 0, 0))
            else: out += bytes((r // a, g // a, b // a, a // (ss * ss)))
    return bytes(out)

def png(size, raw):
    def chunk(t, d): return struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', size, size, 8, 6, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(raw, 9)) + chunk(b'IEND', b'')

import os
outdir = sys.argv[1]
os.makedirs(outdir, exist_ok=True)
for size in (32, 64, 128, 256, 512):
    open(f"{outdir}/icon-{size}.png", "wb").write(png(size, render(size, ss=4 if size > 64 else 6)))

# Windows 用の icon.ico（PNG を埋め込んだ ICO。Vista 以降で使える）
ico_sizes = (32, 64, 128, 256)
blobs = [open(f"{outdir}/icon-{s}.png", "rb").read() for s in ico_sizes]
header = struct.pack("<HHH", 0, 1, len(ico_sizes))
offset = 6 + 16 * len(ico_sizes)
entries = b""
for s, b in zip(ico_sizes, blobs):
    entries += struct.pack("<BBBBHHII", 0 if s >= 256 else s, 0 if s >= 256 else s, 0, 0, 1, 32, len(b), offset)
    offset += len(b)
open(f"{outdir}/icon.ico", "wb").write(header + entries + b"".join(blobs))

# macOS 用の icon.icns（PNG を埋め込んだ ICNS）。cargo-packager は、`.icns` があればそのままバンドルに入れる。
# 種類（OSType）と、その画素数: icp5=32, icp6=64, ic07=128, ic08=256, ic09=512（1 倍）と、
# ic11=32, ic12=64, ic13=256, ic14=512（Retina の枠。画素数は同じ PNG を使う）。
icns_types = (
    (b"icp5", 32), (b"icp6", 64), (b"ic07", 128), (b"ic08", 256), (b"ic09", 512),
    (b"ic11", 32), (b"ic12", 64), (b"ic13", 256), (b"ic14", 512),
)
chunks = b""
for ostype, size in icns_types:
    data = open(f"{outdir}/icon-{size}.png", "rb").read()
    chunks += ostype + struct.pack(">I", len(data) + 8) + data
open(f"{outdir}/icon.icns", "wb").write(b"icns" + struct.pack(">I", len(chunks) + 8) + chunks)
