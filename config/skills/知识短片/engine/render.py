# -*- coding: utf-8 -*-
"""逐帧 seek → 截图 → 管道进 ffmpeg。画面是 t 的纯函数，所以可以只渲一段、只截几帧、乱序截。

  python render.py <工程>                       全片 → <工程>/renders/raw.mp4（无声）
  python render.py <工程> --jobs 4              全片，按镜头切段、最多 4 路并行渲再无损拼起来（长片用这个）
  python render.py <工程> --only 3              只重渲第 3 镜（也接受镜名 / 时间区间），再拼全部已完成分段
  python render.py <工程> --parts 0:12 12:30    不用 SHOTS 时手动指定分段边界
  python render.py <工程> --concat              把 renders/_part*.mp4 拼成 raw.mp4（默认保留分段）
  python render.py <工程> --clean-parts         拼接成功后删掉分段（默认保留，方便下次只重渲脏镜头）
  python render.py <工程> --web                 发布转码 → renders/web.mp4（1080p30、+faststart；打印体积/码率）
  python render.py <工程> --stills 2.5 8.5 21   只截几帧 → <工程>/review/s_0002.50.png
  python render.py <工程> --stills 8 --mark 船  截帧并画元素包围盒 → review/m_*.png
  python render.py <工程> --sheet 16            全片均匀取 16 帧拼一张 → review/sheet.jpg（开工后先看这个，再渲染）
  python render.py <工程> --sheet 3.5 9 21.5    指定时刻拼一张
  python render.py <工程> --sheet 16 --from renders/final.mp4  从成片抽帧拼图（看编码后 banding / 音画对齐）
  python render.py <工程> --motion 8.2 10.6     一段时间里均匀取 12 帧拼一张 → review/motion_8.2-10.6.jpg（看转场、看动作）
  python render.py <工程> --strip 8.2 9.2 24    连续帧条带 → review/strip_*.jpg（看运动顺不顺）
  python render.py <工程> --clip 0 12 [名字]    只渲一段 → renders/<名字或_clip>.mp4（成功写 .done）
  python render.py <工程> --clips 0:12 20:30    一次渲多段并自动拼 → renders/clips.mp4
  python render.py <工程> --preview 8.2 10.6    低码率预览片段 → renders/preview_*.mp4
  python render.py <工程> --events              导出音效事件表 → audio/events.json（给 audio.py）
  python render.py <工程> --det                 确定性自检：同一批时刻乱序再截一遍，逐字节比
  python render.py <工程> --lint                静态+运行时自检：END/FPS、字体、音效、meta、分镜表、Math.random 等
  python render.py <工程> --storyboard          核对 storyboard.md 的时间列与 film.js 的 SHOTS 是否逐镜对得上
  python render.py <工程> --check-safe [间隔]   安全区检查：包围盒 + 像素（隐藏字幕后看 y>840）
  python render.py <工程> --check-band [间隔]   只做像素检查，手写 ctx / 柔边渐变也能查到
  python render.py <工程> --overlap [间隔]      遮挡检查：按实际相交面积报告，可用 fr.mark(..., {allow:'名字'}) 放行
  python render.py --diff A.mp4 B.mp4           两版逐帧差分 → A_diff.jpg + 时刻表
  python render.py <工程> --serve               起本地预览 http://127.0.0.1:8765/?play（8765 被占用时自动换端口）

工程目录里只需要 index.html + film.js。引擎（vk.js、vk-kit.js）留在本文件所在目录，
中文优先用补充包 opt/fonts/video 的思源宋体/黑体，缺失时退到 opt/fonts/common 的宋体/雅黑；
拉丁与等宽字体也在 opt/fonts/video（桌面/安卓）或 /usr/share/fonts/wunder（云端），
没有时退回 engine/fonts 或跑 get_fonts.py 下载。都由内置小服务器挂到 /engine/ 下，
不用往每个工程里拷字体，也没有 file:// 的跨目录字体限制。

镜头表：film.js 里 VK.film({ shots: [{ no, name, from, to }, …] }) 声明后，--jobs 按镜头切段、
--storyboard 用同一张表核对；分镜之间的空档会自动补成「空档」段，保证拼接总时长不变。
安全区/遮挡/包围盒依赖 vk.js 的 recordFrame：给装置用 fr.mark('船', () => { … }) 命名，
字幕由 K.subs 自动标成「字幕」。像素检查会临时隐藏字幕，把整帧与「底 + look.under + 后期」的纯背景帧相减：
背景（包括 B 的整幅插画）两边都有，不算越界；手写 ctx 画的东西只出现在整帧里，会被查出来。
flat（B 寓言）整幅场景都在前景层，默认跳过像素检查，装置请用 fr.mark 命名后靠包围盒查。
"""
import hashlib, http.server, io, json, math, os, random, re, subprocess, sys, tempfile, threading, time
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

ENGINE = Path(__file__).resolve().parent
REQUIRED_FONTS = [
    "NotoSerifSC-VF.ttf", "NotoSansSC-VF.ttf",
    "CormorantGaramond.ttf", "CormorantGaramond-Italic.ttf",
    "Lora.ttf", "Lora-Italic.ttf", "JetBrainsMono.ttf", "CourierPrime-Regular.ttf",
]
SAFE_Y = 840
KNOWN_SFX = {"tick", "soft", "chord", "bloom", "rise", "thud", "tock", "stamp", "shimmer", "hush"}
SUB_RE = re.compile(r"字幕|sub(?:title)?|caption", re.I)

def ffmpeg_bin():
    return os.environ.get("WUNDER_FFMPEG_BIN", "").strip() or "ffmpeg"

def ffprobe_bin():
    # 补充包只带 ffmpeg、不带 ffprobe；有就用，没有就退回 ffmpeg -i 解析。
    return os.environ.get("WUNDER_FFPROBE_BIN", "").strip() or "ffprobe"

def ffmpeg_run(args, what):
    """跑 ffmpeg 并按 UTF-8 解 stderr：Windows 中文控制台默认 GBK，直接 text=True 会炸。"""
    try:
        r = subprocess.run([ffmpeg_bin()] + args, capture_output=True,
                           encoding="utf-8", errors="replace")
    except FileNotFoundError:
        sys.exit(f"找不到 ffmpeg（{ffmpeg_bin()}）：请确认补充包已安装，或把 ffmpeg 放进 PATH")
    if r.returncode != 0:
        detail = (r.stderr or "").strip() or "ffmpeg 没有输出错误信息"
        sys.exit(f"{what}失败（退出码 {r.returncode}）：\n{detail}")
    return r

def font_dirs():
    """字体查找顺序：本地 engine/fonts → 补充包 opt/fonts/video、opt/fonts/common → 系统字体目录。"""
    dirs = [ENGINE / "fonts"]
    home = os.environ.get("WUNDER_HOME", "").strip()
    if home:
        dirs.append(Path(home) / "opt/fonts/video")
        dirs.append(Path(home) / "opt/fonts/common")
    exe = Path(sys.executable).resolve()
    for parent in exe.parents:
        if parent.name == "opt":
            dirs.append(parent.parent / "opt/fonts/video")
            dirs.append(parent.parent / "opt/fonts/common")
            break
    if len(ENGINE.parents) > 3:
        dirs.append(ENGINE.parents[3] / "config/fonts/video")
        dirs.append(ENGINE.parents[3] / "config/fonts/common")
        dirs.append(ENGINE.parents[3] / "config/fonts")
    dirs.append(Path("/usr/share/fonts/wunder/video"))
    dirs.append(Path("/usr/share/fonts/wunder/common"))
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

def font_file(name):
    for d in font_dirs():
        p = d / name
        if p.is_file():
            return p
    return None

def warn_missing_fonts():
    missing = [n for n in REQUIRED_FONTS if not font_file(n)]
    if missing:
        print("提示：缺少视频字体：" + "、".join(missing) +
              "\n  补充包应提供 opt/fonts/video；开发环境可运行 python engine/get_fonts.py", file=sys.stderr)

def apply_fontconfig_env():
    """让 Chrome 通过 fontconfig 看到补充包 opt/fonts/common 里的宋体/雅黑。"""
    if os.environ.get("FONTCONFIG_FILE"):
        return
    home = os.environ.get("WUNDER_HOME", "").strip()
    if not home:
        return
    conf = Path(home) / "opt/fonts/fonts.conf"
    if conf.is_file():
        os.environ["FONTCONFIG_FILE"] = str(conf)

def playwright_chromium():
    """Playwright 自带的 Chromium（云端镜像/补充包），兼容 chrome-linux64 新布局。"""
    exe_dir = Path(sys.executable).resolve().parent
    bases = [exe_dir / "ms-playwright", exe_dir.parent / "ms-playwright",
             exe_dir.parent.parent / "ms-playwright",
             Path(sys.prefix) / "ms-playwright", Path(sys.prefix).parent / "ms-playwright",
             Path("/opt/ms-playwright")]
    env = os.environ.get("PLAYWRIGHT_BROWSERS_PATH", "").strip()
    if env and env != "0":
        bases.insert(0, Path(env))
    home = os.environ.get("HOME", "").strip()
    if home:
        bases.append(Path(home) / ".cache/ms-playwright")
    patterns = ("chrome-linux64/chrome", "chrome-linux-arm64/chrome", "chrome-linux/chrome",
                "chrome-win64/chrome.exe", "chrome-win/chrome.exe",
                "chrome-mac*/Chromium.app/Contents/MacOS/Chromium")
    for base in bases:
        if not base.is_dir():
            continue
        for pattern in patterns:
            found = [p for p in base.glob(f"chromium-*/{pattern}") if p.is_file()]
            if found:
                def revision(path):
                    m = re.search(r"chromium-(\d+)", str(path))
                    return int(m.group(1)) if m else 0
                return max(found, key=revision)
    return None

