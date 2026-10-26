---
name: 视频制作
description: "创建、修改、预览、调试和导出程序化视频。适用于短视频、产品演示、动态图形、字幕视频、素材混剪、地图动画以及 MP4/WebM/透明视频交付。两条路线：有 Remotion/Node 环境走 Remotion；仅本地补充包（python+浏览器+ffmpeg）走轻量 HTML 渲染。"
---

# 视频制作

把用户目标落成可复现、可继续编辑的视频工程，并在交付前完成预览或渲染验收。

## 先选路线（每个工程只做一次）

| 条件 | 路线 |
|---|---|
| 项目已有 Remotion 工程，或环境有可用 Node（>=18）且允许安装依赖 | **Remotion 路线** |
| 仅补充包环境（python + 浏览器 + ffmpeg，无 Node/离线/win7 ia32），或用户要求轻量 | **轻量路线** |

- 拿不准就检查：`node --version` 是否可用、项目里有无 `package.json` 含 remotion 依赖。两个都不满足 → 轻量路线。
- 轻量路线的工程是一个或一组普通 HTML 文件；Remotion 路线的工程是 React 项目。两者不混用。
- 舰体沙盒优先 Remotion（`config/runtime` 已挂载 Node/Remotion）；蛹与舵机默认轻量路线，除非用户明确要 Remotion 且自备 Node 环境。

## 先判断交付目标

- 用户说"制作、创建或修改视频"时，先完成工程和预览；只有明确要求渲染、导出或 MP4 时才生成最终文件。
- 用户只要看效果时：Remotion 路线启动 Studio 用 `browser` 工具检查；轻量路线直接用浏览器打开 HTML，在 Console 里 `seekTo(t)` 抽查关键时间点。截图只用于检查，不作为交付物。
- 用户明确要求导出时，先完成预览和最小质量检查，再渲染。交付输出路径、格式、尺寸、帧率和未完成的检查。
- 用户已有项目时，先检查项目结构和当前改动，保留用户改动，不重置或覆盖无关文件。

## 工具协同

- 使用 `执行命令` 或命令会话运行渲染、媒体探测命令，工作目录必须是用户项目目录；不要把技能目录当作项目目录，也不要把素材复制进技能包。
- 轻量路线运行环境：补充包的 `opt/python/python.exe`、`opt/browser/chrome-win/chrome.exe`（Windows）或 `opt/browser/chrome-linux64/chrome`（Linux amd64）、`opt/ffmpeg/bin/ffmpeg.exe`；也可用环境变量 `WUNDER_CHROME_BIN` / `WUNDER_FFMPEG_BIN`。
- Remotion 路线运行环境：舰体沙盒使用 `config/runtime` 挂载的 Node/Remotion；蛹与舵机需用户自备 Node 并在项目安装 Remotion 依赖。浏览器路径同上。
- 浏览器启用时只使用单一 `browser` 工具：`navigate` → `observe_refs`/`observe_text` → 交互或 `wait` → `screenshot`。导航后旧 ref 失效，必须重新观察；批处理最多 10 步。
- 需要读取专项资料时，用"技能调用"读取 `视频制作` 的 manifest，再按需读取 `{{SKILL_ROOT}}/references/<file>`，不要一次读取整个参考目录。

## 轻量路线工作流（无 Remotion）

1. **明确规格**：画布比例和尺寸、fps、时长、受众、场景顺序、文字、声音、素材来源、字幕语言和交付格式。缺少信息时采用合理默认值并记录。
2. **建工程**：从 `{{SKILL_ROOT}}/assets/lite-template.html` 复制起点模板，读取 `references/lite-authoring.md` 后按契约编写：实现 `window.__video` 与 `window.seekTo(t)`，全部动画是时间 t 的纯函数。
3. **预览验收**：浏览器打开 HTML 抽查首帧、中段、末帧、场景切换、文字溢出、素材加载；必要时用 `screenshot` 保存工作区相对路径。
4. **渲染交付**：用户明确要求时读取 `references/lite-render.md`，用 `scripts/render_lite.py` 渲染。先低清小样确认节奏，再全尺寸出片；音频用 `--audio` 在封装阶段混入。
5. **交付报告**：确认文件存在且体积非零，核对时长、尺寸、帧率和格式，给出工程目录、HTML 入口、产物路径、已做检查和可修改位置。

## Remotion 路线工作流

1. **明确规格**：同轻量路线。
2. **检查工程**：识别 `package.json`、Remotion 入口、composition、`public/` 素材、锁文件和已有脚本。没有工程时读取 `references/project-setup.md` 后创建最小可运行项目。
3. **实现时间线**：使用 `useCurrentFrame()`、`interpolate()` 和 Remotion timing 驱动动画；避免依赖 CSS `transition`、`animation` 或 Tailwind 动画类。场景、字幕和参数保持可编辑。
4. **按需增强**：字幕、旁白、音频可视化、地图、3D、转场、透明背景和动态时长分别读取对应 reference；简单标题不要引入额外依赖。
5. **预览验收**：启动 Studio，检查首帧、中段、末帧、场景切换、文字溢出、素材加载、音画同步和窄窗口。
6. **渲染交付**：用户明确要求时读取 `references/render-delivery.md`，渲染指定 composition。使用 FFmpeg 做媒体探测、抽帧或后处理时，检查输入输出、编码格式和错误信息。
7. **交付报告**：同轻量路线，另给 composition 名与预览 URL。

## 质量与性能底线

- 预览优先于最终渲染；失败时保留可复现的错误、命令和项目状态，不伪造已交付。
- 两条路线共同底线：动画状态必须由帧/时间驱动；避免全局状态、每帧复制全部 props、无界缓存。长视频先做低成本关键帧或低清小样检查，再完整渲染。
- 重要文字必须可读：1080 宽画布的主标题约 84px、辅助文字约 44px 作为起点，并按比例缩放；关键内容留出约 80px 侧边和 100px 上下安全区。
- 原始素材、字幕 JSON、项目源代码和最终视频分开保存；示例和日志不写入真实密钥、身份或受限素材。

## 按需读取的专项资料

轻量路线：
- 页面契约、时间驱动规范、缓动与安全区：`{{SKILL_ROOT}}/references/lite-authoring.md`
- render_lite.py 用法、参数、故障排查、性能预期：`{{SKILL_ROOT}}/references/lite-render.md`
- 起点模板：`{{SKILL_ROOT}}/assets/lite-template.html`

Remotion 路线：
- 新项目、项目结构、Studio 启动：`{{SKILL_ROOT}}/references/project-setup.md`
- JSX、时间线、场景、素材、动画和交互编辑：`{{SKILL_ROOT}}/references/remotion-authoring.md`
- Playwright/Studio 预览、截图与验收：`{{SKILL_ROOT}}/references/browser-preview.md`
- MP4/WebM、透明视频、静帧和抽帧渲染：`{{SKILL_ROOT}}/references/render-delivery.md`
- 音视频元数据、字幕、旁白和 FFmpeg 边界：`{{SKILL_ROOT}}/references/media-captions.md`
