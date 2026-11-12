#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Quarkboy
"""screen.py NAME...: each score set's summary, a line each (out/score/NAME.tsv and its
NAME.stats.jsonl): its score and standard error, exits, the hurts from close attackers out of
view (a run, a minute), health lost a minute, glances and alerts a run. See docs/TESTING.md."""
import os
import json, csv, sys, collections, math, statistics as st
DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', 'out', 'score')


def load(name):
    rows = {(r['wad'], r['map'], r['skill'], r['seed']): r for r in csv.DictReader(open(os.path.join(DIR, f'{name}.tsv')), delimiter='\t')}
    out = []
    for l in open(os.path.join(DIR, f'{name}.stats.jsonl')):
        d = json.loads(l); k = (d['wad'], d['map'], str(d['skill']), str(d['seed']))
        if k in rows: out.append((rows[k], d['stats']))
    return out
def pts(r):
    if r['outcome'] == 'EXITED': return 1.0
    k, t = int(r['kills']), int(r['kills_total']); return 0.5 * min(1, k / t) if t else 0.0
unseen = lambda s: sum(v for k, v in s.items() if k.startswith('hurt by a') and 'in view' not in k and ', near, to a side' in k)
print(f"{'set':22} {'runs':>4} {'score':>12} {'exits':>5} {'unseen/run':>14} {'unseen/min':>11} {'lost/min':>9} {'glances':>8} {'alerts':>7}")
for name in sys.argv[1:]:
    d = load(name); n = len(d)
    p = [pts(r) for r, _ in d]; u = [unseen(s) for _, s in d]
    mins = sum(float(r['seconds']) for r, _ in d) / 60
    se = lambda v: st.pstdev(v) / math.sqrt(len(v))
    al = sum(v for _, s in d for k, v in s.items() if k.startswith('alert:')) / n
    gl = sum(s.get('glances', 0) for _, s in d) / n
    print(f"{name:22} {n:4} {st.mean(p):6.3f}±{se(p):.3f} {sum(r['outcome']=='EXITED' for r,_ in d):5} {st.mean(u):7.1f}±{se(u):4.1f} {sum(u)/mins:11.2f} {sum(s.get('health lost',0) for _,s in d)/mins:9.1f} {gl:8.1f} {al:7.1f}")
