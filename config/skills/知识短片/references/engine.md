# 引擎

一张 1920×1080 的 Canvas，一个 `seek(t)`。**每一帧只由时间 t 决定**：没有时间线对象，没有补间库，没有跨帧状态。
这是参考片（C 家族）露出的源码里用的模型，也是能"只渲一段、只截几帧、乱序截"的前提。

```
engine/
  vk.js        核心：时间助手、Frame（画线画字画光）、三套底、出帧（辉光 + 颗粒）
  vk-kit.js    每条片都用的件：字幕、片名卡、年份栏、章节标、仪表、骰子、闪、暗一下……
  index.html   页面模板（new_film.py 会拷进工程）
  render.py    逐帧截图 → ffmpeg；也负责静帧、拼图、事件表、预览
  audio.py     按事件表合成声音，或垫一首现成配乐
  get_fonts.py 字体缺了就下（补充包/云端字体优先，缺的才下载）
  fonts/      开发时的字体缓存；仓库只带 OFL 文本，字体本身不入库
```

工程目录里只有 `index.html` 和 `film.js`。引擎由 `render.py` 内置的小服务器挂在 `/engine/` 下。

可选的外观是两套：`night`（C 夜曲）和 `flat`（B 寓言）。下面出现的 `gilt`（鎏金）和它的年份栏、金字卡还在引擎里——寓言的片名卡和货架图在用——但不再作为一套可选外观。

## 命令

```bash
python scripts/new_film.py <工程目录> --world C|B     # 建工程：index.html + 一份没有画面的 film.js 骨架
python engine/render.py <工程> --serve                  # 预览 http://127.0.0.1:8765/?play
python engine/render.py <工程> --stills 2.5 8.5 21      # 截几帧 → review/
python engine/render.py <工程> --sheet 16               # 全片均匀 16 帧拼一张 → review/sheet.jpg
python engine/render.py <工程> --sheet 3.5 9 21.5       # 指定时刻拼一张
python engine/render.py <工程> --motion 8.2 10.6        # 这段时间里均匀取 12 帧拼一张 → review/motion_8.2-10.6.jpg（看转场）
python engine/render.py <工程> --clip 0 30 head         # 只渲 0–30s → renders/head.mp4（成功写 .done）
python engine/render.py <工程> --clips 0:12 20:30       # 多段渲染并自动拼 → renders/clips.mp4
python engine/render.py <工程> --preview 8.2 10.6       # 低码率预览片段 → renders/preview_*.mp4
python engine/render.py <工程> --strip 8.2 9.2 24       # 连续帧条带 → review/strip_*.jpg（看运动）
python engine/render.py <工程> --stills 8 --mark 船     # 画元素包围盒 → review/m_*.png（需 fr.mark）
python engine/render.py <工程> --det                    # 确定性自检
python engine/render.py <工程> --lint                   # END/FPS/字体/音效/meta/分镜表/Math.random 检查
python engine/render.py <工程> --storyboard             # 分镜表时间列 vs film.js 的 SHOTS 逐镜核对
python engine/render.py <工程> --check-safe             # 包围盒 + 像素：非字幕内容越过 y=840 的时段
python engine/render.py <工程> --check-band             # 只做像素检查（手写 ctx / 柔边渐变也查）
python engine/render.py <工程> --overlap                # 命名元素实际相交面积（可用 allow 放行）
python engine/render.py <工程>                          # 全片 → renders/raw.mp4（无声）
python engine/render.py <工程> --jobs 4                 # 按镜头切段，最多 4 路并行渲再无损拼接
python engine/render.py <工程> --only 3                 # 只重渲第 3 镜，再拼全部已完成分段
python engine/render.py <工程> --parts 0:12 12:30       # 没有 SHOTS 时手动指定分段边界
python engine/render.py <工程> --concat                 # 拼接 renders/_part*.mp4（默认保留分段）
python engine/render.py <工程> --clean-parts            # 拼接成功后删分段（默认保留）
python engine/render.py <工程> --web                    # 发布转码 → renders/web.mp4（打印体积/码率）
python engine/render.py --diff A.mp4 B.mp4              # 两版逐帧差分 → A_diff.jpg + 时刻表
python engine/render.py <工程> --sheet 16 --from renders/final.mp4  # 从成片抽帧拼图（编码后检查）
python engine/render.py <工程> --events                 # 导出音效事件表 → audio/events.json
python engine/audio.py  <工程>                          # 合成声音 + 合片 → renders/final.mp4
python engine/audio.py  <工程> --refresh-events         # 先自动重导事件表再合成
python engine/audio.py  <工程> --bgm 曲子.mp3 -8        # 垫现成配乐（-8 dB），事件音照叠
```

