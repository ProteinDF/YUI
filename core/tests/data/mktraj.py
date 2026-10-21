import sys, math
# usage: mktraj.py in.pdb out.xyz [frames]
src, dst = sys.argv[1], sys.argv[2]
n = int(sys.argv[3]) if len(sys.argv) > 3 else 20
atoms = []
for l in open(src):
    if l.startswith(("ATOM", "HETATM")):
        el = (l[76:78].strip() or l[12:16].strip()[:1]).capitalize()
        atoms.append((el, float(l[30:38]), float(l[38:46]), float(l[46:54])))
with open(dst, "w") as f:
    for i in range(n):
        f.write(f"{len(atoms)}\nstep={i*10}\n")
        for k, (e, x, y, z) in enumerate(atoms):
            d = 0.5 * math.sin(2 * math.pi * i / n + k * 0.05)
            f.write(f"{e} {x+d:.3f} {y+d:.3f} {z:.3f}\n")
print(len(atoms), "atoms,", n, "frames")
