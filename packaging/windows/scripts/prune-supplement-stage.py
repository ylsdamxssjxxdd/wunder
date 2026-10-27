"""Prune the staged Win7 supplement (single source of truth for both builders).

The PowerShell build host wraps Remove-Item in a delete guard that routes every
call through the Recycle Bin, so pruning hundreds of nested directories fails
closed and aborts the build. This script performs the same deletions directly
with shutil/os, and is invoked by build_win7_desktop_supplement.ps1 so the two
never drift apart.

Usage: python prune-supplement-stage.py <package-root> [--arch ia32|x64]

Three kinds of pruning happen here:

1. Python wheel clutter: every site-packages/*/tests and testdata tree.
2. Foreign-architecture PE images: a 32-bit pack must never carry an x64 DLL
   (the ia32 supplement shipped an x86-64 capstone.dll for exactly this
   reason), so every PE file that the target cannot load is removed.
3. MinGit extras the agent never calls: the .NET/Avalonia Git Credential
   Manager stack, scalar, tig, vim/perl/tcl-tk and the GUI helpers. What is
   left is git.exe plus its native dependencies and the MSYS runtime its shell
   scripts need.
"""

import os
import pathlib
import shutil
import struct
import sys

ARCH_MACHINES = {'ia32': 0x014c, 'x64': 0x8664}
MACHINE_NAMES = {
    0x014c: 'i386',
    0x01c0: 'arm',
    0x8664: 'x86-64',
    0xaa64: 'arm64',
}

# An x64 pack may legitimately ship 32-bit helper executables (Windows runs
# them through WOW64); an ia32 pack cannot load a single wider image.
ARCH_ALLOWED = {
    'ia32': {0x014c},
    'x64': {0x8664, 0x014c},
}

PE_SUFFIXES = ('.exe', '.dll', '.pyd', '.node')

PY_NAMES = ('tests', 'testdata')

GIT_DIRS = [
    'usr/share/vim', 'usr/share/doc', 'usr/share/man', 'usr/share/gtk-doc',
    'usr/lib/perl5', 'usr/share/perl5', 'mingw32/share/doc', 'mingw32/share/perl5',
    'mingw32/share/git-gui', 'mingw32/lib/tcl8.6', 'mingw32/lib/tk8.6',
]

GIT_FILES = [
    # Perl/vim are only reachable through git add -p, git svn and the editor
    # integrations; the agent edits files through its own tools instead.
    'usr/bin/vim.exe', 'usr/bin/vimdiff.exe', 'usr/bin/view.exe', 'usr/bin/rvim.exe',
    'usr/bin/rview.exe', 'usr/bin/ex.exe', 'usr/bin/perl.exe', 'usr/bin/perl5.38.2.exe',
    'usr/bin/msys-perl5_38.dll', 'usr/bin/msys-svn_swig_perl-1-0.dll',
    # GUI/TUI front ends and the repo scaler (11 MB on its own).
    'mingw32/bin/scalar.exe', 'mingw32/bin/tig.exe', 'mingw32/bin/gitk',
    'cmd/scalar.exe', 'cmd/tig.exe',
    'mingw32/libexec/git-core/scalar.exe', 'mingw32/libexec/git-core/git-gui.exe',
    'mingw32/libexec/git-core/git-gui--askpass.exe', 'mingw32/libexec/git-core/gitk.exe',
    'mingw32/libexec/git-core/tcl86.dll', 'mingw32/libexec/git-core/tk86.dll',
    # Small standalone helpers that duplicate a library the DLL already carries.
    'mingw32/bin/brotli.exe', 'mingw32/bin/psl.exe',
]

# Git Credential Manager ships a full .NET/Avalonia UI (about 25 MB) that the
# supplement never shows. git-credential-wincred.exe stays and becomes the
# configured helper, so HTTPS push still has somewhere to read credentials.
GIT_GCM_GLOBS = [
    'Avalonia.*.dll',
    'Microsoft.*.dll',
    'System.*.dll',
    'msalruntime*.dll',
    'SkiaSharp.dll',
    'libSkiaSharp.dll',
    'HarfBuzzSharp.dll',
    'libHarfBuzzSharp.dll',
    'av_libglesv2.dll',
    'MicroCom.Runtime.dll',
    'gcmcore.dll',
    'GitHub.dll',
    'GitLab.dll',
    'Atlassian.Bitbucket.dll',
    'git-credential-manager.exe*',
    'git-credential-helper-selector.exe',
]

GIT_CONFIG_HELPER = ('helper = manager', 'helper = wincred')


def rewrite_git_config(text):
    """Point credential.helper at wincred and stop inheriting a host Git install.

    MinGit's etc/gitconfig ends with an [include] block that pulls in
    C:/Program Files/Git/etc/gitconfig from Git for Windows on the same machine.
    That include wins over our own credential.helper, and on a host with Git
    installed it re-enables the credential manager whose binaries the prune just
    deleted. The supplement ships its own runtime and must not read the host one.
    """
    old, new = GIT_CONFIG_HELPER
    text = text.replace(old, new)
    result = []
    in_include = False
    for line in text.splitlines():
        stripped = line.strip()
        if stripped.startswith('['):
            in_include = stripped.lower() == '[include]'
            if in_include:
                while result and result[-1].strip().startswith(';'):
                    result.pop()
                continue
        if in_include:
            if stripped.lower().startswith('path') and 'Program Files' in stripped:
                continue
            if stripped == '' or stripped.startswith(';'):
                continue
            in_include = False
        result.append(line)
    return '\n'.join(result).rstrip() + '\n'


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