`--jobs` 每段由子进程自己写 `.done` 标记（工程源码指纹 + 分段区间），父进程被杀时已渲完的段仍然有效；
默认续渲只跳过指纹一致且非空的分段，`--fresh` 强制全部重渲。分段默认保留，方便改一镜只重渲一镜；
`--clean-parts` 才删。旧版遗留的分段没有 `.done` 标记，确认完整后可以用 `--concat --force` 强行拼接。
`--only 3` / `--only 开场` / `--only 26.4:29.1` 只重渲选中段，其余分段保留并自动重拼；第一次用 `--only` 前先跑一次 `--jobs` 把全部分段铺出来。
`--jobs` 接受 `--jobs 4` 和 `--jobs=4`，不写时按 CPU 核数（最多 4）决定并发；日志里的「渲染 1/N 段」表示其余分段命中 `.done` 被复用，不是并行失效。
`--check-safe` 的像素检查会隐藏字幕，把整帧和「底 + look.under + 后期」的纯背景帧相减：背景（包括 B 的整幅插画）两边都有，不算越界；
手写 ctx 画的内容只出现在整帧里，会被查出来。flat（B）整幅场景都在前景层，默认跳过像素检查，装置请用 `fr.mark` 命名。
阈值可用 `--band-diff`（像素差，默认 10）和 `--band-ratio`（面积占比，默认 0.2%）调；默认每 0.5 秒采样一次。

渲染速度单路约每秒 3–6 帧（1080p，截 PNG），`--jobs 4` 四路真并行（按 CPU 核数和内存调，每路约 0.5–1 GB）。3 分钟的片子仍然要十几分钟——所以先看拼图，再渲片段，最后才渲全片。
**看图不要靠猜。** 环境里 `file://` 不可用：先 `--serve`，再用浏览器打开 `http://127.0.0.1:8765/review/s_0005.00.png`（`sheet.jpg`、`m_*.png`、`band_*.png` 同理）。
ARM 服务器上单路截帧受单核性能限制，长片优先用 `--jobs`；若明显慢于预期，先确认容器是原生 aarch64、Chrome 可执行文件不是 x86-64（qemu 模拟会慢一个数量级）。
命令工具的前台等待到 `timeout_s` 时不会杀渲染，而是把它转入后台命令会话继续跑并返回 `command_session_id`；
看到 `reason: timeout_escalated` 就先轮询会话（带回 `after_seq` 只取新输出），不要重跑。分段渲染中断后，已完成段带 `.done` 标记，
重跑 `--jobs 4` 默认续渲，或 `--concat` 直接拼接。
同一工程用 `renders/.render.lock` 做单实例互斥，并行渲染只由父进程派生子进程；产物先写 `.*.tmp-<pid>` 再 `os.replace`，
所以 `renders/raw.mp4`、`_part*.mp4`、`final.mp4`、`web.mp4` 只要存在就是完整文件。渲染中探测 mp4 会报
`moov atom not found`，这是文件尚未收尾，不是损坏；判断完成只认同名 `.done`。
依赖：Python 的 `playwright`（优先用补充包里的 Chrome，其次用 Playwright 自带 Chromium，最后退回系统 Chrome）、`numpy`、`scipy`、`Pillow`，以及 `ffmpeg`（补充包不带 `ffprobe`；读视频信息会自动退回 `ffmpeg -i` 解析，脚本不要再新增 ffprobe 硬依赖）。
桌面版/安卓补充包已带 scipy 和 8 个视频字体（思源宋体/黑体 + 6 个拉丁/等宽）；中文优先用视频字体，缺失时退回 `opt/fonts/common` 的宋体/雅黑。云端镜像也带 scipy，字体放在 `config/fonts/{video,common}`（挂到 `/usr/share/fonts/wunder`）。开发环境缺字体时跑 `python engine/get_fonts.py`。