def chrome_executable():
    """优先用补充包里的 Chrome；其次用 Playwright 自带 Chromium。"""
    env = os.environ.get("WUNDER_CHROME_PATH", "").strip()
    if env and Path(env).is_file():
        return Path(env)
    roots = []
    home = os.environ.get("WUNDER_HOME", "").strip()
    if home:
        roots.append(Path(home))
    exe = Path(sys.executable).resolve()
    for parent in exe.parents:
        if parent.name == "opt":
            roots.append(parent.parent)
            break
    rels = [
        "opt/browser/chrome-linux64/chrome", "opt/browser/chrome-linux-arm64/chrome",
        "opt/browser/chrome-linux/chrome",
        "opt/browser/chrome-win64/chrome.exe", "opt/browser/chrome-win/chrome.exe",
        "opt/browser/chrome.exe",
        "opt/browser/chrome-mac/Chromium.app/Contents/MacOS/Chromium",
    ]
    for root in roots:
        for rel in rels:
            p = root / rel
            if p.is_file():
                return p
    # 安卓 proot 的浏览器包固定在 rootfs 的 /opt/browser。
    for abs_path in (Path("/opt/browser/chrome-linux64/chrome"),
                     Path("/opt/browser/chrome-linux-arm64/chrome"),
                     Path("/opt/browser/chrome-linux/chrome"), Path("/opt/browser/chrome")):
        if abs_path.is_file():
            return abs_path
    return playwright_chromium()

class Handler(http.server.SimpleHTTPRequestHandler):
    proj = None
    def translate_path(self, path):
        p = path.split("?", 1)[0].split("#", 1)[0]
        if p.startswith("/engine/fonts/"):
            f = font_file(p[len("/engine/fonts/"):])
            if f: return str(f)
        if p.startswith("/engine/"): return str(ENGINE / p[len("/engine/"):])
        return str(self.proj / p.lstrip("/")) if p != "/" else str(self.proj / "index.html")
    def do_GET(self):
        if self.path.startswith("/favicon"): self.send_response(204); self.end_headers(); return
        super().do_GET()
    def end_headers(self):
        self.send_header("Cache-Control", "no-store"); super().end_headers()
    def log_message(self, *a): pass

def serve(proj, port=0):
    h = type("H", (Handler,), {"proj": proj})
    srv = http.server.ThreadingHTTPServer(("127.0.0.1", port), h)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    return srv, f"http://127.0.0.1:{srv.server_address[1]}/"

def open_page(p, url):
    # 关掉 LCD 次像素抗锯齿（跳着 seek 字边不抖）；每帧等合成器跑完再截；固定 sRGB
    args = ["--disable-lcd-text", "--run-all-compositor-stages-before-draw", "--force-color-profile=srgb"]
    exe = chrome_executable()
    # 安卓 proot 内的浏览器包与常见容器一样没有可用的沙箱，必须显式关掉。
    if exe and str(exe).startswith("/opt/browser/"):
        args += ["--no-sandbox", "--no-zygote", "--disable-dev-shm-usage", "--disable-gpu"]
    if exe:
        try:
            b = p.chromium.launch(executable_path=str(exe), args=args)
        except Exception as e:
            print(f"[chrome] 内置浏览器启动失败（{exe}）：{e}", file=sys.stderr)
            b = None
    else:
        b = None
    if b is None:
        try:
            b = p.chromium.launch(args=args)
        except Exception as e:
            print(f"[chrome] Playwright 自带 Chromium 启动失败：{e}", file=sys.stderr)
            b = p.chromium.launch(channel="chrome", args=args)
    pg = b.new_page(viewport={"width": 1920, "height": 1080}, device_scale_factor=1)
    pg.on("console", lambda m: m.type in ("error", "warning") and print("[page]", m.text))
    pg.on("pageerror", lambda e: print("[pageerror]", e))
    pg.goto(url)
    pg.wait_for_function("window.READY === true", timeout=180000)
    return b, pg

def shot(pg, t):
    pg.evaluate(f"seek({t})")
    return pg.screenshot(type="png")

def with_page(proj, fn):
    from playwright.sync_api import sync_playwright
    srv, url = serve(proj)
    try:
        with sync_playwright() as p:
            b, pg = open_page(p, url)
            try: return fn(pg)
            finally: b.close()
    finally:
        srv.shutdown()

# ---------- 命令行 ----------
def flag_args(a, name):
    """取 --flag 后面直到下一个 --flag 的值。"""
    if name not in a: return []
    i = a.index(name) + 1; out = []
    while i < len(a) and not a[i].startswith("--"):
        out.append(a[i]); i += 1
    return out

def flag_value(a, name, default=None):
    vals = flag_args(a, name)
    return vals[0] if vals else default

def as_float(text, label):
    try: return float(text)
    except (TypeError, ValueError): sys.exit(f"{label} 需要数字，收到：{text!r}")

def as_int(text, label, default):
    if text is None: return default
    try: return int(float(text))
    except (TypeError, ValueError): sys.exit(f"{label} 需要整数，收到：{text!r}")

# ---------- 进度与视频信息 ----------
class Progress:
    def __init__(self, total, label="render"):
        self.total = max(1, int(total)); self.n = 0; self.t0 = time.time(); self.last = 0.0
        self.label = label; self.tty = sys.stdout.isatty()
    def step(self, n):
        self.n = n
        now = time.time()
        if n < self.total and now - self.last < (0.25 if self.tty else 10.0):
            return
        self.last = now
        elapsed = now - self.t0
        rate = n / elapsed if elapsed > 0 else 0
        eta = (self.total - n) / rate if rate > 0 else 0
        msg = f"  {self.label} {n}/{self.total} ({n * 100.0 / self.total:5.1f}%)  已用 {elapsed:.0f}s  剩余 ~{eta:.0f}s"
        if self.tty:
            sys.stdout.write("\r" + msg); sys.stdout.flush()
        else:
            print(msg, flush=True)
    def close(self):
        if self.tty:
            sys.stdout.write("\n"); sys.stdout.flush()

def probe_video(path):
    path = Path(path)
    info = {"size": path.stat().st_size if path.exists() else 0}
    try:
        r = subprocess.run(
            [ffprobe_bin(), "-v", "error", "-show_entries", "format=duration,bit_rate",
             "-show_entries", "stream=width,height,r_frame_rate", "-of", "json", str(path)],
            capture_output=True, encoding="utf-8", errors="replace", timeout=30,
        )
        if r.returncode == 0 and r.stdout.strip():
            data = json.loads(r.stdout)
            fmt = data.get("format") or {}
            if fmt.get("duration"): info["duration"] = float(fmt["duration"])
            if fmt.get("bit_rate"): info["bit_rate"] = float(fmt["bit_rate"])
            for stream in data.get("streams") or []:
                if stream.get("width"): info["width"] = int(stream["width"])
                if stream.get("height"): info["height"] = int(stream["height"])
                if stream.get("r_frame_rate") and "/" in stream["r_frame_rate"]:
                    num, den = stream["r_frame_rate"].split("/", 1)
                    if float(den or 0) > 0: info["fps"] = float(num) / float(den)
    except Exception:
        pass
    if "duration" not in info:
        # 补充包只带 ffmpeg、不带 ffprobe 时，从 ffmpeg -i 的 stderr 里取时长/分辨率。
        try:
            r = subprocess.run([ffmpeg_bin(), "-hide_banner", "-i", str(path)],
                               capture_output=True, encoding="utf-8", errors="replace",
                               stdin=subprocess.DEVNULL, timeout=30)
            text = r.stderr or ""
            m = re.search(r"Duration:\s*(\d+):(\d+):([\d.]+)", text)
            if m:
                info["duration"] = int(m.group(1)) * 3600 + int(m.group(2)) * 60 + float(m.group(3))
            m = re.search(r"\b(\d{2,5})x(\d{2,5})\b", text)
            if m:
                info["width"], info["height"] = int(m.group(1)), int(m.group(2))
            m = re.search(r"([\d.]+)\s+fps", text)
            if m and float(m.group(1)) > 0:
                info["fps"] = float(m.group(1))
        except Exception:
            pass
    if "duration" not in info and info["size"] > 0:
        print(f"提示：读不到 {path.name} 的时长（ffprobe 与 ffmpeg -i 都未成功）；"
              f"不影响出片，排查可运行：ffmpeg -i \"{path}\"", file=sys.stderr)
    return info

