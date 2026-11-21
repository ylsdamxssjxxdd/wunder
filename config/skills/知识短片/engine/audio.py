# -*- coding: utf-8 -*-
"""按画面事件表合成配乐和音效，再和画面合成成片。
  python audio.py <工程目录>                       # 读 audio/events.json → audio/score.wav → renders/final.mp4
  python audio.py <工程目录> --bgm 曲子.mp3 [-8]   # 用现成配乐垫底（默认 -8 dB），事件音照样叠上去
  python audio.py <工程目录> --wav-only            # 只出 wav，不合成
  python audio.py <工程目录> --refresh-events      # 强制重导 events.json，再合成
  python audio.py <工程目录> --reuse-events        # 跳过校验，直接用现有 events.json（会警告）

audio.py 会核对 events.json 里的音效指纹——只算 sfx 代码/声明、META、SUBS/SHOTS 的时间数字：
改画面、调颜色不会让它失效；真改了时间轴就自动重导（逐帧跑代码、不出像素，几秒），
不再因为“挪了一道光”拒绝合成或罚你重扫全片。--reuse-events 可跳过校验直接用旧表。

事件表由 render.py --events 导出：film.js 里每个 fr.sfx(时刻, 名字, 参数) 就是一条。
声音的写法学自参考片（C 家族）：
  低频长音垫底 · 每章开头一记带泛音的钟/垫和弦（约 8 秒衰减）· 屏幕上每打一个字一声"嗒" ·
  提问处一个上滑音 · 说到噪声就真的铺噪声 · 定理落地的那一下之后留 2–3 秒彻底静音（hush）。
事件名：tick / soft / chord / bloom / rise / thud / tock / stamp / shimmer / hush（参数 dur，秒）
META（film.js 的 meta）：{ root: 半音偏移, noise: [[起,止,强度0..1],...], silence: [[起,止],...], drone: 0..1 }
"""
import json, os, subprocess, sys
from pathlib import Path
import numpy as np
sys.path.insert(0, str(Path(__file__).resolve().parent))
from render import ensure_render_lock, temp_output  # 与渲染共用工程锁和原子改名

SR = 48000
D3 = 146.83
PENTA = [0, 2, 4, 7, 9, 12, 14, 16, 19, 21]           # 大调五声，嗒声从这里取音高
CHORD = [0, 7, 12, 14, 19, 24, 26]                   # 根音、五度、八度、九度…… 叠成"开阔"的和弦

def ffmpeg_bin():
    return os.environ.get("WUNDER_FFMPEG_BIN", "").strip() or "ffmpeg"

def hz(semi, base=D3): return base * 2 ** (semi / 12)
def env_exp(n, tau, attack=0.004):
    t = np.arange(n) / SR
    return np.minimum(1, t / attack) * np.exp(-t / tau)
# 只用 numpy，不带 scipy。二阶 Butterworth 经双线性变换后的幅度响应是
# |H| = 1/sqrt(1+(tan(πf/SR)/tan(πfc/SR))^(2n))，直接在频域乘上它（零相位），
# 对噪声塑形、混响 IR 的听感与 scipy 的逐样本 IIR 一致，但快得多，
# 也不必为补充包背一个上百 MB 的 scipy。两端零填充避免循环卷积的边界振铃。
_FILTER_CACHE = {}
def _fast_len(n):                                      # 2/3/5 平滑长度：让 pocketfft 走快路径
    best = 1 << (n - 1).bit_length()
    p5 = 1
    while p5 < best:
        p35 = p5
        while p35 < best:
            q = p35
            while q < n: q *= 2
            if q < best: best = q
            p35 *= 3
        p5 *= 5
    return best
def _butter_mag(n, fc, kind, order=2):
    key = (n, round(float(fc), 3), kind, order)
    mag = _FILTER_CACHE.get(key)
    if mag is None:
        freqs = np.fft.rfftfreq(n, 1.0 / SR)
        wa = np.tan(np.pi * np.minimum(freqs, SR / 2 - 1e-9) / SR)
        wc = np.tan(np.pi * min(max(float(fc), 1e-6), SR / 2 - 1e-9) / SR)
        ratio = np.maximum(wa, 1e-12) / wc
        mag = 1.0 / np.sqrt(1.0 + (ratio if kind == "low" else 1.0 / ratio) ** (2 * order))
        if len(_FILTER_CACHE) > 64: _FILTER_CACHE.clear()
        _FILTER_CACHE[key] = mag
    return mag
