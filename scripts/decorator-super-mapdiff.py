#!/usr/bin/env python3
"""Show the first differing source-map mappings of one A6-41-SUPER capture.

usage: decorator-super-mapdiff.py <captures-dir> <case-id-substring> [max]
"""
from pathlib import Path
import base64, json, sys

B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'
def vlq(s):
    out, shift, value = [], 0, 0
    for ch in s:
        d = B64.index(ch); cont = d & 32; d &= 31
        value += d << shift; shift += 5
        if not cont:
            out.append(-(value >> 1) if value & 1 else value >> 1); value = shift = 0
    return out

def decode(map_json):
    m = json.loads(map_json)
    rows = []
    gl = 0; sf = sl = sc = nm = 0
    for line in m['mappings'].split(';'):
        gc = 0
        for seg in filter(None, line.split(',')):
            v = vlq(seg); gc += v[0]
            if len(v) >= 4:
                sf += v[1]; sl += v[2]; sc += v[3]
                entry = (gl, gc, sf, sl, sc)
                if len(v) == 5: nm += v[4]; entry += (m['names'][nm],)
                rows.append(entry)
            else:
                rows.append((gl, gc))
        gl += 1
    return rows, m

def main():
    captures, needle = Path(sys.argv[1]), sys.argv[2]
    limit = int(sys.argv[3]) if len(sys.argv) > 3 else 12
    for f in sorted(captures.glob('*.json')):
        c = json.loads(f.read_text())
        if needle not in c['case_id'] or c['actual'] is None: continue
        ew = {w['path']: w for w in c['expected']['writes']}; aw = {w['path']: w for w in c['actual']['writes']}
        for path in ew:
            if not path.endswith('.map') or path not in aw: continue
            e = base64.b64decode(ew[path]['callback_utf8_base64']).decode(); a = base64.b64decode(aw[path]['callback_utf8_base64']).decode()
            if e == a: continue
            er, em = decode(e); ar, am = decode(a)
            js_path = path[:-4]
            js = base64.b64decode(aw[js_path]['callback_utf8_base64']).decode().split('\n') if js_path in aw else []
            src = c['expected']['files'][0]['text'].split('\n') if 'files' in c['expected'] else None
            print('==', c['case_id'], path, 'expected', len(er), 'actual', len(ar), 'names', em.get('names') == am.get('names'))
            shown = 0
            for i in range(max(len(er), len(ar))):
                x = er[i] if i < len(er) else None; y = ar[i] if i < len(ar) else None
                if x != y:
                    gl = (x or y)[0]
                    print(f'  #{i} expected={x} actual={y}')
                    if gl < len(js): print('     gen:', js[gl][:140])
                    shown += 1
                    if shown >= limit: break
        break

main()
