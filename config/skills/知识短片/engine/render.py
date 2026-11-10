# -*- coding: utf-8 -*-
"""逐帧 seek → 截图 → 管道进 ffmpeg。画面是 t 的纯函数，所以可以只渲一段、只截几帧、乱序截。

  python render.py <工程>                       全片 → <工程>/renders/raw.mp4（无声）
  python render.py <工程> --jobs 4              全片，分 4 段并行渲再无损拼起来（长片用这个）
  python render.py <工程> --jobs 4 --resume     接着上次没渲完的分段继续（默认就会跳过已完成且源码未变的段）
  python render.py <工程> --concat              把 renders/_part*.mp4 拼成 raw.mp4（超时后救场）
  python render.py <工程> --keep-parts          --jobs / --clips 完成后保留分段
  python render.py <工程> --web                 发布转码 → renders/web.mp4（1080p30、+faststart；打印体积/码率）
  python render.py <工程> --stills 2.5 8.5 21   只截几帧 → <工程>/review/s_0002.50.png
  python render.py <工程> --stills 8 --mark 船  截帧并画元素包围盒 → review/m_*.png
  python render.py <工程> --sheet 16            全片均匀取 16 帧拼一张 → review/sheet.jpg（开工后先看这个，再渲染）
  python render.py <工程> --sheet 3.5 9 21.5    指定时刻拼一张
  python render.py <工程> --motion 8.2 10.6     一段时间里均匀取 12 帧拼一张 → review/motion_8.2-10.6.jpg（看转场、看动作）
  python render.py <工程> --strip 8.2 9.2 24    连续帧条带 → review/strip_*.jpg（看运动顺不顺）
  python render.py <工程> --clip 0 12 [名字]    只渲一段 → renders/<名字或_clip>.mp4
  python render.py <工程> --clips 0:12 20:30    一次渲多段并自动拼 → renders/clips.mp4
  python render.py <工程> --preview 8.2 10.6    低码率预览片段 → renders/preview_*.mp4
  python render.py <工程> --events              导出音效事件表 → audio/events.json（给 audio.py）
  python render.py <工程> --det                 确定性自检：同一批时刻乱序再截一遍，逐字节比
  python render.py <工程> --lint                静态+运行时自检：END/FPS、字体、音效、meta、Math.random 等
  python render.py <工程> --check-safe [间隔]   安全区检查：非字幕元素越过 y=840 时列出时刻和元素名
  python render.py <工程> --overlap [间隔]      遮挡检查：命名的元素包围盒相交时列出时刻和元素对
  python render.py --diff A.mp4 B.mp4           两版逐帧差分 → A_diff.jpg + 时刻表
  python render.py <工程> --serve               起本地预览 http://127.0.0.1:8765/?play（8765 被占用时自动换端口）

工程目录里只需要 index.html + film.js。引擎（vk.js、vk-kit.js）留在本文件所在目录，
中文优先用补充包 opt/fonts/video 的思源宋体/黑体，缺失时退到 opt/fonts/common 的宋体/雅黑；
拉丁与等宽字体也在 opt/fonts/video（桌面/安卓）或 /usr/share/fonts/wunder（云端），
没有时退回 engine/fonts 或跑 get_fonts.py 下载。都由内置小服务器挂到 /engine/ 下，
不用往每个工程里拷字体，也没有 file:// 的跨目录字体限制。

安全区/遮挡/包围盒依赖 vk.js 的 recordFrame：给装置用 fr.mark('船', () => { … }) 命名，
字幕由 K.subs 自动标成「字幕」，检查时不会把字幕算成占位元素。
"""
import hashlib, http.server, io, json, math, os, random, re, subprocess, sys, tempfile, threading, time
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
        print("done", out, f"{bb - a:.2f}s")
        print_video(out)
    with_page(proj, go)

def concat_parts(proj, parts, out, keep=False):
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
        lst.unlink(missing_ok=True)
    print("done", out)
    print_video(out)