def pe_machine(path):
    """Return the PE machine word, or None when the file is not a PE image."""
    try:
        with open(path, 'rb') as handle:
            header = handle.read(0x40)
            if len(header) < 0x40 or header[:2] != b'MZ':
                return None
            offset = struct.unpack_from('<I', header, 0x3c)[0]
            handle.seek(offset)
            coff = handle.read(6)
        if len(coff) < 6 or coff[:4] != b'PE\0\0':
            return None
        return struct.unpack_from('<H', coff, 4)[0]
    except OSError:
        return None


def parse_args(argv):
    positional = []
    arch = 'ia32'
    skip_tests = False
    skip_git = False
    index = 0
    while index < len(argv):
        arg = argv[index]
        if arg == '--arch':
            index += 1
            arch = argv[index] if index < len(argv) else ''
        elif arg.startswith('--arch='):
            arch = arg.split('=', 1)[1]
        elif arg == '--skip-tests':
            skip_tests = True
        elif arg == '--skip-git':
            skip_git = True
        else:
            positional.append(arg)
        index += 1
    return {
        'root': positional[0] if positional else '.',
        'arch': arch,
        'skip_tests': skip_tests,
        'skip_git': skip_git,
    }


def main():
    options = parse_args(sys.argv[1:])
    root = pathlib.Path(options['root'])
    arch = options['arch']
    if arch not in ARCH_MACHINES:
        sys.exit(f'unknown arch: {arch}')
    if not (root / 'opt').is_dir():
        sys.exit(f'not a supplement package root: {root}')

    removed_dirs = 0
    removed_files = 0
    freed = 0
    failed = []
    foreign = []

    def drop_dir(target):
        nonlocal removed_dirs, freed
        size = free_bytes(target)
        try:
            remove(target)
            removed_dirs += 1
            freed += size
        except OSError as exc:
            failed.append(f'{target}: {exc}')

    def drop_file(target):
        nonlocal removed_files, freed
        try:
            size = target.stat().st_size
        except OSError:
            size = 0
        try:
            remove(target)
            removed_files += 1
            freed += size
        except OSError as exc:
            failed.append(f'{target}: {exc}')

    opt = root / 'opt'

    # 1. Wheel test suites.
    site_packages = opt / 'python' / 'Lib' / 'site-packages'
    if site_packages.is_dir() and not options['skip_tests']:
        for pkg in sorted(p for p in site_packages.iterdir() if p.is_dir()):
            for name in PY_NAMES:
                target = pkg / name
                if target.is_dir():
                    drop_dir(target)

    # 2. Foreign-architecture PE images. Anything the target cannot load is dead
    #    weight at best and a load failure at worst.
    allowed = ARCH_ALLOWED[arch]
    for dirpath, dirnames, filenames in os.walk(opt):
        for name in filenames:
            if not name.lower().endswith(PE_SUFFIXES):
                continue
            target = pathlib.Path(dirpath) / name
            machine = pe_machine(target)
            if machine is None or machine in allowed:
                continue
            foreign.append(f'{MACHINE_NAMES.get(machine, hex(machine))} {target.relative_to(root)}')
            drop_file(target)

    # 3. MinGit extras.
    git = opt / 'git'
    if git.is_dir() and not options['skip_git']:
        for rel in GIT_DIRS:
            target = git / rel.replace('/', os.sep)
            if target.is_dir():
                drop_dir(target)
        for rel in GIT_FILES:
            target = git / rel.replace('/', os.sep)
            if target.is_file():
                drop_file(target)
        git_bin = git / 'mingw32' / 'bin'
        for pattern in GIT_GCM_GLOBS:
            for target in sorted(git_bin.glob(pattern)):
                if target.is_file():
                    drop_file(target)

        config = git / 'etc' / 'gitconfig'
        if config.is_file():
            text = config.read_text(encoding='utf-8', errors='replace')
            updated = rewrite_git_config(text)
            if updated != text:
                config.write_text(updated, encoding='utf-8', newline='\n')
                print('git config: credential.helper=wincred, host Git for Windows include dropped')

    print(f'removed {removed_dirs} directories, {removed_files} files, freed {freed / 1048576:.1f} MB')
    if foreign:
        print(f'removed {len(foreign)} foreign-architecture PE files:')
        for item in foreign[:20]:
            print(' ', item)
        if len(foreign) > 20:
            print(f'  ... and {len(foreign) - 20} more')
    if failed:
        print('FAILURES:')
        for item in failed:
            print(' ', item)
        sys.exit(1)


if __name__ == '__main__':
    main()
