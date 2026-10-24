---
name: 视频制作
description: "创建、修改、预览、调试和导出程序化视频。适用于短视频、产品演示、动态图形、字幕视频、素材混剪、地图动画以及 MP4/WebM/透明视频交付。"
---

# 视频制作

把用户目标落成可复现、可继续编辑的视频工程，并在交付前完成预览或渲染验收。默认使用项目已有的 Remotion、Node 和依赖版本；没有现成项目时，按需读取本目录 `references/` 中的专项资料建立工程。

## 先判断交付目标

- 用户说“制作、创建或修改视频”时，先完成工程和预览；只有明确要求渲染、导出或 MP4 时才生成最终文件。
- 用户只要看效果时，启动 Studio，使用 `browser` 工具打开实际 URL，检查预览和关键时间点；截图只用于检查，不作为视频交付物。
- 用户明确要求导出时，先完成预览和最小质量检查，再渲染指定 composition。交付输出路径、格式、尺寸、帧率和未完成的检查。
- 用户已有项目时，先检查项目结构和当前改动，保留用户改动，不重置或覆盖无关文件。

## 工具协同

- 使用 `执行命令` 或命令会话运行 `npm`、`npx remotion`、媒体探测、测试和渲染命令，工作目录必须是用户项目目录。
- Linux 使用补充包中的 Node 16、Remotion 4 和对应架构 compositor；amd64 自带 Chrome for Testing 122，arm64 自带 Chromium 120，Remotion 与 browser 工具分别优先使用 `opt/browser/chrome-linux64/chrome` 或 `opt/browser/chrome-linux/chrome`。补充包运行时会为 compositor 的 FFmpeg 共享库设置 `LD_LIBRARY_PATH`。
- 使用 `读取文件`、`搜索内容`、`列出文件`检查项目和素材；不要把技能目录当作项目目录，也不要把素材复制进技能包。
- 浏览器启用时只使用单一 `browser` 工具：`navigate` → `observe_refs`/`observe_text` → 交互或 `wait` → `screenshot`。导航后旧 ref 失效，必须重新观察；批处理最多 10 步。
- Studio 使用 `http(s)` 地址；如果运行时策略阻止本机地址，说明原因并提供准确 URL，不要绕过安全限制。
- 需要读取专项资料时，用“技能调用”读取 `视频制作` 的 manifest，再按需读取 `{{SKILL_ROOT}}/references/<file>`，不要一次读取整个参考目录。

## 工作流

1. **明确规格**：确定画布比例和尺寸、fps、时长、受众、语气、场景顺序、文字、声音、素材来源、字幕语言和交付格式。缺少信息时采用合理默认值并记录。
2. **检查工程**：识别 `package.json`、Remotion 入口、composition、`public/` 素材、锁文件和已有脚本。没有工程时读取 `references/project-setup.md` 后创建最小可运行项目。
3. **实现时间线**：使用 `useCurrentFrame()`、`interpolate()` 和 Remotion timing 驱动动画；避免依赖 CSS `transition`、`animation` 或 Tailwind 动画类。场景、字幕和参数保持可编辑。
4. **按需增强**：字幕、旁白、音频可视化、地图、3D、转场、透明背景和动态时长分别读取对应 reference；简单标题不要引入额外依赖。
5. **预览验收**：启动 Studio，检查首帧、中段、末帧、场景切换、文字溢出、素材加载、音画同步和窄窗口；必要时用 `screenshot` 保存工作区相对路径。
6. **渲染交付**：用户明确要求时读取 `references/render-delivery.md`，渲染指定 composition。使用 FFmpeg 做媒体探测、抽帧或后处理时，检查输入输出、编码格式和错误信息。
7. **交付报告**：确认文件存在且体积非零，核对时长、尺寸、帧率和格式，给出工程目录、composition、预览 URL、产物路径、已做检查和可修改位置。

## 质量与性能底线

- 预览优先于最终渲染；失败时保留可复现的错误、命令和项目状态，不伪造已交付。
- 动画状态必须由帧驱动；避免全局状态、深层 watch、每帧复制全部 props 和无界缓存。长视频先做低成本关键帧或抽帧检查，再完整渲染。
- 重要文字必须可读：1080 宽画布的主标题约 84px、辅助文字约 44px 作为起点，并按比例缩放；关键内容留出约 80px 侧边和 100px 上下安全区。
- 原始素材、字幕 JSON、项目源代码和最终视频分开保存；示例和日志不写入真实密钥、身份或受限素材。

## 按需读取的专项资料

- 新项目、项目结构、Studio 启动：`{{SKILL_ROOT}}/references/project-setup.md`
- JSX、时间线、场景、素材、动画和交互编辑：`{{SKILL_ROOT}}/references/remotion-authoring.md`
- Playwright/Studio 预览、截图与验收：`{{SKILL_ROOT}}/references/browser-preview.md`
- MP4/WebM、透明视频、静帧和抽帧渲染：`{{SKILL_ROOT}}/references/render-delivery.md`
- 音视频元数据、字幕、旁白和 FFmpeg 边界：`{{SKILL_ROOT}}/references/media-captions.md`
