# -*- coding: utf-8 -*-
"""逐帧 seek → 截图 → 管道进 ffmpeg。画面是 t 的纯函数，所以可以只渲一段、只截几帧、乱序截。

  python render.py <工程>                       全片 → <工程>/renders/raw.mp4（无声）
  python render.py <工程> --jobs 4              全片，分 4 段并行渲再无损拼起来（长片用这个）
  python render.py <工程> --stills 2.5 8.5 21   只截几帧 → <工程>/review/s_0002.50.png
  python render.py <工程> --sheet 16            全片均匀取 16 帧拼一张 → review/sheet.jpg（开工后先看这个，再渲染）
  python render.py <工程> --sheet 3.5 9 21.5    指定时刻拼一张
  python render.py <工程> --motion 8.2 10.6     一段时间里均匀取 12 帧拼一张 → review/motion_8.2-10.6.jpg（看转场、看动作；后面可以再跟帧数）
  python render.py <工程> --clip 0 12 [名字]    只渲一段 → renders/<名字或_clip>.mp4
  python render.py <工程> --events              导出音效事件表 → audio/events.json（给 audio.py）
  python render.py <工程> --det                 确定性自检：同一批时刻乱序再截一遍，逐字节比
  python render.py <工程> --serve               起本地预览 http://127.0.0.1:8765/?play（8765 被占用时自动换端口；←/→ 无，改完刷新）

工程目录里只需要 index.html + film.js。引擎（vk.js、vk-kit.js）留在本文件所在目录，
字体优先用补充包的 opt/fonts/video（桌面/安卓）或 /usr/share/fonts/wunder（云端），
没有时退回 engine/fonts 或跑 get_fonts.py 下载。都由内置小服务器挂到 /engine/ 下，
不用往每个工程里拷字体，也没有 file:// 的跨目录字体限制。
"""
import functools, hashlib, http.server, io, json, os, random, subprocess, sys, threading
from pathlib import Path

ENGINE = Path(__file__).resolve().parent
REQUIRED_FONTS = [
    "NotoSerifSC-VF.ttf", "NotoSansSC-VF.ttf",
    "CormorantGaramond.ttf", "CormorantGaramond-Italic.ttf",
    "Lora.ttf", "Lora-Italic.ttf", "JetBrainsMono.ttf", "CourierPrime-Regular.ttf",
]

def font_dirs():
    """字体查找顺序：本地 engine/fonts → 补充包 opt/fonts/video → 系统字体目录。
    补充包（桌面版/安卓 proot）把 8 个字体放在 opt/fonts/video 或 /usr/share/fonts/wunder/video。"""
    dirs = [ENGINE / "fonts"]
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

def chrome_executable():
    """优先用补充包里的 Chrome；找不到再退回系统 channel=chrome。"""
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
        "opt/browser/chrome-linux64/chrome", "opt/browser/chrome-linux/chrome",
        "opt/browser/chrome-win/chrome.exe", "opt/browser/chrome.exe",
        "opt/browser/chrome-mac/Chromium.app/Contents/MacOS/Chromium",
    ]
    for root in roots:
        for rel in rels:
            p = root / rel
            if p.is_file():
                return p
    # 安卓 proot 的浏览器包固定在 rootfs 的 /opt/browser。
    for abs_path in (Path("/opt/browser/chrome-linux/chrome"), Path("/opt/browser/chrome")):
        if abs_path.is_file():
            return abs_path
    for base in (Path(sys.prefix) / "ms-playwright", Path(sys.prefix).parent / "ms-playwright"):
        if base.is_dir():
            for pattern in ("chromium-*/chrome-linux/chrome", "chromium-*/chrome-win/chrome.exe",
                            "chromium-*/chrome-mac/Chromium.app/Contents/MacOS/Chromium"):
                for p in sorted(base.glob(pattern)):
                    if p.is_file():
                        return p
    return None

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
            print(f"[chrome] 补充包浏览器启动失败（{exe}）：{e}", file=sys.stderr)
            b = p.chromium.launch(channel="chrome", args=args)
    else:
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
    finally: srv.shutdown()

def stills(proj, ts):
    (proj / "review").mkdir(exist_ok=True)
    def go(pg):
        for t in ts: (proj / f"review/s_{t:07.2f}.png").write_bytes(shot(pg, t))
    with_page(proj, go); print("stills:", len(ts))

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