## film.js 的样子

```js
(function () {
'use strict';
const { W, H, lerp, seg, ss, E, fade, rng } = VK;
const K = VKit, P = K.PAL.night;

const SHOTS = [
  { no: 1, name: '开场', from: 0, to: 14 },              // 镜头表：和 storyboard.md 的“# 时间”逐行对应
  { no: 2, name: '猜',   from: 38.6, to: 52 },
];
const C = K.cue(SHOTS);                                  // 镜头级单源：字幕/章节标写相对时间
const SUBS  = C.subs([
  { shot: 1, at: 4.0, dur: 4.5, zh: '你一定{猜到}了下一个字。', en: 'You already know the next character.' },
]);
const MARKS = C.marks([{ shot: 1, at: .4, no: '00', zh: '序', en: 'PROLOGUE' }]);

/* 一站一个函数。自己管自己的进场和退场（乘一个透明度），不要靠 draw 里的 if 硬切 */
function hook(fr, t) { const a = 1 - ss(t, 13.5, 14.0); … }
function ch1(fr, t)  { const a = ss(t, 38.6, 39.5); … }

VK.film({
  dur: 300, theme: 'night', meta: { title: '……' },
  shots: SHOTS, subs: SUBS,                              // 暴露给 --jobs 切段与 --storyboard 核对
  sfx: [[14.02, 'chord', { gain: .8 }], [24.03, 'thud']], // 音效也可以声明在这里
  draw(fr, t) {
    fr.look.bgo = { stars: K.dips(t, [14.05, 38.25]) };   // 底的参数每帧可以改
    K.chapterMark(fr, t, MARKS);                          // 常驻元素
    if (t < 14.1) hook(fr, t);
    if (t >= 38.6) ch1(fr, t);                            // 相邻两站的时间窗重叠半秒，各自淡入淡出
    K.flash(fr, t, 24.03);                                // 全幅效果
    K.subs(fr, t, SUBS, 'night');                         // 字幕永远最后画，压在一切之上
  },
});
})();
```

能跑的例子在 `examples/`：`fold_C`（自己的题目，夜曲，33 秒：从血管推进细胞再推到一串珠子，全片一台摄影机；分镜表在同目录 `storyboard.md`）、`pigeon_B`（自己的题目，寓言，33 秒：六个镜头六种景别，计时器按真实时间走）、`stim_B`（寓言，12 秒）、`info_C`（夜曲，47 秒）。
`examples/kit_gallery` 把所有件各演一遍。

**镜头表是切段和分镜核对的单源。** `SHOTS` 的 `from/to` 就是 storyboard.md 每行的时间列；
`--jobs` 把它当分段边界，`--storyboard` 用它查“分镜里有、film.js 里没有”的镜头。分镜之间允许留空档——渲染时会把空档补成独立段，拼完仍是 END 的长度。
改一镜只重渲那一镜：`python engine/render.py <工程> --only 3`（也接受镜名或时间区间），其余分段保留并自动重拼。
新工程请把 `shots`、`subs`、`sfx` 传进 `VK.film`；旧工程没有 `SHOTS` 也能渲，只是 `--storyboard` 只能查时间列本身。

**长片按章拆文件。** 一条 3 分钟的片子大约 600–1000 行。超过 400 行就拆：在 `index.html` 里 `film.js` 之前加
`<script src="ch1.js"></script>`，每个文件往 `window.CH` 上挂自己的函数（`(window.CH = window.CH || {}).urn = function (fr, t) { … }`），
`film.js` 只留字幕表、章节表和 `draw` 里的调度。改一章不碰别的章。

