"""Prune the staged Win7 supplement exactly like build_win7_desktop_supplement.ps1.

The PowerShell build host wraps Remove-Item in a delete guard that routes every
call through the Recycle Bin, so pruning hundreds of nested directories fails
closed and aborts the build. This script performs the same deletions directly.
"""

import os
import pathlib
import shutil
import sys

root = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else '.')
if not (root / 'opt').is_dir():
    sys.exit(f'not a supplement package root: {root}')

PY_NAMES = ('tests', 'testdata')
GIT_DIRS = [
    'usr/share/vim', 'usr/share/doc', 'usr/share/man', 'usr/share/gtk-doc',
    'usr/lib/perl5', 'usr/share/perl5', 'mingw32/share/doc', 'mingw32/share/perl5',
    'mingw32/share/git-gui', 'mingw32/lib/tcl8.6', 'mingw32/lib/tk8.6',
]
GIT_FILES = [
    'usr/bin/vim.exe', 'usr/bin/vimdiff.exe', 'usr/bin/view.exe', 'usr/bin/rvim.exe',
    'usr/bin/rview.exe', 'usr/bin/ex.exe', 'usr/bin/perl.exe', 'usr/bin/perl5.38.2.exe',
    'usr/bin/msys-perl5_38.dll', 'usr/bin/msys-svn_swig_perl-1-0.dll',
    'mingw32/bin/scalar.exe', 'mingw32/bin/gitk',
    'mingw32/libexec/git-core/scalar.exe', 'mingw32/libexec/git-core/git-gui.exe',
    'mingw32/libexec/git-core/git-gui--askpass.exe', 'mingw32/libexec/git-core/gitk.exe',
    'mingw32/libexec/git-core/tcl86.dll', 'mingw32/libexec/git-core/tk86.dll',
]


def remove(path):
    """Delete a file or directory tree, clearing read-only bits as needed."""
    if path.is_dir() and not path.is_symlink():
        for dirpath, dirnames, filenames in os.walk(path):
            for name in dirnames + filenames:
                try:
                    os.chmod(os.path.join(dirpath, name), 0o700)
                except OSError:
                    pass
        shutil.rmtree(path)
    else:
        try:
            path.unlink()
        except PermissionError:
            os.chmod(path, 0o700)
            path.unlink()


def free_bytes(path):
    total = 0
    for dirpath, dirnames, filenames in os.walk(path):
        for name in filenames:
            try:
                total += os.path.getsize(os.path.join(dirpath, name))
            except OSError:
                pass
    return total


removed_dirs = 0
removed_files = 0
freed = 0
failed = []

site_packages = root / 'opt' / 'python' / 'Lib' / 'site-packages'
if site_packages.is_dir():
    for pkg in sorted(p for p in site_packages.iterdir() if p.is_dir()):
        for name in PY_NAMES:
            target = pkg / name
            if target.is_dir():
                size = free_bytes(target)
                try:
                    remove(target)
                    removed_dirs += 1
                    freed += size
                except OSError as exc:
                    failed.append(f'{target}: {exc}')

git = root / 'opt' / 'git'
if git.is_dir():
    for rel in GIT_DIRS:
        target = git / rel.replace('/', os.sep)
        if target.is_dir():
            size = free_bytes(target)
            try:
                remove(target)
                removed_dirs += 1
                freed += size
            except OSError as exc:
                failed.append(f'{target}: {exc}')
    for rel in GIT_FILES:
        target = git / rel.replace('/', os.sep)
        if target.is_file():
            size = target.stat().st_size
            try:
                remove(target)
                removed_files += 1
                freed += size
            except OSError as exc:
                failed.append(f'{target}: {exc}')

print(f'removed {removed_dirs} directories, {removed_files} files, freed {freed / 1048576:.1f} MB')
if failed:
    print('FAILURES:')
    for item in failed:
        print(' ', item)
    sys.exit(1)