def frames_parallel(proj, out, jobs, crf=14.0, preset="medium", keep=False, fresh=False):
    """把全片切成 jobs 段，各开一个进程渲，再用 concat 无损拼接。
    每段完成写一个 .done 标记（内容是工程源码指纹）；重跑时默认跳过源码未变且已完成的段。"""
    end, fps = with_page(proj, lambda pg: (pg.evaluate("END"), pg.evaluate("FPS")))
    jobs = max(1, int(jobs))
    total = round(end * fps); cuts = [round(total * i / jobs) for i in range(jobs + 1)]
    digest = project_hash(proj)
    parts = [proj / f"renders/_part{i:02d}.mp4" for i in range(jobs)]
    failures = []
    for i in range(jobs):
        part, done = parts[i], done_marker(parts[i])
        if not fresh and part.exists() and part.stat().st_size > 0 and done.exists():
            try: marker = done.read_text(encoding="utf-8").strip()
            except OSError: marker = ""
            if marker == digest:
                print(f"  skip {part.name}（{cuts[i] / fps:.2f}–{cuts[i + 1] / fps:.2f}s，已完成）")
                continue
        log = proj / f"renders/_part{i:02d}.log"
        cmd = [sys.executable, str(Path(__file__).resolve()), str(proj), "--clip",
               repr(cuts[i] / fps), repr(cuts[i + 1] / fps), part.stem,
               "--crf", f"{crf:g}", "--preset", preset]
        env = dict(os.environ, PYTHONIOENCODING="utf-8")
        with open(log, "w", encoding="utf-8") as lf:
            rc = subprocess.call(cmd, stdout=lf, stderr=subprocess.STDOUT, env=env)
        if rc != 0 or not part.exists() or part.stat().st_size == 0:
            failures.append((i, rc, log))
            print(f"  第 {i} 段失败（退出码 {rc}），日志：{log}")
        else:
            done.write_text(digest, encoding="utf-8")
            print(f"  done {part.name}（{cuts[i] / fps:.2f}–{cuts[i + 1] / fps:.2f}s）")
    if failures:
        for _, _, log in failures:
            try: tail = log.read_text(encoding="utf-8", errors="replace").splitlines()[-12:]
            except OSError: tail = []
            print(f"--- {log.name} ---\n" + "\n".join(tail))
        sys.exit(f"{len(failures)} 段未完成。已完成的分段保留在 {proj / 'renders'}；"
                 f"修好后重跑 --jobs {jobs}（默认续渲），或直接 --concat。")
    concat_parts(proj, parts, out, keep)

def concat_existing(proj, name=None, keep=False, force=False):
    parts = sorted((proj / "renders").glob("_part*.mp4"))
    if not parts:
        sys.exit(f"{proj / 'renders'} 下没有 _part*.mp4")
    complete, skipped = [], []
    for p in parts:
        if p.stat().st_size > 0 and done_marker(p).exists(): complete.append(p)
        else: skipped.append(p.name)
    if skipped and not force:
        print("忽略没有完成标记的分段：" + "、".join(skipped) + "（确认可拼接后加 --force）", file=sys.stderr)
    use = [p for p in parts if p.stat().st_size > 0] if force else complete
    if not use:
        sys.exit("没有已完成的分段可拼；分段可能被超时中断，需重渲对应段")
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
                   stage: c ? [c.width, c.height] : null };
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
    for message in warnings: print("lint warning:", message)
    for message in errors: print("lint error:", message)
    if errors:
        print(f"lint: {len(errors)} 个错误，{len(warnings)} 个警告")
        sys.exit(1)
    print(f"lint: ok（{len(warnings)} 个警告）")

def sample_times(end, interval):
    n = max(1, int(math.ceil(end / max(.05, interval))))
    return [round(i * end / n, 3) for i in range(n)]