## 时间

| | |
|---|---|
| `seg(t, a, b)` | t 在 [a, b] 里走到哪了，0…1，两头夹住 |
| `ss(t, a, b)` | 同上，再过一道平滑（最常用：某个量在 a→b 之间从 0 到 1） |
| `fade(t, a, b, fin=.5, fout=.5)` | 出现–停留–消失的包络：a 起经 fin 秒亮起，b 前 fout 秒暗下 |
| `E.out3 / io3 / in2 / in3 / out2 / out5 / outExpo / outBack / lin` | 缓动。进场用 `out3`，位移用 `io3`，推近用 `in2`，数字滚动用 `out3` |
| `lerp(a, b, k)` `clamp(x, a=0, b=1)` `smooth(x)` | |
| `K.track([[t, v], …], t)` | 关键帧曲线，平滑穿过每个点。机位、物体轨迹用它 |
| `K.dips(t, [t1, t2, …], half=.55)` | 章与章之间"暗一下"：每个时刻附近降到 0，返回亮度系数 |
| `VK.scene(fr, t, a, b, fn(lt, k), fin, fout)` | 只在 [a, b] 内调用 fn，并替它乘好淡入淡出。lt 是本地时间 |

**随机** 只用 `rng(seed)`（返回一个 0…1 的发生器）和 `hash(n)`。不要用 `Math.random()` 和 `Date.now()`——
同一帧每次渲染必须一模一样，`render.py --det` 会查。

**模拟要真算，但要能回放。** 载入时把整场模拟一次算完、存成表，每帧只按时间查表：

```js
/* 摸 25,000 次石子：DRAWS[n] = 摸完第 n 次时白石的比例 */
const DRAWS = (() => { const r = rng(1713), out = new Float32Array(25001); let w = 0;
  for (let n = 1; n <= 25000; n++) { if (r() < .6) w++; out[n] = w / n; } return out; })();
function urn(fr, t) {
  const n = Math.max(1, Math.floor(25000 * E.in3(seg(t, 75, 82))));   // 这一帧摸到第几次：先慢后快
  /* 折线画到第 n 个点；计数器显示 n 和 DRAWS[n] */
}
```

粒子（钉板的珠子、撒下的针、落下的骨头）同理：每个粒子的出发时刻和随机路径由它的序号决定，
某一帧的位置用"t − 出发时刻"直接算出来，不要一帧一帧积分。

## Frame：`fr`

`fr.t` 当前时间，`fr.frame` 帧号，`fr.x` 原生的 2D context（需要时可以直接用）。

**成组**

| | |
|---|---|
| `fr.g({ a, at:[x,y], rot, s, add, blur, clip:[x,y,w,h], mark:'船' }, fn)` | 在一组变换 / 透明度 / 加法混合里画 fn；`mark` 给这组图元命名 |
| `fr.mark('船', fn, { allow: '波圈' })` | 给一组图元命名。`--check-safe`/`--overlap`/`--mark` 用这些名字出报告；不写就只能报 `未命名 ellipse`。`allow` 让 `--overlap` 跳过这组与指定名字的相交（可写数组，`'all'` 表示全部放行） |
| `fr.layer({ a, s, at, rot, cx, cy, blur, add }, fn)` | 先画到临时画布再整体合成。一组互相重叠的半透明形状要一起淡入淡出时用它，否则重叠处会透 |

**光**

| | |
|---|---|
| `fr.glow(x, y, r, color, a)` | 一团柔光（加法） |
| `fr.spark(x, y, r, color, a)` | 光点：小白核 + 色晕 + 大淡晕 |
| `fr.comet(x, y, len, r, color, a, ang=0)` | 带彗尾的光点，尾巴朝 ang 的反方向 |

**线和形**（最后一个参数都是选项：`color` 描边色、`fill` 填充、`w` 线宽、`a` 透明度、`glow` 辉光半径、`dash`、`add` 加法混合、`cap`）