def _filter(x, fc, kind, order=2):
    x = np.asarray(x, dtype=np.float64)
    if x.size == 0: return x
    pad = min(max(int(.02 * SR), 64), x.shape[0])
    padded = np.concatenate([np.zeros_like(x[:pad]), x, np.zeros_like(x[:pad])], 0)
    n = _fast_len(padded.shape[0])
    mag = _butter_mag(n, fc, kind, order)
    if padded.ndim > 1: mag = mag.reshape((-1,) + (1,) * (padded.ndim - 1))
    y = np.fft.irfft(np.fft.rfft(padded, n, axis=0) * mag, n=n, axis=0)
    return y[pad:pad + x.shape[0]]
def _convolve(x, ir):                                  # 线性卷积（补零到平滑长度，与 fftconvolve 等价）
    size = _fast_len(x.size + ir.size - 1)
    return np.fft.irfft(np.fft.rfft(x, size) * np.fft.rfft(ir, size), size)[:x.size]
def lowpass(x, fc, order=2): return _filter(x, fc, "low", order)
def highpass(x, fc, order=2): return _filter(x, fc, "high", order)
def pan(x, p):                                        # p: -1 左 … 1 右
    l, r = np.cos((p + 1) * np.pi / 4), np.sin((p + 1) * np.pi / 4); return np.stack([x * l, x * r], 1)
def add(buf, x, t):
    i = int(t * SR)
    if i >= len(buf) or i < 0: return
    n = min(len(x), len(buf) - i); buf[i:i + n] += x[:n]

def tone(f, dur, tau, partials=((1, 1.0),), attack=0.004, vib=0.0):
    n = int(dur * SR); t = np.arange(n) / SR; out = np.zeros(n)
    for mult, amp in partials:
        ph = 2 * np.pi * f * mult * t + (vib * np.sin(2 * np.pi * 4.3 * t) if vib else 0)
        out += amp * np.sin(ph) * np.exp(-t / (tau / (1 + 0.35 * (mult - 1))))   # 高次泛音衰减更快
    return out * np.minimum(1, t / attack)

def ev_tick(rng, p):
    f = hz(PENTA[int(p.get("pitch", 0)) % len(PENTA)] + 24)
    x = tone(f, .5, .09, ((1, 1), (2.01, .28), (3.02, .1)), attack=.002)
    click = highpass(rng.standard_normal(int(.012 * SR)) * env_exp(int(.012 * SR), .003), 3000) * .25
    x[:len(click)] += click
    return pan(x * .16 * p.get("gain", 1), rng.uniform(-.35, .35))
def ev_soft(rng, p):
    return pan(tone(hz(12 + 7), 2.2, .7, ((1, 1), (2, .2)), attack=.03) * .12 * p.get("gain", 1), 0)
def ev_chord(rng, p):
    root = p.get("root", 0); n = int(9 * SR); out = np.zeros((n, 2))
    for k, semi in enumerate(CHORD):
        f = hz(root + semi); x = tone(f, 9, 2.6 - k * .12, ((1, 1), (2.003, .32), (3.01, .14), (4.02, .06)), attack=.012, vib=.6)
        out += pan(x * (.11 / (1 + k * .22)), (k % 3 - 1) * .5)
    sub = tone(hz(root - 12), 6, 2.2, ((1, 1),), attack=.02)
    out[:len(sub)] += pan(sub * .16, 0)
    return out * p.get("gain", 1)
def ev_bloom(rng, p):                                 # 闪白：和弦 + 一口气的噪声扫过
    out = ev_chord(rng, {"root": p.get("root", 0), "gain": 1.15 * p.get("gain", 1)})
    n = int(1.6 * SR); ns = rng.standard_normal((n, 2)) * env_exp(n, .35, .01)[:, None]
    out[:n] += lowpass(highpass(ns, 500), 7000) * .12
    return out
