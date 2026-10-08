#!/usr/bin/env python3
import argparse,hashlib,json,platform,subprocess,sys
from pathlib import Path
p=argparse.ArgumentParser(); p.add_argument('artifact'); p.add_argument('--target',required=True); p.add_argument('--output'); a=p.parse_args(); f=Path(a.artifact)
if not f.is_file(): raise SystemExit(f'missing artifact: {f}')
data=f.read_bytes(); h=hashlib.sha256(data).hexdigest(); out={'artifact':str(f),'bytes':len(data),'sha256':h,'target':a.target,'host':platform.platform()}
if a.target=='win7-x86' and data[:2]==b'MZ':
 peoff=int.from_bytes(data[60:64],'little'); machine=int.from_bytes(data[peoff+4:peoff+6],'little') if data[peoff:peoff+4]==b'PE\0\0' else None; out['pe_machine']=hex(machine) if machine else None
 if machine != 0x14c: raise SystemExit('PE is not i386 (0x14c)')
Path(a.output or str(f)+'.json').write_text(json.dumps(out,indent=2)+'\n',encoding='utf-8'); print(json.dumps(out,indent=2));
