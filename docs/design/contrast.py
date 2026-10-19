def lin(c):
    c=c/255
    return c/12.92 if c<=0.03928 else ((c+0.055)/1.055)**2.4
def L(rgb): r,g,b=rgb; return 0.2126*lin(r)+0.7152*lin(g)+0.0722*lin(b)
def cr(a,b):
    la,lb=L(a),L(b); hi,lo=max(la,lb),min(la,lb); return (hi+0.05)/(lo+0.05)
def h(s): s=s.lstrip('#'); return tuple(int(s[i:i+2],16) for i in (0,2,4))
def over(fg,a,bg): return tuple(round(f*a/255+b*(1-a/255)) for f,b in zip(fg,bg))
bg=h('10131A'); side=h('0B0E14')
glass=over(h('2A3140'),150,bg)
glass_sec=over(h('2A3140'),150,over(h('2B4A6A'),40,bg))
well=over((0,0,0),60,glass)
row_sel=over(h('4EA8FF'),int(255*0.09),glass)
surfaces={'bg':bg,'sidebar':side,'glass':glass,'well':well,'row_sel':row_sel,'dialog':h('1A1F2A'),'bar':over(h('1C222E'),235,bg)}
import sys
texts={k:h(v) for k,v in [a.split('=') for a in sys.argv[1:]]}
print('glass',['%02X'%c for c in glass])
for tn,tc in texts.items():
    print(tn, '  '.join(f"{sn}:{cr(tc,sc):.2f}" for sn,sc in surfaces.items()))
