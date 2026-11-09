# -*- coding: utf-8 -*-
"""补齐引擎用到的字体。字体通常在补充包的 opt/fonts/video 里；开发环境缺字体时才下载。
  python get_fonts.py                缺哪个下哪个（下到 engine/fonts/）
  python get_fonts.py --check        只检查（含补充包与系统字体目录）
  python get_fonts.py --dest DIR     下载到指定目录（打包时用，如 <repo>/fonts/video）
全部是 SIL OFL 1.1 授权的开源字体，来自 github.com/google/fonts。
中文两款各 20MB 左右，所以仓库里不带字体。"""
import os, shutil, sys, urllib.request
from pathlib import Path

ENGINE = Path(__file__).resolve().parent
DEFAULT_DEST = ENGINE / "fonts"
BASE = "https://raw.githubusercontent.com/google/fonts/main/ofl/"
FONTS = {   # 本地文件名: 仓库路径
    "NotoSerifSC-VF.ttf": "notoserifsc/NotoSerifSC%5Bwght%5D.ttf",             # 宋体（思源宋体），可变字重 200–900
    "NotoSansSC-VF.ttf": "notosanssc/NotoSansSC%5Bwght%5D.ttf",                # 黑体（思源黑体），可变字重 100–900
    "CormorantGaramond.ttf": "cormorantgaramond/CormorantGaramond%5Bwght%5D.ttf",
    "CormorantGaramond-Italic.ttf": "cormorantgaramond/CormorantGaramond-Italic%5Bwght%5D.ttf",
    "Lora.ttf": "lora/Lora%5Bwght%5D.ttf",
    "Lora-Italic.ttf": "lora/Lora-Italic%5Bwght%5D.ttf",
    "JetBrainsMono.ttf": "jetbrainsmono/JetBrainsMono%5Bwght%5D.ttf",
    "CourierPrime-Regular.ttf": "courierprime/CourierPrime-Regular.ttf",
    "OFL-NotoSerifSC.txt": "notoserifsc/OFL.txt", "OFL-NotoSansSC.txt": "notosanssc/OFL.txt",
    "OFL-CormorantGaramond.txt": "cormorantgaramond/OFL.txt", "OFL-Lora.txt": "lora/OFL.txt",
    "OFL-JetBrainsMono.txt": "jetbrainsmono/OFL.txt", "OFL-CourierPrime.txt": "courierprime/OFL.txt",
}

def font_dirs():
    """和 render.py 一致的查找顺序：engine/fonts → 补充包 → 系统字体目录。"""
    dirs = [DEFAULT_DEST]
    home = os.environ.get("WUNDER_HOME", "").strip()
    if home:
        dirs.append(Path(home) / "opt/fonts/video")
    exe = Path(sys.executable).resolve()
    for parent in exe.parents:
        if parent.name == "opt":
            dirs.append(parent.parent / "opt/fonts/video")
            break
    if len(ENGINE.parents) > 3:
        dirs.append(ENGINE.parents[3] / "config/fonts/video")
        dirs.append(ENGINE.parents[3] / "config/fonts")
    dirs.append(Path("/usr/share/fonts/wunder/video"))
    dirs.append(Path("/usr/share/fonts/wunder"))
    out, seen = [], set()
    for d in dirs:
        try:
            d = d.resolve()
        except OSError:
            pass
        if d not in seen:
            seen.add(d); out.append(d)
    return out

def found(name):
    for d in font_dirs():
        p = d / name
        if p.is_file() and p.stat().st_size >= 1000:
            return p
    return None

if __name__ == "__main__":
    args = sys.argv[1:]
    dest = DEFAULT_DEST
    if "--dest" in args:
        i = args.index("--dest")
        if len(args) <= i + 1: sys.exit("--dest 需要一个目录")
        dest = Path(args[i + 1]).resolve()
    missing = [k for k in FONTS if not found(k)]
    if "--check" in args:
        print("字体齐全" if not missing else "缺：" + "、".join(missing)); sys.exit(1 if missing else 0)
    dest.mkdir(parents=True, exist_ok=True)
    for k in FONTS:
        target = dest / k
        if target.is_file() and target.stat().st_size >= 1000:
            continue
        src = found(k)
        if src and src != target:
            shutil.copyfile(src, target); print("复制", k, "←", src, flush=True)
            continue
        print("下载", k, "…", flush=True)
        try:
            with urllib.request.urlopen(BASE + FONTS[k], timeout=180) as r: target.write_bytes(r.read())
        except Exception as e:
            print("  失败：", e, "\n  可以手动从", BASE + FONTS[k], "下载后放到", target)
    print("完成。" if not [k for k in FONTS if not (dest / k).is_file()] else "还有没下到的，重跑一次或手动放进去。")