| | |
|---|---|
| `fr.line(x1, y1, x2, y2, o)` | |
| `fr.poly(pts, o)` | 折线。`o.k`（0…1）只画前 k 这么长——线"长出来"就靠它 |
| `fr.along(pts, k)` | 返回 `[x, y, 切向角]`，给线头安一个光点 |
| `fr.path(svgPathString, o)` | SVG 路径；`o.at / rot / s` 摆放 |
| `fr.circle(cx, cy, r, o)` `fr.ellipse(cx, cy, rx, ry, o)` `fr.arc(cx, cy, r, a0, a1, o)` | `o.a0 / a1` 只画一段弧 |
| `fr.rect(x, y, w, h, o)` | `o.r` 圆角 |
| `fr.grad(x0, y0, x1, y1, [[位置, 色, 透明度], …])` `fr.rgrad(cx, cy, r0, r1, stops)` | 渐变，传给 `fill` |

**字**

`fr.text(str, x, y, o)`，返回宽度。选项：

| | |
|---|---|
| `font` | 字体键，见下 |
| `size` `color` `a` | |
| `track` | 字距，像素（参考片的字距都很大：字幕 0.1em，片名 0.1–0.3em，小标 0.3–0.6em） |
| `align` | `'c'`（默认）`'l'` `'r'` |
| `glow` `glowColor` `glowA` | 字自己的辉光 |
| `shadow` | 深色投影的透明度（压在亮底上的字要开） |
| `blur` | 模糊，像素 |
| `grad` | 竖向渐变 `[[0, 色], [1, 色]]`（金字用） |

`fr.chars(str, x, y, o, per)` 逐字画：`per(i, 字)` 返回 `{ a, blur, dy, color, glow, s }` 覆盖这个字的样子——逐字显影、打字、单字变色都用它。
`fr.layout(str, x, o)` 只算不画，返回每个字的 `{ ch, x, w, cx }`；`fr.measure(str, o)` 返回整行宽。定稿前用它量一下最长的那句字幕。

字体键：

| 键 | 是什么 | 用在哪 |
|---|---|---|
| `serifh` `serif` `serifm` `serifr` `serifl` `serifx` | 宋体 900 / 700 / 500 / 400 / 300 / 200（共用字体） | A 的字幕用 `serif`，金字片名用 `serifh`；C 的大字用 `serifl`，字幕用 `serifr` |
| `sansh` `sans` `sansm` `sansr` | 微软雅黑/黑体 900 / 700 / 500 / 400（共用字体） | B 的字幕和标注 |
| `latin` `latinl` `latinm` `latini` `latinim` | Cormorant Garamond 常规 / 细 / 半粗 / 斜体 / 斜体中粗 | 数字、外文题名、公式 |
| `lora` | Lora 斜体 | C 的英文字幕 |
| `mono` | JetBrains Mono | 等宽小标、章节编号 |
| `type` | Courier Prime | 打字机文稿 |

**声音事件** 推荐写在 `VK.film({ sfx: [[时刻, 名字, 参数], …] })` 里，和画面代码分开；
也可以在 `draw` 里调 `fr.sfx(时刻, 名字, 参数)`。同一个（时刻，名字）只记一次；`render.py --events` 会把全片过一遍收集起来。
名字：`tick`（一声嗒，`pitch` 选音高）`soft` `chord`（`root` 半音偏移）`bloom`（和弦 + 一口气）`rise`（上滑音，`dur`）
`thud` `tock` `stamp` `shimmer` `hush`（之后彻底静音 `dur` 秒）。都有 `gain`。

## 底和出帧：`fr.look`

每帧开始时 `fr.look` 是默认值，`draw` 里可以改：