def check_safe(proj, interval=1.0, safe_y=SAFE_Y):
    def go(pg):
        end = pg.evaluate("END")
        if not isinstance(end, (int, float)) or end <= 0:
            sys.exit("END 未定义或不是正数")
        groups = {}
        for t in sample_times(end, interval):
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
            print(f"safe: ok（y≤{safe_y}，{len(sample_times(end, interval))} 个采样时刻）")
            return
        total = sum(len(runs) for runs in groups.values())
        print(f"safe: {total} 段越界（y>{safe_y}，{len(sample_times(end, interval))} 个采样时刻）")
        for key, runs in groups.items():
            for run in runs:
                extra = f"，每帧最多 {run['max_per_frame']} 个" if key.startswith("未命名") else ""
                print(f"  {key}: {run['t0']:.2f}–{run['t1']:.2f}s（{run['frames']} 帧{extra}），"
                      f"最低 y={run['worst'][3]}，示例 bbox={run['worst']}")
        print("  提示：字幕由 K.subs 自动标成「字幕」；装置用 fr.mark('名字', …) 包住，报告里就能看到名字。")
    with_page(proj, go)

def check_overlap(proj, interval=1.0, min_ratio=0.02):
    def inter(a, b):
        x0, y0 = max(a[0], b[0]), max(a[1], b[1]); x1, y1 = min(a[2], b[2]), min(a[3], b[3])
        return max(0, x1 - x0) * max(0, y1 - y0)
    def area(box):
        return max(0, box[2] - box[0]) * max(0, box[3] - box[1])
    def go(pg):
        end = pg.evaluate("END")
        if not isinstance(end, (int, float)) or end <= 0:
            sys.exit("END 未定义或不是正数")
        runs = {}
        named_seen = False
        for t in sample_times(end, interval):
            boxes = [b for b in pg.evaluate("(t) => recordFrame(t)", t) if b.get("name")]
            if boxes: named_seen = True
            for i in range(len(boxes)):
                for j in range(i + 1, len(boxes)):
                    a, b = boxes[i]["box"], boxes[j]["box"]
                    hit = inter(a, b)
                    small = min(area(a), area(b))
                    if hit >= 400 and small > 0 and hit / small >= min_ratio:
                        key = " × ".join(sorted([boxes[i]["name"], boxes[j]["name"]]))
                        items = runs.setdefault(key, [])
                        if items and t - items[-1]["t1"] <= interval * 1.6:
                            run = items[-1]; run["t1"] = t; run["frames"] += 1
                            if hit / small > run["ratio"]: run["ratio"] = hit / small; run["box"] = b
                        else:
                            items.append({"t0": t, "t1": t, "frames": 1, "ratio": hit / small, "box": b})
        if not named_seen:
            print("overlap: 没有命名的元素；先用 fr.mark('名字', …) 包住装置再查遮挡")
            return
        if not runs:
            print(f"overlap: ok（{len(sample_times(end, interval))} 个采样时刻）")
            return
        print(f"overlap: {sum(len(v) for v in runs.values())} 段相交")
        for key, items in runs.items():
            for run in items:
                print(f"  {key}: {run['t0']:.2f}–{run['t1']:.2f}s（{run['frames']} 帧），"
                      f"最大重叠 {run['ratio'] * 100:.0f}%，示例 bbox={run['box']}")
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
    keep = "--keep-parts" in a
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
    elif "--check-safe" in a:
        check_safe(proj, as_float(flag_value(a, "--check-safe", "1.0"), "--check-safe"),
                   as_float(flag_value(a, "--safe-y", str(SAFE_Y)), "--safe-y"))
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
    elif "--sheet" in a: sheet(proj, flag_args(a, "--sheet"))
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
    elif "--jobs" in a and as_int(flag_value(a, "--jobs", "1"), "--jobs", 1) > 1:
        frames_parallel(proj, proj / "renders/raw.mp4", as_int(flag_value(a, "--jobs", "1"), "--jobs", 1),
                        crf, preset, keep, fresh)
    else:
        frames(proj, proj / "renders/raw.mp4", crf=crf, preset=preset)
