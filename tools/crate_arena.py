"""Write arenas/crates.txt: E2M7's crate hall, 10 variations (start, facing, imps placed at random
among the crates, seeded), the player with the rocket launcher."""
import struct, random, math
d=open(__import__('os').path.expanduser((__import__('os').environ.get('EVILBOOM_DOOM_DIR') or __import__('os').environ.get('EVILDOOM_DOOM_DIR') or ('~/.local/share/evilboom' if __import__('os').path.isdir(__import__('os').path.expanduser('~/.local/share/evilboom')) else '~/.local/share/evildoom')))+'/base/DOOM.WAD','rb').read()
n,off=struct.unpack('<ii',d[4:12])
dirs=[struct.unpack('<ii8s',d[off+16*i:off+16*i+16]) for i in range(n)]
names=[x[2].rstrip(b'\0').decode() for x in dirs]
i=names.index('E2M7')
lump=lambda nm: dirs[i+1+[names[i+1+k] for k in range(10)].index(nm)]
V=lump('VERTEXES'); L=lump('LINEDEFS'); SD=lump('SIDEDEFS'); SE=lump('SECTORS')
vx=[struct.unpack('<hh',d[V[0]+4*k:V[0]+4*k+4]) for k in range(V[1]//4)]
lines=[struct.unpack('<hhhhhhh',d[L[0]+14*k:L[0]+14*k+14]) for k in range(L[1]//14)]
sides=[struct.unpack('<hh8s8s8sh',d[SD[0]+30*k:SD[0]+30*k+30]) for k in range(SD[1]//30)]
X0,Y0,X1,Y1=2624,992,3840,2112
room=195
boxes=[]  # (x0,y0,x1,y1)
others={}
for li,(v1,v2,fl,sp,tag,r,l) in enumerate(lines):
    a=sides[r][5]; b=sides[l][5] if l!=-1 else None
    if room not in (a,b): continue
    o=b if a==room else a
    p,q=vx[v1],vx[v2]
    if o is None:
        if p[0] in (X0,X1) and q[0]==p[0]: continue
        if p[1] in (Y0,Y1) and q[1]==p[1]: continue
        others.setdefault('solid',[]).append((p,q))
    else:
        others.setdefault(o,[]).append((p,q))
# group one-sided crate lines into boxes by shared corners
segs=others.pop('solid',[])
groups=[]
for s in segs:
    for g in groups:
        if any(pt in (t[0],t[1]) for t in g for pt in s): g.append(s); break
    else: groups.append([s])
for g in groups+list(others.values()):
    xs=[p[0] for s in g for p in s]; ys=[p[1] for s in g for p in s]
    boxes.append((min(xs),min(ys),max(xs),max(ys)))
def clear(x,y,m):
    if not (X0+m<x<X1-m and Y0+m<y<Y1-m): return False
    return all(not (b[0]-m<x<b[2]+m and b[1]-m<y<b[3]+m) for b in boxes)
out=['# Rockets and moving among crates and columns (evil-boom arena arenas/crates.txt; the user\'s:',
     '# a stress test for her rockets and movement): E2M7\'s crate hall (x 2624..3840, y 992..2112;',
     '# crates 64 high and full height scattered through it), eight solid columns put about it at',
     '# random (not in her map knowledge; a missile goes over them, 16 high to it), six imps among',
     '# them, the rocket launcher; ten layouts.',
     '# Generated: '+str(len(boxes))+' crate blocks avoided by 40 (her) and 32 (imps).','']
for v in range(10):
    rnd=random.Random(1000+v)
    while True:
        sx,sy=rnd.uniform(X0,X1),rnd.uniform(Y0,Y1)
        if clear(sx,sy,40): break
    ang=rnd.choice([0,45,90,135,180,225,270,315])
    cols=[]
    while len(cols)<8:
        x,y=rnd.uniform(X0,X1),rnd.uniform(Y0,Y1)
        if clear(x,y,24) and math.hypot(x-sx,y-sy)>64 and all(math.hypot(x-a,y-b)>48 for a,b in cols): cols.append((x,y))
    imps=[]
    while len(imps)<6:
        x,y=rnd.uniform(X0,X1),rnd.uniform(Y0,Y1)
        if clear(x,y,32) and math.hypot(x-sx,y-sy)>400 and all(math.hypot(x-a,y-b)>64 for a,b in imps+cols): imps.append((x,y))
    out+=['arena crates-%d'%(v+1),'map doom E2M7','at %d %d %d'%(sx,sy,ang),'give rocket rocket=40']
    out+=['spawn %s %d %d 0'%(rnd.choice(['TallGreenColumn','TallRedColumn','TechPillar']),x,y) for x,y in cols]
    out+=['spawn DoomImp %d %d %d'%(x,y,(math.degrees(math.atan2(sy-y,sx-x))%360)) for x,y in imps]
    out+=['seconds 90','']
open(__import__('os').path.join(__import__('os').path.dirname(__file__),'..','arenas','crates.txt'),'w').write('\n'.join(out))
print(len(boxes),'blocks'); print('\n'.join(out[:14]))
