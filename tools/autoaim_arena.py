# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Quarkboy
"""Write arenas/autoaim.txt: rockets fired down off ledges into pits and over low railings, where
Doom's autoaim tilts the rocket down into the ledge's lip or the railing. Sites from a
search of Doom / Doom II (a ledge over a pit at least 64 lower; a thin raised railing between two
open floors); per site, her by the edge (or a little back from the railing) facing across, five
imps placed at random beyond, the rocket launcher. Seeded: the same file every run."""
import struct, random, math, os, zlib
BASE = os.path.expanduser(os.environ.get('EVILBOOM_DOOM_DIR') or os.environ.get('EVILDOOM_DOOM_DIR') or ('~/.local/share/evilboom' if os.path.isdir(os.path.expanduser('~/.local/share/evilboom')) else '~/.local/share/evildoom'))
WADS = {'doom': BASE + '/base/DOOM.WAD', 'doom2': BASE + '/base/doom2/DOOM2.WAD'}
# (wad, map, the sector she stands on, the sector the monsters are in, rail sector or None)
SITES = [
    ('doom2', 'MAP20', 170, 155, None), ('doom2', 'MAP15', 16, 14, None), ('doom2', 'MAP09', 73, 32, None),
    ('doom2', 'MAP13', 165, 164, None), ('doom', 'E1M6', 23, 20, None), ('doom2', 'MAP03', 103, 108, None),
    ('doom2', 'MAP08', 23, 22, None), ('doom2', 'MAP14', 96, 107, 110), ('doom2', 'MAP14', 176, 182, 187),
]
def load(path, name):
    d = open(path, 'rb').read(); n, off = struct.unpack('<ii', d[4:12])
    dirs = [struct.unpack('<ii8s', d[off+16*i:off+16*i+16]) for i in range(n)]
    names = [x[2].rstrip(b'\0').decode() for x in dirs]
    i = names.index(name)
    get = lambda k: next(dirs[i+j] for j in range(1, 11) if names[i+j] == k)
    V, L, SD, SE = get('VERTEXES'), get('LINEDEFS'), get('SIDEDEFS'), get('SECTORS')
    vx = [struct.unpack('<hh', d[V[0]+4*k:V[0]+4*k+4]) for k in range(V[1]//4)]
    lines = [struct.unpack('<hhhhhhh', d[L[0]+14*k:L[0]+14*k+14]) for k in range(L[1]//14)]
    sides = [struct.unpack('<hh8s8s8sh', d[SD[0]+30*k:SD[0]+30*k+30]) for k in range(SD[1]//30)]
    secs = [struct.unpack('<hh8s8shhh', d[SE[0]+26*k:SE[0]+26*k+26]) for k in range(SE[1]//26)]
    segs = []  # (p, q, front sector, back sector)
    for (v1, v2, fl, sp, tg, r, l) in lines:
        segs.append((vx[v1], vx[v2], sides[r][5], sides[l][5] if l != -1 else None))
    return segs, secs
def inside(segs, s, p):
    c = False
    for a, b, f, k in segs:
        if s not in (f, k) or (f == s and k == s): continue
        if (a[1] > p[1]) != (b[1] > p[1]) and p[0] < (b[0]-a[0])*(p[1]-a[1])/(b[1]-a[1]) + a[0]: c = not c
    return c
def segd(p, a, b):
    dx, dy = b[0]-a[0], b[1]-a[1]; t = max(0, min(1, ((p[0]-a[0])*dx+(p[1]-a[1])*dy)/((dx*dx+dy*dy) or 1)))
    return math.hypot(a[0]+t*dx-p[0], a[1]+t*dy-p[1])
def bbox(segs, s):
    ps = [p for a, b, f, k in segs if s in (f, k) for p in (a, b)]
    return min(p[0] for p in ps), min(p[1] for p in ps), max(p[0] for p in ps), max(p[1] for p in ps)
def spot(rnd, segs, s, clear, near=None, within=None):
    x0, y0, x1, y1 = bbox(segs, s)
    for _ in range(20000):
        p = (rnd.uniform(x0, x1), rnd.uniform(y0, y1))
        if not inside(segs, s, p): continue
        if any(segd(p, a, b) < clear for a, b, f, k in segs if s in (f, k) and not (near and (f, k) in near)): continue
        if within and min(segd(p, a, b) for a, b, f, k in segs if (f, k) in within) > within_d: continue
        return p
    return None
out = ['# Rockets down into pits and over low railings (evil-boom arena arenas/autoaim.txt):',
       '# Doom\'s autoaim tilts a rocket down at what\'s below, into the ledge\'s lip or the railing.',
       '# Her by the edge (a little back from a railing) facing across, five imps beyond, the rocket',
       '# launcher; two placements a site (tools/autoaim_arena.py).', '']
for wad, mp, stand, pit, rail in SITES:
    segs, secs = load(WADS[wad], mp)
    edge = {(stand, pit), (pit, stand)} if rail is None else {(stand, rail), (rail, stand)}
    for v in range(2):
        rnd = random.Random(zlib.crc32(f"{mp}-{stand}-{v}".encode()))
        within_d = 64 if rail is None else 96
        me = spot(rnd, segs, stand, 20, near=edge, within=edge)
        if me is None: print('no spot', mp, stand); continue
        # (where a shot down can get to them: autoaim reaches 100/160 of a unit down a unit out, so
        # no nearer than the drop over that, and some)
        drop = secs[stand][0] - secs[pit][0]
        reach = max(256, drop / 0.625 + 48)
        imps = []
        for _ in range(4000):
            if len(imps) == 5: break
            p = spot(rnd, segs, pit, 32)
            if p and math.hypot(p[0]-me[0], p[1]-me[1]) > reach and all(math.hypot(p[0]-q[0], p[1]-q[1]) > 64 for q in imps): imps.append(p)
        if len(imps) < 3: print('too few imps far enough', mp, stand); continue
        cx = sum(p[0] for p in imps)/len(imps); cy = sum(p[1] for p in imps)/len(imps)
        ang = math.degrees(math.atan2(cy-me[1], cx-me[0])) % 360
        kind = 'pit' if rail is None else 'rail'
        out += [f'arena {kind}-{mp.lower()}-{stand}-{v+1}', f'# her on sector {stand} (floor {secs[stand][0]}), them in sector {pit} (floor {secs[pit][0]}, {drop:+} below)' + ('' if rail is None else f', over railing sector {rail} (floor {secs[rail][0]})'),
                f'map {wad} {mp}', f'at {me[0]:.0f} {me[1]:.0f} {ang:.0f}', 'give rocket rocket=40']
        out += [f'spawn DoomImp {x:.0f} {y:.0f} {math.degrees(math.atan2(me[1]-y, me[0]-x)) % 360:.0f}' for x, y in imps]
        out += ['seconds 90', '']
open(os.path.join(os.path.dirname(__file__), '..', 'arenas', 'autoaim.txt'), 'w').write('\n'.join(out))
print(sum(1 for l in out if l.startswith('arena')), 'scenarios')