def print_video(path):
    path = Path(path); info = probe_video(path)
    size_mb = info["size"] / 1048576.0
    dur = info.get("duration")
    bit = info.get("bit_rate") or (info["size"] * 8.0 / dur if dur else None)
    parts = [f"{path.name}: {size_mb:.1f} MB"]
    if dur: parts.append(f"{dur:.1f}s")
    if bit: parts.append(f"{bit / 1e6:.2f} Mbps")
    if info.get("width") and info.get("height"): parts.append(f"{info['width']}x{info['height']}")
    if info.get("fps"): parts.append(f"{info['fps']:.0f}fps")
    print("  " + "  ".join(parts))

def encode_args(crf, preset, preview=False):
    if preview:
        return ["-c:v", "libx264", "-preset", "veryfast", "-crf", "30", "-pix_fmt", "yuv420p",
                "-vf", "scale=960:-2:flags=lanczos", "-an", "-movflags", "+faststart"]
    return ["-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", f"{crf:g}", "-preset", preset,
            "-movflags", "+faststart"]

# ---------- 源码指纹：续渲与音效事件表都用它判断“工程有没有变” ----------
def project_sources(proj):
    return sorted([p for p in proj.iterdir() if p.is_file() and p.suffix.lower() in (".js", ".html")])

def project_hash(proj):
    h = hashlib.sha1()
    for p in project_sources(proj):
        h.update(p.name.encode("utf-8")); h.update(b"\0"); h.update(p.read_bytes()); h.update(b"\0")
    return h.hexdigest()

def done_marker(part):
    return part.with_suffix(".done")

def marker_info(part):
    try: text = done_marker(part).read_text(encoding="utf-8").strip()
    except OSError: return {}
    try:
        data = json.loads(text)
        return data if isinstance(data, dict) else {"hash": text}
    except json.JSONDecodeError:
        return {"hash": text}

def marker_range_ok(part, entry):
    info = marker_info(part)
    if info.get("from") is None or info.get("to") is None: return True
    return abs(float(info["from"]) - entry["from"]) <= .01 and abs(float(info["to"]) - entry["to"]) <= .01

def marker_matches(part, digest, entry):
    info = marker_info(part)
    if info.get("hash") != digest: return False
    return marker_range_ok(part, entry)

def clear_manifest(proj):
    (proj / "renders/_parts.json").unlink(missing_ok=True)

def write_manifest(proj, entries, digest):
    data = {"source_hash": digest,
            "parts": [{"file": e["file"], "from": round(e["from"], 3), "to": round(e["to"], 3),
                       "name": e["name"], "no": e["no"]} for e in entries]}
    (proj / "renders/_parts.json").write_text(json.dumps(data, ensure_ascii=False, indent=2), encoding="utf-8")

def read_manifest(proj):
    path = proj / "renders/_parts.json"
    if not path.is_file(): return None
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
        return data if isinstance(data, dict) and isinstance(data.get("parts"), list) else None
    except (OSError, json.JSONDecodeError):
        return None

def fill_gaps(items, end):
    """镜头/分段之间有空档时补成独立段，保证拼完的时长和 END 一致（不然音频和字幕会整体前移）。"""
    items = sorted(items, key=lambda x: x["from"])
    for item in items:
        item["from"] = max(0.0, min(item["from"], end))
        item["to"] = max(item["from"], min(item["to"], end))
    out, cur = [], 0.0
    for item in items:
        if item["from"] > cur + .001:
            out.append({"from": cur, "to": item["from"], "name": "空档", "no": None, "gap": True})
        if item["from"] < cur - .001:
            prev = out[-1]
            prev["to"] = max(prev["to"], item["to"])
            if not prev.get("gap"): prev["name"] += " + " + item["name"]
        else:
            out.append(item)
        cur = max(cur, out[-1]["to"])
    if cur < end - .001:
        out.append({"from": cur, "to": end, "name": "空档", "no": None, "gap": True})
    return out

def shot_ranges(shots, end):
    items = []
    for i, shot in enumerate(shots or []):
        try:
            a, b = float(shot.get("from")), float(shot.get("to"))
        except (TypeError, ValueError, AttributeError):
            continue
        a, b = max(0.0, min(a, end)), max(0.0, min(b, end))
        if b - a <= .001: continue
        items.append({"from": a, "to": b, "name": shot.get("name") or f"镜{i + 1}",
                      "no": shot.get("no", i + 1)})
    return fill_gaps(items, end)

def equal_ranges(end, fps, jobs):
    total = round(end * fps); cuts = [round(total * i / jobs) for i in range(jobs + 1)]
    return [{"from": cuts[i] / fps, "to": cuts[i + 1] / fps, "name": f"part{i}", "no": i + 1}
            for i in range(jobs) if cuts[i + 1] > cuts[i]]

def part_entries(proj, ranges):
    entries = []
    for i, item in enumerate(ranges):
        name = f"_part{i:02d}.mp4"
        entries.append(dict(item, index=i, file=name, path=proj / "renders" / name))
    return entries

def select_entries(entries, tokens):
    picked = set()
    for token in tokens:
        token = (token or "").strip()
        if not token: continue
        if re.fullmatch(r"\d+", token):
            n = int(token)
            hit = [e for e in entries if str(e.get("no")) == token]
            if not hit: hit = [e for e in entries if e["index"] + 1 == n]
        elif re.fullmatch(r"[\d.]+[:–—-][\d.]+", token):
            a, b = parse_clip_specs([token])[0]
            hit = [e for e in entries if e["from"] < b and e["to"] > a]
        else:
            hit = [e for e in entries if token == e.get("name") or token in (e.get("name") or "")]
        if not hit:
            labels = "、".join(f'{e["index"] + 1}{"/" + str(e.get("no")) if e.get("no") is not None else ""}·{e["name"]}' for e in entries)
            sys.exit(f"--only 没有匹配 {token!r}；可用：{labels}")
        picked.update(e["index"] for e in hit)
    return picked

def project_meta(proj):
    def go(pg):
        return {"end": pg.evaluate("END"), "fps": pg.evaluate("FPS"),
                "shots": pg.evaluate("window.SHOTS || null"),
                "subs": pg.evaluate("window.SUBS || null")}
    return with_page(proj, go)

# ---------- 静帧与拼图 ----------
def stills(proj, ts, marks=None):
    from PIL import Image, ImageDraw, ImageFont
    (proj / "review").mkdir(exist_ok=True)
    def go(pg):
        font = None
        if marks is not None:
            try: font = ImageFont.truetype(str(font_file("JetBrainsMono.ttf") or (ENGINE / "fonts/JetBrainsMono.ttf")), 22)
            except Exception: font = ImageFont.load_default()
        for t in ts:
            boxes = pg.evaluate("(t) => recordFrame(t)", t) if marks is not None else []
            png = shot(pg, t)
            orig = proj / f"review/s_{t:07.2f}.png"; orig.write_bytes(png)
            if marks is None:
                continue
            wanted = [x.strip() for m in marks if m and m != "all" for x in m.split(",") if x.strip()]
            picked = []
            for item in boxes:
                name = item.get("name") or ""
                if not name: continue
                if not wanted or any(w == name or w in name for w in wanted): picked.append(item)
            if not picked:
                print(f"  t={t:.2f}s 没有匹配的命名元素；用 fr.mark('名字', …) 包住装置")
                continue
            im = Image.open(io.BytesIO(png)).convert("RGB"); d = ImageDraw.Draw(im)
            for item in picked:
                x0, y0, x1, y1 = item["box"]
                d.rectangle([x0, y0, x1, y1], outline=(255, 90, 90), width=3)
                label = item.get("name") or item.get("kind") or "?"
                if font: d.text((x0 + 4, max(0, y0 - 26)), label, font=font, fill=(255, 90, 90))
            out = proj / f"review/m_{t:07.2f}.png"; im.save(out)
            print(f"  {out}  " + "、".join(f"{b['name']}{b['box']}" for b in picked))
    with_page(proj, go)
    print("stills:", len(ts))