def ev_rise(rng, p):                                  # 上滑音：提问、蓄势
    dur = p.get("dur", 1.0); n = int(dur * SR); t = np.arange(n) / SR
    f = hz(7) * 2 ** (t / dur * 1.0); ph = 2 * np.pi * np.cumsum(f) / SR
    e = np.sin(np.pi * np.minimum(1, t / dur) ** 1.6) ** 1.2
    ns = lowpass(rng.standard_normal(n), 2500) * (t / dur) ** 2 * .22
    return pan((np.sin(ph) * .1 + ns * .1) * e * p.get("gain", 1), 0)
def ev_thud(rng, p):
    n = int(.6 * SR); t = np.arange(n) / SR; f = 95 * np.exp(-t * 9) + 48
    x = np.sin(2 * np.pi * np.cumsum(f) / SR) * env_exp(n, .13, .002) + lowpass(rng.standard_normal(n), 600) * env_exp(n, .04) * .5
    return pan(x * .42 * p.get("gain", 1), 0)
def ev_tock(rng, p):
    f = 520 * 2 ** (int(p.get("pitch", 0)) * 2 / 12); n = int(.25 * SR)
    x = tone(f, .25, .04, ((1, 1), (2.7, .4), (5.1, .15)), attack=.001) + highpass(rng.standard_normal(n), 2500) * env_exp(n, .006) * .3
    return pan(x * .2 * p.get("gain", 1), rng.uniform(-.5, .5))
def ev_stamp(rng, p):
    n = int(.5 * SR); x = ev_thud(rng, {"gain": 1.1})[:n, 0] + highpass(rng.standard_normal(n), 1800) * env_exp(n, .02) * .35
    return pan(x * p.get("gain", 1), 0)
def ev_shimmer(rng, p):
    n = int(2.4 * SR); t = np.arange(n) / SR; out = np.zeros(n)
    for k in range(7): out += np.sin(2 * np.pi * hz(24 + PENTA[k + 2]) * t + rng.uniform(0, 6)) * np.exp(-((t - .25 - k * .06) / .5) ** 2)
    return pan(out * .035 * p.get("gain", 1), 0)
EV = dict(tick=ev_tick, soft=ev_soft, chord=ev_chord, bloom=ev_bloom, rise=ev_rise, thud=ev_thud, tock=ev_tock, stamp=ev_stamp, shimmer=ev_shimmer)

def reverb(x, rng, t60=3.6, wet=.42):
    n = int(t60 * SR); t = np.arange(n) / SR
    ir = lowpass(rng.standard_normal((n, 2)) * np.exp(-6.9 * t / t60)[:, None], 5200); ir[:int(.012 * SR)] *= np.linspace(0, 1, int(.012 * SR))[:, None]
    ir /= np.sqrt((ir ** 2).sum(0)).max()
    wetsig = np.stack([_convolve(x[:, c], ir[:, c]) for c in (0, 1)], 1)
    return x + wetsig * wet

def synth(spec, bgm=False):
    end = spec["end"]; meta = spec.get("meta") or {}; n = int((end + .05) * SR); rng = np.random.default_rng(20261006)
    dry = np.zeros((n, 2)); t = np.arange(n) / SR; root = meta.get("root", 0)
    hush = [(when, when + p.get("dur", 2.5)) for when, name, p in spec["events"] if name == "hush"]
    for when, name, p in spec["events"]:
        if name in EV: add(dry, EV[name](rng, dict(p, root=p.get("root", 0) + root) if name in ("chord", "bloom") else p), when)
    out = reverb(dry, rng)
    if not bgm:                                       # 低频长音垫底：两个略微失谐的正弦 + 很慢的呼吸
        lv = .05 * meta.get("drone", 1)
        d = (np.sin(2 * np.pi * hz(root - 24) * t) + .6 * np.sin(2 * np.pi * hz(root - 24 + .08) * 2 * t) + .25 * np.sin(2 * np.pi * hz(root - 12 + 7) * t)) * (.75 + .25 * np.sin(2 * np.pi * t / 11))
        air = lowpass(highpass(rng.standard_normal(n), 300), 1800) * .012 * (.6 + .4 * np.sin(2 * np.pi * t / 17))
        out += np.stack([d * lv + air, d * lv + np.roll(air, 4000)], 1)
    for a, b, k in meta.get("noise", []):             # 文案说到噪声，配乐里就真的有噪声
        g = np.clip((t - a) / max(.01, b - a), 0, 1) ** 1.5 * (t <= b) * k
        out += lowpass(highpass(rng.standard_normal((n, 2)), 200), 6000) * (g * .12)[:, None]
    g = np.ones(n)
    for a, b in list(meta.get("silence", [])) + hush:  # 彻底静音：0.15s 收掉，0.4s 回来
        g *= 1 - np.clip((t - a) / .15, 0, 1) * (1 - np.clip((t - b) / .4, 0, 1))
    g *= np.clip(t / .6, 0, 1) * np.clip((end - t) / 1.2, 0, 1)
    out *= g[:, None]
    return out