| | |
|---|---|
| `fr.look.bg` | `'gilt'` `'night'` `'flat'`，默认是 `film({ theme })` |
| `fr.look.bgo` | 底的参数。gilt：`warm`（0 冷蓝…1 暖棕）`pool`（上方光池）`floor`（脚下暖光）`horizon`（桌面地平线）`dust`；night：`core`（中央深蓝）`stars`（星点密度）`tint`；flat：`color` |
| `fr.look.bloom` | 辉光强度。默认 night 1、gilt 0.34、flat 0.12 |
| `fr.look.grain` | 胶片颗粒，默认 0.05 |
| `fr.look.under(ctx, t)` | 画在底之上、内容之下 |
| `fr.look.post(ctx, t)` | 出帧后的全幅效果。一般不直接写，用下面这几个件 |

辉光的做法：内容画在一张透明层上，出帧时把这层缩小、模糊两遍、用加法叠回去。所以**越亮的东西光晕越大**。
在夜空里画实心的大色块会糊成一团——把它画暗一点，或者这一段把 `bloom` 调低。

## 件：`VKit`

**字幕** `K.subs(fr, t, list, 'gilt' | 'flat' | 'night')`

```js
{ a: 3.45, b: 6.5, zh: '运气，能被{计算}吗?', big: 1 }        // {词} 金色，{!词} 红色；big: 1 放大，2 再大
{ a: 9.0, b: 13.6, zh: '……', en: '……' }                     // en 只有 night 用
{ a: 42, b: 45, zh: '蛋越大，越值得孵', tone: 'gold' }        // 整句金色（规则句）
```
`a` 是第一个字开始出现的时刻，`b` 是完全消失的时刻。相邻两句之间留 0.15–0.3 秒空。位置和字号见 `worlds.md`，改默认值改 `K.SUB`。

**A 的年份栏** `K.hudYear(fr, t, chapters, end)`

```js
{ t: 17.0, year: '1494', place: '威尼斯 · 帕乔利',
  src: { la: 'Summa de Arithmetica', zh: '《算术大全》', by: '卢卡·帕乔利 · 威尼斯 · 1494' } }   // src 可省；approx: true 在年份前加"约"
```
到了下一条的 `t`，年份逐位滚过去，地点人物上下交替，出处先退后进。`end` 是整个栏退场的时刻。
`K.seal(fr, cx, cy, 半宽, ['中', '断'], a, { rot, s })` 朱印；`K.meander(fr, y, a)` 回纹边。

**C 的章节标** `K.chapterMark(fr, t, [{ t, no: '03', zh: '余', en: 'REDUNDANCY' }, …], end)`

**B 的仪表** `K.gauge(fr, x, y, { value, max: 150, redFrom: 100, label: '本能读数', zone: '超常', caption: '对象：{一杯奶茶}' })`
`value` 自己按时间插值好再传。过了 `redFrom` 整个表变红。

**片名卡**

- `K.titleGilt(fr, lt, { la, zh, tag, motif(fr, lt, a), a, cy, size, track, rays, tagDy })`：A / B 的金字卡。`lt` 是片名卡的本地时间，0 = 闪白最亮的那一刻。
  `motif` 画片名下面的母题图形；没有母题图形时把 `tagDy` 设成 212 让副题靠上。
- `K.titleNight(fr, lt, { zh, sub, en, a, cy, lineY, size, track })`：C 的点阵成字。lt 0 = 闪亮那一帧，约 3.3 秒聚拢完，4.6 秒出副题。
- `K.card(fr, t, { from, to, kind, … })`：区间版片名卡。自动把 `lt` 换算成 `t - from`，并在 `to` 前淡出；
  `kind` 省略是 `'night'`，传 `'gilt'` 用金字卡。其余参数原样传给对应的 title 函数。排一张 6 秒的卡直接写
  `K.card(fr, t, { from: 10.9, to: 16.8, zh: '…', sub: '…', en: '…' })`，不用自己反推本地时间和淡出。
- `K.dustText(fr, str, cx, cy, 文字选项, k, t)`：单独用"点聚成字"，k 0…1。

**全幅效果**（可以叠用）