def sheet(proj, args, name="sheet"):
    from PIL import Image, ImageDraw, ImageFont
    (proj / "review").mkdir(exist_ok=True)
    def go(pg):
        end = pg.evaluate("END")
        ts = [float(x) for x in args] if len(args) > 1 else [round((i + .5) * end / int(args[0] if args else 16), 2) for i in range(int(args[0] if args else 16))]
        cols = 4 if len(ts) > 6 else 2 if len(ts) > 1 else 1; tw = 1920 // cols; th = tw * 9 // 16
        out = Image.new("RGB", (cols * tw, -(-len(ts) // cols) * th), (20, 20, 20)); d = ImageDraw.Draw(out)
        try: font = ImageFont.truetype(str(font_file("JetBrainsMono.ttf") or (ENGINE / "fonts/JetBrainsMono.ttf")), 18)
        except Exception: font = ImageFont.load_default()
        for i, t in enumerate(ts):
            im = Image.open(io.BytesIO(shot(pg, t))).convert("RGB").resize((tw, th), Image.LANCZOS)
            x, y = (i % cols) * tw, (i // cols) * th; out.paste(im, (x, y))
            d.rectangle([x, y + th - 24, x + 78, y + th], fill=(0, 0, 0)); d.text((x + 5, y + th - 22), f"{t:.2f}", font=font, fill=(255, 230, 0))   # 时间标在左下角，不挡左上的年份 / 章节标
        out.save(proj / f"review/{name}.jpg", quality=90); print("sheet:", proj / f"review/{name}.jpg", len(ts))
    with_page(proj, go)

def sheet_video(proj, video, args, name=None):
    """从成片抽帧拼图：看编码后的暗部 banding、音画是否对齐。--sheet 16 --from renders/final.mp4"""
    from PIL import Image, ImageDraw, ImageFont
    video = Path(video)
    if not video.is_file(): sys.exit(f"找不到成片：{video}")
    dur = probe_video(video).get("duration")
    if not dur: sys.exit(f"读不到成片时长：{video}")
    if len(args) > 1:
        ts = [as_float(x, "--sheet") for x in args]
    else:
        n = as_int(args[0] if args else None, "--sheet", 16)
        ts = [round((i + .5) * dur / max(1, n), 2) for i in range(max(1, n))]
    ts = [t for t in ts if 0 <= t <= dur]
    (proj / "review").mkdir(exist_ok=True)
    cols = 4 if len(ts) > 6 else 2 if len(ts) > 1 else 1
    tw = 1920 // cols; th = tw * 9 // 16
    out_img = Image.new("RGB", (cols * tw, -(-len(ts) // cols) * th), (20, 20, 20))
    draw = ImageDraw.Draw(out_img)
    try: font = ImageFont.truetype(str(font_file("JetBrainsMono.ttf") or (ENGINE / "fonts/JetBrainsMono.ttf")), 18)
    except Exception: font = ImageFont.load_default()
    for i, t in enumerate(ts):
        im = Image.open(io.BytesIO(extract_frame(video, t))).convert("RGB").resize((tw, th), Image.LANCZOS)
        x, y = (i % cols) * tw, (i // cols) * th; out_img.paste(im, (x, y))
        draw.rectangle([x, y + th - 24, x + 92, y + th], fill=(0, 0, 0))
        draw.text((x + 5, y + th - 22), f"{t:.2f}", font=font, fill=(255, 230, 0))
    out_path = proj / "review" / f"{name or ('sheet_' + video.stem)}.jpg"
    out_img.save(out_path, quality=90)
    print(f"sheet(video): {out_path}  {video.name}  {dur:.1f}s  {len(ts)} 帧")

def strip(proj, t0, t1, n=12):
    def go(pg):
        fps = pg.evaluate("FPS")
        i0, i1 = round(t0 * fps), round(t1 * fps)
        if i1 <= i0: i1 = i0 + 1
        count = i1 - i0
        take = max(2, min(int(n), count)) if count > 1 else 1
        if take <= 1: return [round(i0 / fps, 3)]
        return [round((i0 + k * (count - 1) / (take - 1)) / fps, 3) for k in range(take)]
    times = with_page(proj, go)
    sheet(proj, [repr(t) for t in times], f"strip_{t0:g}-{t1:g}")

# ---------- 渲染 ----------
def frames(proj, out, a=0.0, b=None, crf=14.0, preset="medium", preview=False, label=None):
    def go(pg):
        end, fps = pg.evaluate("END"), pg.evaluate("FPS")
        bb = end if b is None else min(b, end)
        n0, n1 = round(a * fps), round(bb * fps)
        total = max(0, n1 - n0)
        if total <= 0:
            sys.exit(f"渲染区间为空：{a:g}–{bb:g}s；终点必须大于起点")
        errf = tempfile.TemporaryFile()
        try:
            ff = subprocess.Popen([ffmpeg_bin(), "-v", "error", "-y", "-f", "image2pipe",
                                   "-framerate", str(fps), "-i", "-"]
                                  + encode_args(crf, preset, preview) + [str(out)],
                                  stdin=subprocess.PIPE, stderr=errf)
        except FileNotFoundError:
            sys.exit(f"找不到 ffmpeg（{ffmpeg_bin()}）：请确认补充包已安装，或把 ffmpeg 放进 PATH")
        prog = Progress(total, label or out.stem)
        try:
            for k, i in enumerate(range(n0, n1), 1):
                try:
                    ff.stdin.write(shot(pg, i / fps))
                except (BrokenPipeError, OSError):
                    errf.seek(0)
                    sys.exit("ffmpeg 提前退出：" + (errf.read().decode("utf-8", "replace").strip() or "未知错误"))
                prog.step(k)
        finally:
            try: ff.stdin.close()
            except Exception: pass
        rc = ff.wait(); prog.close()
        if rc != 0:
            errf.seek(0)
            sys.exit(f"ffmpeg 失败（退出码 {rc}）：\n" + errf.read().decode("utf-8", "replace").strip())
        errf.close()
        # 完成标记由子进程自己写：父进程被杀时，已经渲完的段仍然算数。
        # 标记里带区间，镜头表改了以后同名分段会被判脏，不会拿旧内容去拼。
        if not preview:
            marker = {"hash": project_hash(proj), "from": round(a, 3), "to": round(bb, 3)}
            done_marker(Path(out)).write_text(json.dumps(marker), encoding="utf-8")
        print("done", out, f"{bb - a:.2f}s")
        print_video(out)
    with_page(proj, go)

def concat_parts(proj, parts, out, keep=True):
    parts = [p for p in parts if p.exists() and p.stat().st_size > 0]
    if not parts:
        sys.exit("没有可拼接的分段")
    lst = proj / "renders/_parts.txt"
    lst.write_text("".join(f"file '{p.as_posix()}'\n" for p in parts), encoding="utf-8")
    ffmpeg_run(["-v", "error", "-y", "-nostdin", "-f", "concat", "-safe", "0", "-i", str(lst),
                "-c", "copy", "-movflags", "+faststart", str(out)], "拼接分段")
    if not keep:
        for p in parts:
            p.unlink(missing_ok=True)
            done_marker(p).unlink(missing_ok=True)
            p.with_suffix(".log").unlink(missing_ok=True)
        lst.unlink(missing_ok=True)
        clear_manifest(proj)
    print("done", out)
    print_video(out)

def frames_parallel(proj, out, jobs, crf=14.0, preset="medium", keep=True, fresh=False,
                    only=None, segments=None):
    """把全片按镜头（或 --parts / 等分）切成段，最多 jobs 个进程同时渲，再无损拼接。
    每段由子进程自己写 .done 标记（内容是工程源码指纹），父进程被杀时已渲完的段仍然有效。
    --only 只重渲指定镜号/名字/时间，再用全部已完成分段重新拼接；分段默认保留，--clean-parts 才删。"""
    meta = project_meta(proj)
    end, fps = meta["end"], meta["fps"]
    jobs = max(1, int(jobs))
    shots = meta.get("shots")
    if segments:
        ranges = fill_gaps([{"from": a, "to": min(b, end), "name": f"part{i}", "no": i + 1}
                            for i, (a, b) in enumerate(segments)], end)
        if shots: print("提示：--parts 覆盖了 film.js 的 SHOTS 切分")
    elif shots:
        ranges = shot_ranges(shots, end)
    else:
        ranges = equal_ranges(end, fps, jobs)
    gaps = [r for r in ranges if r.get("gap")]
    if gaps:
        print("  空档已单独成段（保持总时长）：" + "、".join(f"{r['from']:.2f}–{r['to']:.2f}s" for r in gaps))
    entries = part_entries(proj, ranges)
    digest = project_hash(proj)
    write_manifest(proj, entries, digest)
    selected = select_entries(entries, only) if only else None

    pending = []
    for entry in entries:
        part, done = entry["path"], done_marker(entry["path"])
        complete = part.exists() and part.stat().st_size > 0 and done.exists()
        same_range = complete and marker_range_ok(part, entry)
        if selected is not None and entry["index"] not in selected:
            print(f"  保留 {part.name}（{entry['from']:.2f}–{entry['to']:.2f}s）" if same_range
                  else f"  跳过 {part.name}（区间与当前镜头表不一致或分段不存在，先跑 --jobs）")
            continue
        if selected is not None:
            pending.append(entry)   # 选中的段无条件重渲
            continue
        if complete and same_range and not fresh and marker_matches(part, digest, entry):
            print(f"  skip {part.name}（{entry['from']:.2f}–{entry['to']:.2f}s，源码未变）")
            continue
        pending.append(entry)
    if selected is not None and not pending:
        print("--only 选中的分段已全部完成，直接拼接")

    def render_one(entry):
        log = proj / f"renders/_part{entry['index']:02d}.log"
        cmd = [sys.executable, str(Path(__file__).resolve()), str(proj), "--clip",
               repr(entry["from"]), repr(entry["to"]), entry["path"].stem,
               "--crf", f"{crf:g}", "--preset", preset]
        env = dict(os.environ, PYTHONIOENCODING="utf-8")
        try:
            with open(log, "w", encoding="utf-8") as lf:
                rc = subprocess.call(cmd, stdout=lf, stderr=subprocess.STDOUT, env=env)
        except OSError as error:
            return entry, -1, log, str(error)
        return entry, rc, log, None

    failures = []
    if pending:
        print(f"  渲染 {len(pending)} 段（最多 {min(jobs, len(pending))} 路并行；每路约 0.5–1 GB 内存）")
        with ThreadPoolExecutor(max_workers=min(jobs, len(pending))) as pool:
            futures = {pool.submit(render_one, entry): entry for entry in pending}
            for future in as_completed(futures):
                entry, rc, log, error = future.result()
                part, done = entry["path"], done_marker(entry["path"])
                ok = rc == 0 and part.exists() and part.stat().st_size > 0 and done.exists()
                if ok:
                    print(f"  done {part.name}（{entry['from']:.2f}–{entry['to']:.2f}s）")
                else:
                    failures.append((entry, rc, log, error))
                    print(f"  第 {entry['index']} 段失败（退出码 {rc}），日志：{log}")
    if failures:
        for _, _, log, error in failures:
            try: tail = log.read_text(encoding="utf-8", errors="replace").splitlines()[-12:]
            except OSError: tail = []
            print(f"--- {log.name} ---\n" + "\n".join(tail or ([error] if error else [])))
        sys.exit(f"{len(failures)} 段未完成。已完成的分段保留在 {proj / 'renders'}；"
                 f"修好后重跑 --jobs {jobs}（默认续渲），或直接 --concat。")

    if selected is not None:
        missing = [entry["path"].name for entry in entries
                   if not (entry["path"].exists() and entry["path"].stat().st_size > 0
                           and done_marker(entry["path"]).exists()
                           and marker_range_ok(entry["path"], entry))]
        if missing:
            sys.exit("--only 已重渲，但其余分段还不完整，暂不拼接：" + "、".join(missing) +
                     f"\n  先跑：python engine/render.py \"{proj}\" --jobs {jobs}")
        concat_existing(proj, name=None, keep=keep, force=False)
    else:
        concat_parts(proj, [entry["path"] for entry in entries], out, keep)

def concat_existing(proj, name=None, keep=True, force=False):
    manifest = read_manifest(proj)
    expected = {}
    if manifest:
        listed = [proj / "renders" / str(entry.get("file")) for entry in manifest["parts"] if entry.get("file")]
        missing = [p.name for p in listed if not p.is_file()]
        if missing: print("清单里缺失的分段：" + "、".join(missing), file=sys.stderr)
        parts = [p for p in listed if p.is_file()]
        expected = {proj / "renders" / str(entry.get("file")): entry
                    for entry in manifest["parts"] if entry.get("file")}
    else:
        parts = sorted((proj / "renders").glob("_part*.mp4"))
    if not parts:
        sys.exit(f"{proj / 'renders'} 下没有 _part*.mp4")
    complete, skipped = [], []
    for p in parts:
        info = expected.get(p)
        if p.stat().st_size <= 0 or not done_marker(p).exists():
            skipped.append(p.name)
        elif info is not None and not marker_range_ok(p, {"from": float(info["from"]), "to": float(info["to"])}):
            skipped.append(p.name + "（区间不一致，镜头表改过后要重渲）")
        else:
            complete.append(p)
    if skipped and not force:
        print("忽略没有完成标记或区间不一致的分段：" + "、".join(skipped) + "（确认可拼接后加 --force）", file=sys.stderr)
    use = [p for p in parts if p.stat().st_size > 0] if force else complete
    if not use:
        sys.exit("没有已完成的分段可拼；分段可能被超时中断，需重渲对应段")
    if not force and manifest and len(use) < len(manifest["parts"]):
        print(f"注意：只拼了 {len(use)}/{len(manifest['parts'])} 段，成片时长会短于 END；"
              f"补渲后重跑 --concat。", file=sys.stderr)
    concat_parts(proj, use, proj / f"renders/{name or 'raw.mp4'}", keep)

def clips(proj, specs, out_name=None, crf=14.0, preset="medium", keep=False):
    parts = []
    for index, (a, b) in enumerate(specs):
        part = proj / f"renders/_clip{index:02d}.mp4"; parts.append(part)
        frames(proj, part, a, b, crf, preset, label=f"clip {a:g}-{b:g}")
    out = proj / f"renders/{out_name or 'clips'}.mp4"
    concat_parts(proj, parts, out, keep)

def parse_clip_specs(values):
    specs = []
    for value in values:
        for token in value.split(","):
            token = token.strip()
            if not token: continue
            sep = ":" if ":" in token else "-" if "-" in token else None
            if not sep: sys.exit(f"--clips 每段写成 起:止 或 起-止，收到：{token!r}")
            left, right = token.split(sep, 1)
            a, b = as_float(left, "--clips 起点"), as_float(right, "--clips 终点")
            if b <= a: sys.exit(f"--clips 终点必须大于起点：{token!r}")
            specs.append((a, b))
    if not specs: sys.exit("--clips 后面至少写一段，如 0:12 20:30")
    return specs

def preview_clip(proj, a, b, name=None):
    out = proj / f"renders/{name or f'preview_{a:g}-{b:g}'}.mp4"
    frames(proj, out, a, b, preview=True, label=f"preview {a:g}-{b:g}")

def web(proj, name=None, crf=None, kb=None):
    final, raw = proj / "renders/final.mp4", proj / "renders/raw.mp4"
    src = final if final.exists() else raw
    if final.exists() and raw.exists() and raw.stat().st_mtime > final.stat().st_mtime + 1:
        print("提示：raw.mp4 比 final.mp4 新，本次用 raw 转码（无音频）；需要声音先重跑 audio.py")
        src = raw
    if not src.exists():
        sys.exit("没有 renders/final.mp4 或 renders/raw.mp4；先渲全片再 --web")
    out = proj / f"renders/{name or 'web'}"
    if out.suffix.lower() != ".mp4": out = out.with_suffix(".mp4")
    args = ["-v", "error", "-y", "-nostdin", "-i", str(src), "-map", "0:v:0", "-map", "0:a?",
            "-c:v", "libx264", "-preset", "slow", "-pix_fmt", "yuv420p", "-movflags", "+faststart"]
    if kb:
        args += ["-b:v", f"{int(kb)}k", "-maxrate", f"{int(kb * 1.2)}k", "-bufsize", f"{int(kb * 2)}k"]
    else:
        args += ["-crf", f"{crf if crf is not None else 23:g}", "-maxrate", "8M", "-bufsize", "16M"]
    args += ["-c:a", "copy", str(out)]
    print("web:", src.name, "→", out.name, "（音频直接复制）" if src.name == "final.mp4" else "（源无音频）")
    ffmpeg_run(args, "发布转码")
    print_video(out)

# ---------- 音效事件 ----------
def events(proj):
    (proj / "audio").mkdir(exist_ok=True)
    def go(pg):
        ev = pg.evaluate("events()"); m = pg.evaluate("({end: END, fps: FPS, meta: META})")
        data = {"end": m["end"], "fps": m["fps"], "meta": m["meta"], "events": ev,
                "source_hash": project_hash(proj),
                "source_files": [p.name for p in project_sources(proj)]}
        json.dump(data, open(proj / "audio/events.json", "w", encoding="utf-8"), ensure_ascii=False)
        print("events:", len(ev), "source:", data["source_hash"][:10])
    with_page(proj, go)

# ---------- 分镜表 ----------
STORY_RE = re.compile(r"^\|\s*(\d+)\s*[·.、]\s*([\d.]+)\s*s?\s*[–—~-]\s*([\d.]+)\s*s?\s*\|")

def read_storyboard(proj):
    path = proj / "storyboard.md"
    if not path.is_file(): return None
    rows = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = raw.strip().replace("**", "")
        m = STORY_RE.match(line)
        if not m: continue
        cells = [c.strip() for c in line.strip("|").split("|")]
        rows.append({"no": int(m.group(1)), "from": float(m.group(2)), "to": float(m.group(3)),
                     "sub": cells[1] if len(cells) > 1 else "",
                     "mode": cells[2] if len(cells) > 2 else ""})
    return sorted(rows, key=lambda r: r["from"]) or None

def storyboard_report(proj, end, shots=None, subs=None, gap=1.5):
    """分镜表自查：时间空档/重叠/END 对不上，以及分镜里的镜头在 SHOTS 中缺函数。"""
    rows = read_storyboard(proj)
    if rows is None: return None
    errors, warnings = [], []
    for prev, cur in zip(rows, rows[1:]):
        delta = cur["from"] - prev["to"]
        if delta < -.05:
            errors.append(f"第 {prev['no']} 镜与第 {cur['no']} 镜时间重叠 {-delta:.2f}s")
        elif delta > gap:
            errors.append(f"第 {prev['no']} 镜结束到第 {cur['no']} 镜开始空了 {delta:.2f}s（黑屏风险）")
    last = rows[-1]["to"]
    if isinstance(end, (int, float)) and end > 0:
        if end - last > gap: errors.append(f"最后一镜到 END 空了 {end - last:.2f}s（黑屏风险）")
        elif last > end + .5: errors.append(f"分镜表结束 {last:.2f}s 晚于 END {end:.2f}s")

    if shots:
        ranges = []
        for shot in shots:
            try: a, b = float(shot.get("from")), float(shot.get("to"))
            except (TypeError, ValueError, AttributeError): continue
            if b > a: ranges.append((a, b))
        def uncovered(a, b):
            segs = sorted((max(a, x), min(b, y)) for x, y in ranges if y > a and x < b)
            out, cur = [], a
            for x, y in segs:
                if x > cur + .05: out.append((cur, min(x, b)))
                cur = max(cur, y)
            if cur < b - .05: out.append((cur, b))
            return out
        for row in rows:
            if re.search(r"不画|只上字|停住|提问", row["mode"]): continue
            holes = [(a, b) for a, b in uncovered(row["from"], row["to"]) if b - a > .35]
            if holes:
                holes_text = "、".join(f"{a:.2f}–{b:.2f}s" for a, b in holes)
                errors.append(f"第 {row['no']} 镜（{row['from']:.2f}–{row['to']:.2f}s）在 SHOTS 里没有对应镜头：{holes_text}")
        for shot in ranges:
            if not any(shot[0] < row["to"] and shot[1] > row["from"] for row in rows):
                warnings.append(f"SHOTS 里的 {shot[0]:.2f}–{shot[1]:.2f}s 没有出现在分镜表")
    else:
        warnings.append("film.js 没有声明 SHOTS；只能检查分镜表时间，无法核对镜头函数。"
                        "在 VK.film({ shots: SHOTS }) 里传入镜头表后可逐镜核对。")

    if subs is not None:
        def norm(text):
            return re.sub(r"[\s，。！？、；：“”‘’（）()·,\.!\?;:'\"{}!]", "", text or "")
        for row in rows:
            overlap = [s for s in subs if isinstance(s, dict) and isinstance(s.get("a"), (int, float))
                       and isinstance(s.get("b"), (int, float)) and s["a"] < row["to"] and s["b"] > row["from"]]
            if not overlap:
                warnings.append(f"第 {row['no']} 镜在分镜表里有字幕，但 SUBS 里没有落在该时段的条目")
            elif row["sub"] and not any(norm(row["sub"]) in norm(s.get("zh")) or norm(s.get("zh")) in norm(row["sub"]) for s in overlap):
                warnings.append(f"第 {row['no']} 镜分镜表字幕与 SUBS 文本对不上：{row['sub'][:24]}")
    else:
        warnings.append("film.js 没有把 SUBS 传给 VK.film({ subs: SUBS })；跳过字幕对齐检查。")
    return rows, errors, warnings

def print_storyboard(proj, end, shots=None, subs=None):
    report = storyboard_report(proj, end, shots, subs)
    if report is None:
        sys.exit(f"{proj / 'storyboard.md'} 不存在")
    rows, errors, warnings = report
    for message in warnings: print("storyboard warning:", message)
    for message in errors: print("storyboard error:", message)
    if errors:
        print(f"storyboard: {len(errors)} 个错误，{len(warnings)} 个警告")
        sys.exit(1)
    print(f"storyboard: ok（{len(rows)} 镜，{len(warnings)} 个警告）")

# ---------- 自检 ----------
def det(proj, n=12):
    def go(pg):
        end = pg.evaluate("END"); r = random.Random(7); ts = [round(r.uniform(0, end), 3) for _ in range(n)]
        h1 = {t: hashlib.md5(shot(pg, t)).hexdigest() for t in ts}
        order = ts[:]; r.shuffle(order)
        bad = [t for t in order if hashlib.md5(shot(pg, t)).hexdigest() != h1[t]]
        print("deterministic" if not bad else f"NOT deterministic at {bad} —— 有东西在跨帧存状态或用了 Math.random()")
    with_page(proj, go)

def lint(proj):
    def go(pg):
        info = pg.evaluate("""() => {
          let ev = [], err = null;
          try { ev = events(); } catch (e) { err = String(e); }
          const c = document.getElementById('stage');
          return { end: window.END, fps: window.FPS, meta: window.META || {},
                   missing: window.VK_MISSING_FONTS || [], events: ev, events_error: err,
                   stage: c ? [c.width, c.height] : null,
                   shots: window.SHOTS || null, subs: window.SUBS || null };
        }""")
        return info
    info = with_page(proj, go)
    errors, warnings = [], []
    end, fps = info.get("end"), info.get("fps")
    if not isinstance(end, (int, float)) or end <= 0: errors.append("END 未定义或不是正数")
    if not isinstance(fps, (int, float)) or fps <= 0: errors.append("FPS 未定义或不是正数")
    if info.get("events_error"): errors.append("events() 抛错：" + str(info["events_error"]))
    if info.get("missing"): errors.append("字体未加载：" + "、".join(info["missing"]))
    if info.get("stage") != [1920, 1080]: errors.append(f"stage 画布是 {info.get('stage')}，应为 1920×1080")
    meta = info.get("meta") or {}
    if not meta.get("title"): warnings.append("meta.title 为空")
    known = KNOWN_SFX
    hushes = []
    for event in info.get("events") or []:
        if not isinstance(event, (list, tuple)) or len(event) < 2: continue
        when, name = event[0], event[1]; params = event[2] if len(event) > 2 and isinstance(event[2], dict) else {}
        if name not in known: errors.append(f"未知音效 {name!r} @ {when}s")
        if not isinstance(when, (int, float)) or when < 0 or (isinstance(end, (int, float)) and when > end + .001):
            errors.append(f"音效 {name} @ {when} 超出 0–{end}s")
        if name == "hush" and isinstance(when, (int, float)):
            hushes.append((when, when + float(params.get("dur", 2.5))))
    for event in info.get("events") or []:
        if not isinstance(event, (list, tuple)) or len(event) < 2: continue
        when, name = event[0], event[1]
        if name == "hush" or not isinstance(when, (int, float)): continue
        for a, b in hushes:
            if a + .05 < when < b - .05:
                warnings.append(f"{name} @ {when}s 落在 hush {a:.2f}–{b:.2f}s 内，会被静音")
    for key in ("noise", "silence"):
        for item in meta.get(key) or []:
            if not isinstance(item, (list, tuple)) or len(item) not in (2, 3):
                errors.append(f"meta.{key} 的条目应为 [起, 止] 或 [起, 止, 强度]：{item!r}"); continue
            a, b = item[0], item[1]
            if not isinstance(a, (int, float)) or not isinstance(b, (int, float)) or b < a:
                errors.append(f"meta.{key} 区间无效：{item!r}")
            elif isinstance(end, (int, float)) and (a < 0 or b > end + .001):
                errors.append(f"meta.{key} 区间 {a}–{b}s 超出 0–{end}s")
    src = "\n".join(p.read_text(encoding="utf-8", errors="replace")
                    for p in project_sources(proj) if p.suffix.lower() == ".js")
    if re.search(r"Math\.random\s*\(", src): errors.append("工程用了 Math.random()，--det 会失败；改用 rng(seed)")
    if re.search(r"Date\.now\s*\(", src): errors.append("工程用了 Date.now()，--det 会失败；时间只从 t 取")
    report = storyboard_report(proj, end, info.get("shots"), info.get("subs"))
    if report:
        _, sb_errors, sb_warnings = report
        errors.extend(sb_errors); warnings.extend(sb_warnings)
    for message in warnings: print("lint warning:", message)
    for message in errors: print("lint error:", message)
    if errors:
        print(f"lint: {len(errors)} 个错误，{len(warnings)} 个警告")
        sys.exit(1)
    print(f"lint: ok（{len(warnings)} 个警告）")

def sample_times(end, interval):
    n = max(1, int(math.ceil(end / max(.05, interval))))
    return [round(i * end / n, 3) for i in range(n)]

# 像素兜底：手写 ctx / 渐变不会被 recordFrame 记到。
# 做法是整帧 - 纯背景帧的差分：背景（含 B 的整幅插画）两边都有，不算内容；
# 字幕带里与背景不同的像素才是真正越界的东西。
BAND_DIFF = 10     # 像素差阈值（0–255）
BAND_RATIO = .002  # 超过阈值的像素占比
BAND_MEAN = 1.5    # 平均像素差

def analyze_band_diff(full_png, bg_png, safe_y=SAFE_Y, diff_max=BAND_DIFF):
    from PIL import Image, ImageChops
    import numpy as np
    safe_y = int(safe_y)
    full_im = Image.open(io.BytesIO(full_png)).convert("L")
    w, h = full_im.size
    box = (0, safe_y + 1, w, h)
    bg_im = Image.open(io.BytesIO(bg_png)).convert("L")
    a = np.asarray(ImageChops.difference(full_im.crop(box), bg_im.crop(box)), dtype=np.float32)
    mask = a > diff_max
    ratio = float(mask.mean())
    mean = float(a.mean())
    ys, xs = np.where(mask)
    bbox = ([int(xs.min()), int(ys.min()) + safe_y + 1,
             int(xs.max()) + 1, int(ys.max()) + safe_y + 2] if len(xs) else None)
    return ratio, mean, bbox

def band_scan(pg, times, safe_y=SAFE_Y, diff_max=BAND_DIFF, ratio_max=BAND_RATIO,
              mean_max=BAND_MEAN, out_dir=None):
    """隐藏字幕后逐帧比较整帧与纯背景帧；手写 ctx、柔边渐变也会被像素抓到。"""
    from PIL import Image, ImageDraw
    safe_y = int(safe_y)
    if not pg.evaluate("typeof window.__vkBackground === 'function'"):
        print("band: 引擎没有 __vkBackground（旧版 vk.js）；跳过像素检查", file=sys.stderr)
        return []
    if pg.evaluate("window.__vkTheme || 'night'") == "flat":
        print("band: flat 场景整幅是插画底，跳过像素检查；装置请用 fr.mark 命名，靠 --check-safe 的包围盒")
        return []
    pg.evaluate("() => { if (window.VKit) window.VKit.subs = function () {}; }")
    flagged = []
    for t in times:
        full = shot(pg, t)
        pg.evaluate("(t) => window.__vkBackground(t)", t)
        bg = pg.screenshot(type="png")
        ratio, mean, bbox = analyze_band_diff(full, bg, safe_y, diff_max)
        if ratio <= ratio_max and mean <= mean_max:
            continue
        item = {"t": t, "ratio": ratio, "mean": mean, "bbox": bbox}
        if out_dir is not None and len(flagged) < 12:
            out_dir.mkdir(parents=True, exist_ok=True)
            im = Image.open(io.BytesIO(full)).convert("RGB")
            ImageDraw.Draw(im).line([(0, safe_y), (im.width, safe_y)], fill=(255, 70, 70), width=3)
            path = out_dir / f"band_{t:07.2f}.png"; im.save(path)
            item["path"] = path
        flagged.append(item)
    if not flagged:
        print(f"band: ok（{len(times)} 个采样时刻，字幕已隐藏，y≤{safe_y} 与背景一致）")
        return flagged
    print(f"band: {len(flagged)} 个时刻字幕带里有非字幕内容（y>{safe_y}）")
    for item in flagged:
        where = f"，{item['path']}" if item.get("path") else ""
        bbox = f"  bbox={item['bbox']}" if item.get("bbox") else ""
        print(f"  t={item['t']:.2f}s  与背景差 {item['ratio'] * 100:.2f}%（平均 {item['mean']:.1f}）{bbox}{where}")
    print("  提示：先看标注帧；手写 ctx 也会被查到。可用 --band-diff / --band-ratio 调阈值。")
    return flagged

def check_safe(proj, interval=0.5, safe_y=SAFE_Y, diff_max=BAND_DIFF, ratio_max=BAND_RATIO):
    def go(pg):
        end = pg.evaluate("END")
        if not isinstance(end, (int, float)) or end <= 0:
            sys.exit("END 未定义或不是正数")
        times = sample_times(end, interval)
        groups = {}
        for t in times:
            boxes = pg.evaluate("(t) => recordFrame(t)", t)
            frame_counts = {}
            for item in boxes:
                name = item.get("name") or ""
                if SUB_RE.search(name or ""): continue
                box = item.get("box") or [0, 0, 0, 0]
                if len(box) < 4 or box[3] <= safe_y: continue
                key = name or f"未命名 {item.get('kind') or '?'}"
                frame_counts[key] = frame_counts.get(key, 0) + 1
                runs = groups.setdefault(key, [])
                if runs and t - runs[-1]["t1"] <= interval * 1.6:
                    run = runs[-1]; run["t1"] = t; run["frames"] += 1
                    run["max_per_frame"] = max(run["max_per_frame"], frame_counts[key])
                    if box[3] > run["worst"][3]: run["worst"] = box
                else:
                    runs.append({"t0": t, "t1": t, "frames": 1, "max_per_frame": frame_counts[key], "worst": box})
        if not groups:
            print(f"bbox: ok（y≤{safe_y}，{len(times)} 个采样时刻）")
        else:
            total = sum(len(runs) for runs in groups.values())
            print(f"bbox: {total} 段越界（y>{safe_y}，{len(times)} 个采样时刻）")
            for key, runs in groups.items():
                for run in runs:
                    extra = f"，每帧最多 {run['max_per_frame']} 个" if key.startswith("未命名") else ""
                    print(f"  {key}: {run['t0']:.2f}–{run['t1']:.2f}s（{run['frames']} 帧{extra}），"
                          f"最低 y={run['worst'][3]}，示例 bbox={run['worst']}")
            print("  提示：字幕由 K.subs 自动标成「字幕」；装置用 fr.mark('名字', …) 包住，报告里就能看到名字。")
        flagged = band_scan(pg, times, safe_y, diff_max, ratio_max, out_dir=proj / "review")
        if not groups and not flagged:
            print(f"safe: ok（{len(times)} 个采样时刻，包围盒与像素都在安全区内）")
        else:
            print(f"safe: 不通过（包围盒 {sum(len(v) for v in groups.values())} 段，像素 {len(flagged)} 个时刻）")
    with_page(proj, go)

def check_band(proj, interval=0.5, safe_y=SAFE_Y, diff_max=BAND_DIFF, ratio_max=BAND_RATIO):
    def go(pg):
        end = pg.evaluate("END")
        if not isinstance(end, (int, float)) or end <= 0:
            sys.exit("END 未定义或不是正数")
        band_scan(pg, sample_times(end, interval), safe_y, diff_max, ratio_max, out_dir=proj / "review")
    with_page(proj, go)

def check_overlap(proj, interval=1.0, min_ratio=0.02):
    def inter(a, b):
        x0, y0 = max(a[0], b[0]), max(a[1], b[1]); x1, y1 = min(a[2], b[2]), min(a[3], b[3])
        return max(0, x1 - x0) * max(0, y1 - y0)
    def area(box):
        return max(0, box[2] - box[0]) * max(0, box[3] - box[1])
    def allows(value):
        if value is None: return set()
        if isinstance(value, str): return {value}
        if isinstance(value, (list, tuple)): return {str(x) for x in value}
        return set()
    def go(pg):
        end = pg.evaluate("END")
        if not isinstance(end, (int, float)) or end <= 0:
            sys.exit("END 未定义或不是正数")
        runs = {}
        named_seen = False
        allowed = 0
        for t in sample_times(end, interval):
            boxes = [b for b in pg.evaluate("(t) => recordFrame(t)", t) if b.get("name")]
            if boxes: named_seen = True
            for i in range(len(boxes)):
                for j in range(i + 1, len(boxes)):
                    name_a, name_b = boxes[i]["name"], boxes[j]["name"]
                    allow_a, allow_b = allows(boxes[i].get("allow")), allows(boxes[j].get("allow"))
                    if "all" in allow_a or "all" in allow_b or name_b in allow_a or name_a in allow_b:
                        allowed += 1
                        continue
                    best_hit, best_small, best_box = 0, 0, boxes[j]["box"]
                    for pa in boxes[i].get("parts") or [boxes[i]["box"]]:
                        for pb in boxes[j].get("parts") or [boxes[j]["box"]]:
                            hit, small = inter(pa, pb), min(area(pa), area(pb))
                            if small > 0 and hit > best_hit:
                                best_hit, best_small, best_box = hit, small, pb
                    if best_hit >= 400 and best_small > 0 and best_hit / best_small >= min_ratio:
                        key = " × ".join(sorted([name_a, name_b]))
                        items = runs.setdefault(key, [])
                        if items and t - items[-1]["t1"] <= interval * 1.6:
                            run = items[-1]; run["t1"] = t; run["frames"] += 1
                            if best_hit / best_small > run["ratio"]:
                                run["ratio"], run["area"], run["box"] = best_hit / best_small, best_hit, best_box
                        else:
                            items.append({"t0": t, "t1": t, "frames": 1,
                                          "ratio": best_hit / best_small, "area": best_hit, "box": best_box})
        tail = f"（另有 {allowed} 对按 allow 跳过）" if allowed else ""
        if not named_seen:
            print("overlap: 没有命名的元素；先用 fr.mark('名字', …) 包住装置再查遮挡")
            return
        if not runs:
            print(f"overlap: ok（{len(sample_times(end, interval))} 个采样时刻）{tail}")
            return
        print(f"overlap: {sum(len(v) for v in runs.values())} 段相交{tail}")
        for key, items in runs.items():
            for run in items:
                print(f"  {key}: {run['t0']:.2f}–{run['t1']:.2f}s（{run['frames']} 帧），"
                      f"最大实际相交 {run['area']:.0f}px²（占较小图元 {run['ratio'] * 100:.0f}%），"
                      f"示例 bbox={run['box']}")
        print("  提示：有意让两个装置相交时，用 fr.mark('声波圈', () => {…}, {allow:'你'}) 登记。")
    with_page(proj, go)

# ---------- 版本差分 ----------
def extract_frame(video, t):
    try:
        r = subprocess.run([ffmpeg_bin(), "-v", "error", "-nostdin", "-ss", f"{t:.3f}",
                            "-i", str(video), "-frames:v", "1",
                            "-f", "image2pipe", "-vcodec", "png", "-"], capture_output=True)
    except FileNotFoundError:
        sys.exit(f"找不到 ffmpeg（{ffmpeg_bin()}）：请确认补充包已安装，或把 ffmpeg 放进 PATH")
    if r.returncode != 0 or not r.stdout:
        sys.exit(f"抽帧失败 {video} @ {t:.2f}s：{r.stderr.decode('utf-8', 'replace').strip()}")
    return r.stdout

def diff_videos(a, b, samples=8, out=None):
    from PIL import Image, ImageChops, ImageDraw, ImageFont
    a, b = Path(a), Path(b)
    if not a.exists(): sys.exit(f"找不到 {a}")
    if not b.exists(): sys.exit(f"找不到 {b}")
    da = probe_video(a).get("duration"); db = probe_video(b).get("duration")
    if not da or not db:
        sys.exit("读不到视频时长：ffprobe 不存在时会退回 ffmpeg -i 解析，两者都没成功。\n"
                 "  请确认 PATH 里有可用的 ffmpeg（补充包只带 ffmpeg，不带 ffprobe）。")
    if abs(da - db) > .15:
        print(f"注意：时长不一致 A={da:.2f}s B={db:.2f}s，按较短者比较")
    dur = min(da, db); samples = max(2, min(int(samples), 24))
    times = [(i + .5) * dur / samples for i in range(samples)]
    tw, th, cols = 480, 270, 3
    out_img = Image.new("RGB", (cols * tw, samples * th), (16, 16, 16))
    draw = ImageDraw.Draw(out_img)
    try: font = ImageFont.truetype(str(font_file("JetBrainsMono.ttf") or (ENGINE / "fonts/JetBrainsMono.ttf")), 18)
    except Exception: font = ImageFont.load_default()
    print(f"diff: {a.name} ↔ {b.name}，{samples} 个时刻")
    print("  时刻     平均差  变化像素  bbox")
    for row, t in enumerate(times):
        ia = Image.open(io.BytesIO(extract_frame(a, t))).convert("RGB").resize((tw, th), Image.LANCZOS)
        ib = Image.open(io.BytesIO(extract_frame(b, t))).convert("RGB").resize((tw, th), Image.LANCZOS)
        gray = ImageChops.difference(ia, ib).convert("L")
        hist = gray.histogram(); total = max(1, sum(hist))
        changed = sum(hist[16:]) / total * 100
        mean = sum(i * c for i, c in enumerate(hist)) / total
        mask = gray.point(lambda v: 255 if v > 24 else 0)
        bbox = mask.getbbox()
        heat = Image.merge("RGB", (gray.point(lambda v: min(255, v * 6)), gray, gray))
        x = 0; y = row * th
        out_img.paste(ia, (x, y)); out_img.paste(ib, (x + tw, y)); out_img.paste(heat, (x + 2 * tw, y))
        label = f"{t:.2f}s  mean={mean:.1f}  changed={changed:.1f}%"
        draw.rectangle([x, y, x + 500, y + 24], fill=(0, 0, 0))
        draw.text((x + 5, y + 3), label, font=font, fill=(255, 230, 0))
        print(f"  {t:7.2f}  {mean:7.2f}  {changed:6.1f}%  {bbox}")
    out_path = Path(out) if out else a.with_name(a.stem + "_diff.jpg")
    out_img.save(out_path, quality=88)
    print("diff:", out_path)

# ---------- 主入口 ----------
if __name__ == "__main__":
    a = sys.argv[1:]
    if not a: sys.exit(__doc__)
    crf = as_float(flag_value(a, "--crf", "14"), "--crf")
    preset = flag_value(a, "--preset", "medium")
    clean = "--clean-parts" in a
    keep = not clean
    fresh = "--fresh" in a
    if "--diff" in a:
        vals = flag_args(a, "--diff")
        if len(vals) < 2: sys.exit("用法：python render.py --diff A.mp4 B.mp4 [--samples 8] [--out 结果.jpg]")
        diff_videos(vals[0], vals[1], as_int(flag_value(a, "--samples"), "--samples", 8), flag_value(a, "--out"))
        sys.exit()
    if a[0].startswith("--"):
        sys.exit("第一个参数是工程目录，例如：python render.py <工程> --sheet 16")
    proj = Path(a[0]).resolve()
    if not (proj / "film.js").exists(): sys.exit(f"{proj} 里没有 film.js")
    (proj / "renders").mkdir(exist_ok=True)
    apply_fontconfig_env()
    warn_missing_fonts()
    if "--lint" in a:
        lint(proj)
    elif "--storyboard" in a:
        meta = project_meta(proj)
        print_storyboard(proj, meta["end"], meta.get("shots"), meta.get("subs"))
    elif "--check-safe" in a:
        check_safe(proj, as_float(flag_value(a, "--check-safe", "0.5"), "--check-safe"),
                   as_float(flag_value(a, "--safe-y", str(SAFE_Y)), "--safe-y"),
                   as_float(flag_value(a, "--band-diff", str(BAND_DIFF)), "--band-diff"),
                   as_float(flag_value(a, "--band-ratio", str(BAND_RATIO)), "--band-ratio"))
    elif "--check-band" in a:
        check_band(proj, as_float(flag_value(a, "--check-band", "0.5"), "--check-band"),
                   as_float(flag_value(a, "--safe-y", str(SAFE_Y)), "--safe-y"),
                   as_float(flag_value(a, "--band-diff", str(BAND_DIFF)), "--band-diff"),
                   as_float(flag_value(a, "--band-ratio", str(BAND_RATIO)), "--band-ratio"))
    elif "--overlap" in a:
        check_overlap(proj, as_float(flag_value(a, "--overlap", "1.0"), "--overlap"))
    elif "--web" in a:
        web(proj, flag_value(a, "--web"), kb=as_int(flag_value(a, "--kb"), "--kb", None))
    elif "--concat" in a:
        concat_existing(proj, flag_value(a, "--concat"), keep, force="--force" in a)
    elif "--serve" in a:
        try:
            srv, url = serve(proj, 8765)
        except OSError:
            srv, url = serve(proj, 0)
            print("8765 被占用，改用", url)
        print("预览:", url + "?play", "（Ctrl+C 结束）")
        try: threading.Event().wait()
        except KeyboardInterrupt: pass
    elif "--stills" in a:
        ts = [as_float(x, "--stills") for x in flag_args(a, "--stills")]
        if not ts: sys.exit("--stills 后面写时刻，例如 --stills 2.5 8.5")
        stills(proj, ts, flag_args(a, "--mark") if "--mark" in a else None)
    elif "--strip" in a:
        vals = flag_args(a, "--strip")
        if len(vals) < 2: sys.exit("用法：--strip 起 止 [帧数]，例如 --strip 8.2 9.2 24")
        strip(proj, as_float(vals[0], "--strip 起点"), as_float(vals[1], "--strip 终点"),
              as_int(vals[2] if len(vals) > 2 else None, "--strip 帧数", 12))
    elif "--sheet" in a:
        from_video = flag_value(a, "--from")
        if from_video:
            video = Path(from_video)
            if not video.is_absolute() and not video.exists(): video = proj / video
            sheet_video(proj, video, flag_args(a, "--sheet"))
        else:
            sheet(proj, flag_args(a, "--sheet"))
    elif "--motion" in a:
        vals = flag_args(a, "--motion")
        if len(vals) < 2: sys.exit("用法：--motion 起 止 [帧数]")
        t0, t1 = as_float(vals[0], "--motion 起点"), as_float(vals[1], "--motion 终点")
        n = max(2, as_int(vals[2] if len(vals) > 2 else None, "--motion 帧数", 12))
        sheet(proj, [repr(round(t0 + (t1 - t0) * k / (n - 1), 3)) for k in range(n)], f"motion_{t0:g}-{t1:g}")
    elif "--clips" in a:
        clips(proj, parse_clip_specs(flag_args(a, "--clips")), flag_value(a, "--out"), crf, preset, keep)
    elif "--preview" in a:
        vals = flag_args(a, "--preview")
        if len(vals) < 2: sys.exit("用法：--preview 起 止 [名字]")
        preview_clip(proj, as_float(vals[0], "--preview 起点"), as_float(vals[1], "--preview 终点"),
                     vals[2] if len(vals) > 2 else None)
    elif "--clip" in a:
        vals = flag_args(a, "--clip")
        if len(vals) < 2: sys.exit("用法：--clip 起 止 [名字]")
        frames(proj, proj / f"renders/{vals[2] if len(vals) > 2 else '_clip'}.mp4",
               as_float(vals[0], "--clip 起点"), as_float(vals[1], "--clip 终点"), crf, preset)
    elif "--events" in a: events(proj)
    elif "--det" in a: det(proj)
    elif "--only" in a or "--parts" in a or "--jobs" in a:
        only = None
        if "--only" in a:
            tokens = []
            for value in flag_args(a, "--only"):
                tokens.extend(x.strip() for x in value.split(",") if x.strip())
            if not tokens: sys.exit("--only 后面写镜号、名字或时间区间，例如 --only 3 或 --only 开场")
            only = tokens
        segments = parse_clip_specs(flag_args(a, "--parts")) if "--parts" in a else None
        jobs = as_int(flag_value(a, "--jobs"), "--jobs", 1 if only else 4)
        frames_parallel(proj, proj / "renders/raw.mp4", jobs, crf, preset, keep, fresh, only, segments)
    else:
        frames(proj, proj / "renders/raw.mp4", crf=crf, preset=preset)