def write_wav(path, x):
    import wave
    x = np.clip(x, -1, 1); pcm = (x * 32767).astype("<i2")
    with wave.open(str(path), "wb") as w: w.setnchannels(2); w.setsampwidth(2); w.setframerate(SR); w.writeframes(pcm.tobytes())

def reimport_events(proj):
    try:
        subprocess.run([sys.executable, str(Path(__file__).resolve().with_name("render.py")), str(proj), "--events"], check=True)
    except subprocess.CalledProcessError as error:
        sys.exit(f"重导事件表失败（退出码 {error.returncode}）；先单独运行 render.py --events 看报错。")

def load_events(proj):
    path = proj / "audio/events.json"
    try:
        return json.load(open(path, encoding="utf-8"))
    except json.JSONDecodeError as error:
        sys.exit(f"{path} 不是有效 JSON（{error}）；重跑 render.py --events 覆盖它。")

def check_events_source(proj, a):
    """events.json 必须和“什么时候响什么”一致，否则音效会落在旧台词的位置上。

    指纹只算 sfx 相关的东西（sfx 代码/声明、META、SUBS/SHOTS 的时间数字）：
    改画面、调颜色不会让它失效，也就不会为了挪一道光重扫全片。
    真的改了时间轴就自动重导（逐帧跑代码、不出像素，几秒）；--reuse-events 可直接用旧表。"""
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from render import event_source_digest
    path = proj / "audio/events.json"
    reuse = "--reuse-events" in a
    force = "--refresh-events" in a
    if not path.is_file():
        if reuse:
            sys.exit("还没有音效事件表 audio/events.json，没有旧表可复用。\n"
                     f"  先运行：python engine/render.py \"{proj}\" --events")
        print("音效事件表还不存在，先导出（只跑代码，几秒）…", flush=True)
        reimport_events(proj)
    spec = load_events(proj)
    current = event_source_digest(proj)
    recorded = spec.get("events_hash")
    if not recorded:
        if reuse:
            print("提示：events.json 是旧格式（没有音效指纹），按 --reuse-events 直接使用。", file=sys.stderr)
        else:
            print("提示：events.json 是旧格式（没有音效指纹），重导一次（几秒）…", file=sys.stderr)
            reimport_events(proj)
            spec = load_events(proj)
    elif recorded != current:
        if reuse:
            print("注意：--reuse-events 跳过了事件表校验；音效时间可能和当前画面差半拍。", file=sys.stderr)
        else:
            print("音效时间轴或音效代码改过了（SUBS/SHOTS/sfx/META 有变动），自动重导事件表（几秒）…", flush=True)
            reimport_events(proj)
            spec = load_events(proj)
    elif force:
        print("--refresh-events：指纹虽然一致，也重导一次。", flush=True)
        reimport_events(proj)
        spec = load_events(proj)
    end = float(spec.get("end") or 0)
    known = {"tick", "soft", "chord", "bloom", "rise", "thud", "tock", "stamp", "shimmer", "hush"}
    for event in spec.get("events") or []:
        if not isinstance(event, (list, tuple)) or len(event) < 2:
            continue
        when, name = event[0], event[1]
        if name not in known:
            sys.exit(f"events.json 里有未知音效 {name!r} @ {when}s；检查 film.js 的 fr.sfx 名字。")
        if not isinstance(when, (int, float)) or when < 0 or when > end + .001:
            sys.exit(f"音效 {name} @ {when}s 超出 0–{end}s；检查 film.js 的时刻。")
    return spec

