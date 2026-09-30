#!/usr/bin/env python3
"""Validate an offline Rust-builder profile without changing the project."""
import argparse, json, os, platform, shutil, sys
from pathlib import Path

TARGETS={"win7-x86":("win7", "i686-win7-windows-gnu"),"linux-amd64-ubuntu18":("kylin-arm", "x86_64-unknown-linux-gnu"),"linux-arm64-ubuntu18":("kylin-arm", "aarch64-unknown-linux-gnu"),"kylin-x86":("kylin-x86", "x86_64-unknown-linux-gnu")}
def main():
 p=argparse.ArgumentParser(); p.add_argument('--target',choices=TARGETS,default='win7-x86'); p.add_argument('--sdk-root'); p.add_argument('--project',default='.')
 a=p.parse_args(); root=Path(a.sdk_root or os.getenv('RUST_BUILDER_ROOT','')).expanduser()
 if not root: root=Path(a.project).resolve().parent/'Rust-builder'
 profile,triple=TARGETS[a.target]; base=root/profile
 errors=[]
 if not base.is_dir(): errors.append(f'SDK profile missing: {base}')
 for rel in [('offline/rust','Rust toolchain'),('offline/cargo-vendor-slint','Cargo vendor')]:
  if base.joinpath(rel[0]).exists() is False and a.target!='kylin-x86': errors.append(f'{rel[1]} missing: {base/rel[0]}')
 print(json.dumps({'target':a.target,'triple':triple,'sdk_root':str(root),'profile':profile,'host':platform.machine(),'cargo':shutil.which('cargo'),'rustc':shutil.which('rustc'),'errors':errors},ensure_ascii=False,indent=2))
 return 1 if errors else 0
if __name__=='__main__': sys.exit(main())
