"""Index SDK payload with bounded-memory hashing; excludes generated indexes."""
import hashlib,json,os,sys
from pathlib import Path
root=Path(sys.argv[1]).resolve()
entries=[]
for base,dirs,names in os.walk(root):
 dirs.sort()
 for name in sorted(names):
  p=Path(base)/name
  if p.parent==root and name in ('manifest.json','SHA256SUMS'): continue
  if p.is_symlink(): continue
  h=hashlib.sha256()
  try:
   with p.open('rb') as stream:
    for block in iter(lambda:stream.read(1024*1024),b''): h.update(block)
  except OSError as e: raise RuntimeError('Unreadable SDK file: '+str(p)) from e
  entries.append(dict(path=p.relative_to(root).as_posix(),bytes=p.stat().st_size,sha256=h.hexdigest()))
(root/'manifest.json').write_text(json.dumps(dict(schema=2,profile='kylin-x86',files=entries),indent=2)+'\n',encoding='utf-8')
(root/'SHA256SUMS').write_text(''.join(x['sha256']+'  '+x['path']+'\n' for x in entries),encoding='utf-8')
print('Indexed',len(entries),'files;',sum(x['bytes'] for x in entries),'bytes')
