#!/usr/bin/env python3
import argparse, hashlib, json, os
from pathlib import Path
p=argparse.ArgumentParser(); p.add_argument('root'); p.add_argument('--manifest',default='manifest.json'); a=p.parse_args(); root=Path(a.root)
items=[]
for f in sorted(x for x in root.rglob('*') if x.is_file() and x.name not in ('SHA256SUMS',)):
 h=hashlib.sha256();
 with f.open('rb') as x:
  for b in iter(lambda:x.read(1024*1024),b''): h.update(b)
 items.append({'path':str(f.relative_to(root)).replace('\\','/'),'bytes':f.stat().st_size,'sha256':h.hexdigest()})
(root/'SHA256SUMS').write_text('\n'.join(f"{i['sha256']}  {i['path']}" for i in items)+'\n',encoding='utf-8')
(root/a.manifest).write_text(json.dumps({'schema':1,'profile':root.name,'files':items},ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
print(f'generated {len(items)} entries')
