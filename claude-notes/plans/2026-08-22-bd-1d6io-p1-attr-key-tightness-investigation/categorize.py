import json,os,re,sys

EX='ts-packages/annotated-qmd/examples'

def resolve(pool, idx, _d=0):
    if not isinstance(idx,int) or idx>=len(pool): return None
    e=pool[idx]; t,r,d=e.get('t'),e.get('r'),e.get('d')
    if r is None: return ('other',None,None)
    if t==0: return ('orig',r[0],r[1])
    if t==1 and _d<64:
        par=resolve(pool,d,_d+1)
        if par and par[1] is not None: return (par[0],par[1]+r[0],par[1]+r[1])
        return ('sub?',r[0],r[1])
    return ('t%s'%t,r[0],r[1])

def walk(node,pool,out,path):
    """Collect (path, type, resolved) for AST nodes AND attr-source sidecars."""
    if isinstance(node,dict):
        if 't' in node and 's' in node:
            out.append((path,node['t'],resolve(pool,node['s'])))
        a=node.get('a')
        if isinstance(a,dict):
            for slot in ('id','classes','kvs'):
                v=a.get(slot)
                if v is None: continue
                for j,item in enumerate(v if isinstance(v,list) else [v]):
                    for k,idx in enumerate(item if isinstance(item,list) else [item]):
                        out.append(('%s.a.%s[%d][%d]'%(path,slot,j,k),'attr',resolve(pool,idx)))
        for k,v in sorted(node.items()):
            if k in ('s','a'): continue
            walk(v,pool,out,path+'.'+k)
    elif isinstance(node,list):
        for i,v in enumerate(node): walk(v,pool,out,'%s[%d]'%(path,i))

def load(path):
    d=json.load(open(path)); pool=d['astContext']['p']; out=[]
    walk(d['blocks'],pool,out,'blocks'); walk(d['meta'],pool,out,'meta')
    return out,pool

rows=[]
for stem in sorted(x[:-4] for x in os.listdir(EX) if x.endswith('.qmd')):
    old,pold=load('/tmp/aqmd-old/%s.json'%stem)
    new,pnew=load('%s/%s.json'%(EX,stem))
    src=open('%s/%s.qmd'%(EX,stem),'rb').read()
    byte_diff = open('/tmp/aqmd-old/%s.json'%stem,'rb').read() != open('%s/%s.json'%(EX,stem),'rb').read()
    cats=set(); n=0
    if len(old)!=len(new):
        cats.add('NODE-COUNT')
    for a,b in zip(old,new):
        if a[2]==b[2]: continue
        n+=1
        ot = src[a[2][1]:a[2][2]].decode('utf8','replace') if a[2] and a[2][1] is not None else ''
        nt = src[b[2][1]:b[2][2]].decode('utf8','replace') if b[2] and b[2][1] is not None else ''
        if a[1]=='attr': cats.add('attr-key')
        elif ot.startswith((' ','\t')) and not nt.startswith((' ','\t')): cats.add('tightness')
        elif len(nt)>len(ot) and nt.startswith(ot): cats.add('meta-truncation')
        elif ot=='' : cats.add('missing-provenance')
        else: cats.add('OTHER:%s'%a[1])
    poolshift = (len(pold)!=len(pnew))
    rows.append((stem, n, poolshift, byte_diff, ','.join(sorted(cats)) or '—'))

print('%-18s %6s %6s %6s  %s'%('fixture','nodeΔ','poolΔ','bytesΔ','categories'))
for r in rows:
    print('%-18s %6d %6s %6s  %s'%(r[0],r[1],'yes' if r[2] else '',  'yes' if r[3] else '', r[4]))