| | |
|---|---|
| `K.flash(fr, t, t0, { rise, fall, color, edge, peak })` | 闪。A 进片名：`{ rise: .3, fall: .4 }`；C：默认（一帧到顶，0.35 秒落回） |
| `K.ripple(fr, t, t0, cx, cy, { r0, r1, dur, color })` | 一圈扩散的细环。和 `flash`、`hush` 一起用，就是"定理落地" |
| `K.noiseOverlay(fr, k)` | 全幅噪点 |

`K.sepia(fr, k)` 是老纪录片效果（泛黄 + 椭圆暗角 + 片门抖动），它不是全幅后期：调用的那一刻作用在**已经画好的东西**上。
所以顺序是：画场景 → `K.sepia` → 再画仪表和字幕，后者保持原色。

**小道具**

| | |
|---|---|
| `K.die(fr, cx, cy, size, R, { wire, label, a })` | 3D 骰子。`R` 是旋转矩阵（`K.M3.mm(K.M3.rx(a), K.M3.ry(b), …)`）；`wire` 0…1 从实体溶成金线框，`label` 写在线框的每个面上 |
| `K.dieFace(fr, cx, cy, 半宽, 点数, { a, fill })` | 平面的骰面小图标 |
| `K.numberLine(fr, x0, x1, y, { lo, hi, ticks, k, a })` | 数轴，返回 `X(v)` 把数值换成横坐标 |
| `K.bellPts(x0, x1, yBase, 高)` | 钟形曲线的点列，交给 `fr.poly` |
| `K.PAL.gilt / flat / night` | 三套色板 |

货架就这么大。每条片各自的装置（陶瓮、信封、表格、DNA）在自己的 `film.js` 里用 `fr` 现画——
一个装置通常 20–40 行。画之前先翻 `examples/` 里有没有相近的。

## 容易踩的坑

- **字幕区被占。** 纵向 80%–95% 只属于字幕。装置的最低点不要低于 y=840；改完用 `--check-safe` 查一遍，
  不要只靠截图肉眼扫。检查只记录 text/line/poly/rect/ellipse 的包围盒（glow/spark/path 这类气氛元素不算），
  装置用 `fr.mark('名字', …)` 包住，报告才会写名字；字幕由 `K.subs` 自动标成「字幕」，不会被误报。
- **遮挡看不见。** 两个装置叠在一起、量尺压住主体时，用 `--overlap` 查命名元素的包围盒相交；先 `fr.mark` 再查。
- **相邻两站硬切。** `if (t < 17) a(); else b();` 会切。让两个时间窗重叠半秒，各自乘 `ss` 淡入淡出。
- **辉光过曝。** 见上。A 的实物（骰子、金币、骨头）本身颜色要压暗一档，否则加上辉光会发白。
- **大块的实心东西。** 细胞、身体、场景这类有体积的东西，画进 `fr.look.under = ctx => { … }`：它在底之上、内容之下，不参加辉光。`ctx` 是原生的 2D 上下文。写法见 `examples/fold_C/film.js` 的 `heroCell`。
- **推拉镜头。** 跨好几个数量级的推近、拉远，缩放要在对数空间里插值（`exp(lerp(ln s0, ln s1, k))`），焦点按 `1/s` 插值，否则会先平移再猛冲。自己把点换算到屏幕坐标，不要用 canvas 的缩放——线宽和字号才不会跟着变。见 `fold_C` 的 `camAt`。
- **老式数字。** Cormorant 默认是高低不齐的老式数字；页面模板已经打开等高数字（`lnum`），自己换字体时记得。
- **半透明叠半透明。** 线框、粒子堆用 `add: true`；实体叠实体用 `fr.layer` 整组淡。
- **一帧画太多。** 上千个粒子用 `fr.x.fillRect`，别每个都开 `shadowBlur` 或 `blur`。慢的预处理（取样字形、算模拟）放在载入时，缓存起来。
- **文字太长。** 定稿前 `fr.measure` 量最长的字幕：A 不超过 1300px，C 中文不超过 1400px。
- **时间写死在好几处。** 每站开头定一个本地时间 `const lt = t - 17`，站内全用 `lt`；整站挪动时只改一个数。
