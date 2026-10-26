# 轻量路线 · 页面创作规范

轻量路线的画面是**一个普通 HTML 文件**，用 HTML/CSS/JS/Canvas 直接编写，无构建步骤、无框架。渲染器逐帧调用页面的 `window.seekTo(t)` 并截图，所以**一切视觉都必须是时间 t 的纯函数**。

## 页面契约（必须遵守）

```html
<script>
window.__video = { width: 1280, height: 720, fps: 30, duration: 6 };
window.seekTo = function (t) {
  // 把画面更新到第 t 秒；可以返回 Promise（等字体/图片解码完再 resolve）
};
</script>
```

- `window.__video` 可选但推荐；渲染命令行参数 `--width/--height/--fps/--duration` 会覆盖它。
- `seekTo` 必须同步更新 DOM 与 Canvas；需要等资源时返回 Promise（渲染器会 await）。
- 渲染器在 seek 后立即截图，所以 seekTo 里不要依赖 `requestAnimationFrame` 回调才生效的写法——直接设置最终样式。

## 铁律：时间驱动，禁止墙钟

| 禁止 | 替代 |
|---|---|
| `setTimeout` / `setInterval` 驱动动画 | 全部动画在 `seekTo(t)` 内按 t 计算 |
| CSS `transition` / `animation`（墙钟播放，截图时机不可控） | 在 `seekTo` 里直接设置 `style.opacity/transform` 等终值 |
| `Date.now()` / `performance.now()` | 只用传入的 t |
| 动画库自动播放 | 若用 GSAP：`gsap.timeline({paused:true})`，在 `seekTo` 里 `tl.time(t)` |

直接用 `assets/lite-template.html` 作为起点，它已包含契约、缓动工具和进度条范式。

## 常用工具函数

```js
function clamp01(x){ return x<0?0:x>1?1:x; }
function lerp(a,b,t){ return a+(b-a)*t; }
function easeOut(t){ return 1-Math.pow(1-clamp01(t),3); }
function easeInOut(t){ t=clamp01(t); return t<0.5?4*t*t*t:1-Math.pow(-2*t+2,3)/2; }
function span(t,start,len){ return clamp01((t-start)/len); } // 时间窗进度
```

所有位移、缩放、透明度都过 `lerp + 缓动`，不允许跳变；场景切换用 0.5–1s 的重叠淡化。

## 画面规范

- 固定画布：`html, body` 设成 `width/height = __video` 尺寸，`overflow:hidden`，不做响应式。
- 文字可读性：1080 宽画布主标题约 84px、辅助文字约 44px，按比例缩放；关键内容留 80px 侧边、100px 上下安全区。
- Canvas 2D 适合粒子、图表、波形、打字机代码；DOM/CSS 适合标题、卡片、字幕。两者可叠加。
- 中文字体：补充包不含中文字体文件，Windows 目标机用系统字体栈 `"Microsoft YaHei","SimHei",sans-serif`；Linux 目标机用 `"Noto Sans CJK SC","WenQuanYi Micro Hei",sans-serif`。字体缺失会 fallback，不要把关键文字画进位图。
- 图片/视频素材放项目目录相对路径引用；大图在 `seekTo` 首次用到时可能未解码，用 `img.decode()` 包进 seekTo 返回的 Promise。
- 音频不进 HTML：旁白/音乐用渲染命令的 `--audio` 参数在封装阶段混入。

## 平台尺寸速查

| 场景 | 尺寸 | 备注 |
|---|---|---|
| 横屏通用 / B站 / YouTube | 1920×1080 | 默认 |
| 竖屏短视频 / 抖音 / Shorts | 1080×1920 | 提到"竖屏/短视频"时默认 |
| 方形信息流 | 1080×1080 | |
| 快速预览/样片 | 1280×720 | 迭代期间用，省渲染时间 |

## 调试方法

- 开发时直接用浏览器打开 HTML，`Console` 里手动 `seekTo(2.5)` 检查任意时间点；或写一小段 `setInterval` 预览循环（**只用于人工预览，渲染前删掉或保证 seekTo 不依赖它**）。
- 验收至少检查：首帧、中段、末帧、场景切换点、文字溢出、素材加载。