def cli_flags(argv):
    """--help / 不带工程 / 打错的开关先在这里拦下：音频会覆盖 renders/final.mp4，不能猜着干。"""
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from render import HELP_TOKENS, print_help, reject_flags
    if any(tok in HELP_TOKENS for tok in argv): print_help(0, __doc__)
    reject_flags(argv, known={"--bgm", "--wav-only", "--refresh-events", "--reuse-events"}, scan=False)
    if not argv: print_help(0, __doc__)


if __name__ == "__main__":
    a = sys.argv[1:]
    cli_flags(a)
    proj = Path(a[0]).resolve()
    spec = check_events_source(proj, a)
    bgm = a[a.index("--bgm") + 1] if "--bgm" in a else None
    bgm_db = float(a[a.index("--bgm") + 2]) if bgm and len(a) > a.index("--bgm") + 2 and a[a.index("--bgm") + 2].lstrip("-").replace(".", "").isdigit() else -8
    wav_only = "--wav-only" in a
    if bgm and not Path(bgm).is_file():
        sys.exit(f"找不到配乐文件：{bgm}")
    ensure_render_lock(proj)
    raw, out, end = proj / "renders/raw.mp4", proj / "renders/final.mp4", spec["end"]
    if not wav_only and (not raw.is_file() or raw.stat().st_size == 0):
        sys.exit("还没有 renders/raw.mp4（无声全片），无法合成声音。\n"
                 f"  先渲全片：python engine/render.py \"{proj}\"\n"
                 f"  长片用：  python engine/render.py \"{proj}\" --jobs 4")
    if not wav_only:      # --only 默认不重拼全片：分段比 raw 新，说明声音会配在旧画面上
        newest = max((p.stat().st_mtime for p in (proj / "renders/_parts").glob("_part*.mp4")), default=0)
        if newest > raw.stat().st_mtime + 1:
            print("注意：renders/raw.mp4 比最新分段旧（改过镜头但还没拼片）。\n"
                  f"  先跑：python engine/render.py \"{proj}\" --concat\n"
                  "  否则这段声音会配在旧画面上。", file=sys.stderr)
    x = synth(spec, bgm=bool(bgm)); pk = np.abs(x).max() or 1; x = x / pk * 10 ** (-3 / 20)
    wav = proj / "audio/score.wav"; write_wav(wav, x); print("score:", wav, f"{len(x) / SR:.1f}s", len(spec["events"]), "events")
    if wav_only: sys.exit()
    if bgm:   # 现成配乐：垫底 + 事件音；整体到 -14 LUFS
        fc = f"[2:a]atrim=0:{end},volume={bgm_db}dB,afade=t=in:d=0.4,afade=t=out:st={end - 1.5}:d=1.5[b];[1:a][b]amix=inputs=2:normalize=0,loudnorm=I=-14:TP=-1.5:LRA=11[a]"
        cmd = [ffmpeg_bin(), "-v", "error", "-y", "-nostdin", "-i", str(raw), "-i", str(wav), "-stream_loop", "-1", "-i", bgm, "-filter_complex", fc]
    else:     # 合成配乐本来就安静、动态大：只到 -18 LUFS，别压扁
        cmd = [ffmpeg_bin(), "-v", "error", "-y", "-nostdin", "-i", str(raw), "-i", str(wav), "-filter_complex", "[1:a]loudnorm=I=-18:TP=-1.5:LRA=14[a]"]
    tmp = temp_output(out)
    try:
        r = subprocess.run(cmd + ["-map", "0:v", "-map", "[a]", "-c:v", "copy", "-c:a", "aac", "-b:a", "192k", "-ar", "48000", "-shortest", str(tmp)],
                           capture_output=True, encoding="utf-8", errors="replace")
    except FileNotFoundError:
        sys.exit(f"找不到 ffmpeg（{ffmpeg_bin()}）：请确认补充包已安装，或把 ffmpeg 放进 PATH")
    if r.returncode != 0:
        tmp.unlink(missing_ok=True)
        detail = (r.stderr or "").strip() or "ffmpeg 没有输出错误信息"
        sys.exit(f"合成失败（退出码 {r.returncode}）：\n{detail}")
    os.replace(tmp, out)
    print("done", out)