def frames(proj, out, a=0.0, b=None):
    def go(pg):
        end, fps = pg.evaluate("END"), pg.evaluate("FPS")
        bb = end if b is None else min(b, end)
        ff = subprocess.Popen(["ffmpeg", "-v", "error", "-y", "-f", "image2pipe", "-framerate", str(fps), "-i", "-",
                               "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "14", "-preset", "medium", "-movflags", "+faststart", str(out)], stdin=subprocess.PIPE)
        n0, n1 = round(a * fps), round(bb * fps)
        for i in range(n0, n1):
            ff.stdin.write(shot(pg, i / fps))
            if (i - n0) % 150 == 0: print(f"  {i - n0}/{n1 - n0}", flush=True)
        ff.stdin.close(); ff.wait(); print("done", out, f"{bb - a:.2f}s")
    with_page(proj, go)

def frames_parallel(proj, out, jobs):
    """把全片切成 jobs 段，各开一个进程渲，再用 concat 无损拼接（每段编码参数相同、都从关键帧起）。"""
    end, fps = with_page(proj, lambda pg: (pg.evaluate("END"), pg.evaluate("FPS")))
    total = round(end * fps); cuts = [round(total * i / jobs) for i in range(jobs + 1)]
    parts, procs = [], []
    for i in range(jobs):
        name = f"_part{i:02d}"; parts.append(proj / f"renders/{name}.mp4")
        procs.append(subprocess.Popen([sys.executable, str(Path(__file__).resolve()), str(proj), "--clip", repr(cuts[i] / fps), repr(cuts[i + 1] / fps), name],
                                      stdout=subprocess.DEVNULL if i else None))
    if any(p.wait() for p in procs): sys.exit("有一段渲染失败了，先单独跑 --clip 看报错")
    lst = proj / "renders/_parts.txt"; lst.write_text("".join(f"file '{p.as_posix()}'\n" for p in parts), encoding="utf-8")
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "concat", "-safe", "0", "-i", str(lst), "-c", "copy", "-movflags", "+faststart", str(out)], check=True)
    for p in parts + [lst]: p.unlink()
    print("done", out, f"{end:.2f}s", f"({jobs} 路并行)")

def events(proj):
    (proj / "audio").mkdir(exist_ok=True)
    def go(pg):
        ev = pg.evaluate("events()"); m = pg.evaluate("({end: END, fps: FPS, meta: META})")
        json.dump({"end": m["end"], "fps": m["fps"], "meta": m["meta"], "events": ev}, open(proj / "audio/events.json", "w", encoding="utf-8"), ensure_ascii=False)
        print("events:", len(ev))
    with_page(proj, go)

def det(proj, n=12):
    def go(pg):
        end = pg.evaluate("END"); r = random.Random(7); ts = [round(r.uniform(0, end), 3) for _ in range(n)]
        h1 = {t: hashlib.md5(shot(pg, t)).hexdigest() for t in ts}
        order = ts[:]; r.shuffle(order)
        bad = [t for t in order if hashlib.md5(shot(pg, t)).hexdigest() != h1[t]]
        print("deterministic" if not bad else f"NOT deterministic at {bad} —— 有东西在跨帧存状态或用了 Math.random()")
    with_page(proj, go)

if __name__ == "__main__":
    a = sys.argv[1:]
    if not a: sys.exit(__doc__)
    proj = Path(a[0]).resolve()
    if not (proj / "film.js").exists(): sys.exit(f"{proj} 里没有 film.js")
    (proj / "renders").mkdir(exist_ok=True)
    warn_missing_fonts()
    if "--serve" in a:
        try:
            srv, url = serve(proj, 8765)
        except OSError:
            srv, url = serve(proj, 0)
            print("8765 被占用，改用", url)
        print("预览:", url + "?play", "（Ctrl+C 结束）")
        try: threading.Event().wait()
        except KeyboardInterrupt: pass
    elif "--stills" in a: stills(proj, [float(x) for x in a[a.index("--stills") + 1:]])
    elif "--sheet" in a: sheet(proj, a[a.index("--sheet") + 1:])
    elif "--motion" in a:
        i = a.index("--motion"); t0, t1 = float(a[i + 1]), float(a[i + 2]); n = int(a[i + 3]) if len(a) > i + 3 else 12
        sheet(proj, [repr(round(t0 + (t1 - t0) * k / (n - 1), 3)) for k in range(n)], f"motion_{t0:g}-{t1:g}")
    elif "--clip" in a:
        i = a.index("--clip"); name = a[i + 3] if len(a) > i + 3 else "_clip"
        frames(proj, proj / f"renders/{name}.mp4", float(a[i + 1]), float(a[i + 2]))
    elif "--events" in a: events(proj)
    elif "--det" in a: det(proj)
    elif "--jobs" in a and int(a[a.index("--jobs") + 1]) > 1: frames_parallel(proj, proj / "renders/raw.mp4", int(a[a.index("--jobs") + 1]))
    else: frames(proj, proj / "renders/raw.mp4")
