---
name: 3D网页
description: "创建或修改以单个 HTML 文件交付的轻量 3D 网页。适用于需要在网页中生成、展示或交互操作 3D 模型，并且要求兼容较老浏览器、离线打开、低资源占用或不依赖构建工具的任务。"
---

# 3D 网页制作

这个技能指导智能体把 3D 场景、模型、交互和降级方案整合成一个可直接打开的 HTML 文件。最终只交付 `.html`；运行时不能再从 CDN、相邻文件或网络读取脚本、模型、纹理、字体和样式。

## 交付边界

- 先确认用户要看的主体、视角、交互和动效，再选择程序化模型、内嵌 GLB 或两者混合；不为简单几何体引入模型加载器。
- 输出一个自包含的 HTML：HTML、CSS、业务 JavaScript、3D 运行时、字体和小型资源全部内嵌。不要交付需要 `npm install`、打包器、服务器或额外目录才能运行的项目。
- 默认使用技能内置的 `assets/three.min.js`（Three.js r124，非模块脚本）。把它复制到最终 HTML 的 `<script>` 中，不写 `<script src>`、`import`、`require` 或 CDN 地址。内嵌时保留 `assets/LICENSE-THREE.txt` 的 MIT 许可文本；可用 `scripts/inline_three.py` 自动完成替换。
- 需要加载真实模型时，用内置的 `assets/GLTFLoader.js`（与 r124 配套的非模块版）解析内嵌 GLB，见下文「嵌入 GLB 模型」。
- 最终文件应支持 `file://` 直接打开。不要依赖 fetch、XHR、WebSocket 或动态跨域资源；确有外部数据需求时，改为交付时内嵌的静态数据。

## 制作流程

1. **规划模型**：把主体拆成少量可复用的几何体、材质和层级节点。规则形状优先使用低分段几何体，避免在每帧创建或修改大量对象。用 `Group` 表达部件关系，用 `userData` 保存交互标记。注意本技能内置的是 Three.js r124：`BoxGeometry`、`PlaneGeometry` 等仍是旧版 Geometry，`attributes` 为 `undefined`，会静默降级；源码里统一写 `BoxBufferGeometry`、`PlaneBufferGeometry`，或直接用 `BufferGeometry`。
2. **搭建场景**：创建 `Scene`、透视或正交相机、`WebGLRenderer`、环境光和一到两盏方向光。设置清晰的背景色和可读的材质；除非用户明确需要，避免阴影、后处理、粒子和高分辨率纹理。
3. **加入操作**：提供鼠标拖拽旋转、滚轮缩放，并用触摸事件覆盖单指旋转和双指缩放。使用普通函数、ES5 语法和显式事件监听，避免模块、异步语法、可选链、私有字段和仅新浏览器支持的 CSS。交互控件要有键盘可达性和 `aria-label`。
4. **安排动画**：只保留能表达状态变化的动画。使用 `requestAnimationFrame`，把时间步长限制在合理范围；静态场景应在交互或状态变化时渲染，持续动画应避免每帧扫描整个模型。禁止在动画循环中重复解析数据、创建材质或复制完整场景。
5. **做性能分级**：将设备像素比限制到 `Math.min(window.devicePixelRatio || 1, 1.5)`，监听 resize 时复用相机和 renderer。把默认三角形数控制在约 2 万以内、绘制调用控制在约 50 次以内；移动或老设备仍卡顿时降低分段、关闭抗锯齿或减少动画，而不是让页面失去响应。
6. **加入降级**：先检测 `canvas.getContext('webgl')` 或 `experimental-webgl`。WebGL 不可用、初始化失败或脚本加载异常时，显示一个清晰的 2D Canvas/DOM 静态示意和操作说明；降级分支不能引用未创建的 3D 对象，也不能让页面白屏。
7. **整合为单文件**：将业务脚本放在依赖之后，页面启动放在 `DOMContentLoaded` 或兼容的立即执行函数中。所有 ID、事件和错误提示保持唯一、可读；不在 HTML 中留下外部 `src`、`href`、`url()` 或字体引用。
8. **验收并交付**：用现代浏览器和至少一个旧版内核或兼容模式打开 HTML，检查首屏、拖拽、触摸、缩放、窗口变化、WebGL 禁用时的降级、键盘操作和长时间运行。检查控制台无未捕获异常；最后只提供 HTML 路径。

## 嵌入 GLB 模型

用户给了 `.glb`（或要求「用这个模型」）时按这条链路走，不要自己写 GLB 解析器：

1. **准备运行时**：页面里先有 Three.js，再有 GLTFLoader。源码态写 `<script src="../assets/three.min.js"></script>` 和 `<script src="../assets/GLTFLoader.js"></script>`，交付前用 `python scripts/inline_three.py 源.html 输出.html` 一次性内嵌两者（两个标签/标记都会替换）。
2. **内嵌模型**：`python scripts/inline_glb.py 模型.glb --html 源.html 输出.html`，脚本把 `window.WUNDER_GLB["模型名"] = { name, bytes, base64 }` 插在 `</head>` 之前，所以页面里的初始化脚本运行时模型已经就绪。多个模型直接并列传参，`--name` 按顺序改名。
3. **解析**：`base64` → `Uint8Array` → `ArrayBuffer`，交给 `new THREE.GLTFLoader().parse(buffer, '', onLoad, onError)`。不要用 `loader.load(url)`：交付物里没有可 fetch 的地址，`file://` 下也会被浏览器拦掉。
   ```js
   function glbBuffer(base64) {
     var binary = atob(base64), len = binary.length, bytes = new Uint8Array(len);
     for (var i = 0; i < len; i++) { bytes[i] = binary.charCodeAt(i); }
     return bytes.buffer;
   }
   var entry = window.WUNDER_GLB["模型名"];
   new THREE.GLTFLoader().parse(glbBuffer(entry.base64), '', function (gltf) {
     scene.add(gltf.scene);
     fitToView(gltf.scene);
   }, function (err) { showFallback(err); });
   ```
4. **姿态归一化**：模型来自不同工具，单位和朝向都不一致。用 `new THREE.Box3().setFromObject(model)` 取包围盒，把模型居中、按最大边缩放到目标尺寸、底部贴到地面，再按缩放后的尺寸定相机距离与 `near/far`；不要假设模型是 1 单位高或正面朝 +Z。
5. **材质与纹理**：GLB 自带的贴图由 GLTFLoader 解出（走 `TextureLoader` 的 `data:`/blob 路径，不需要网络）。缺贴图或 UV 时用颜色材质兜底，并保留 `gltf.scene` 的原始颜色；模型很暗时先加环境光或提亮材质，而不是直接判定「渲染失败」。
6. **体积预算**：单个 GLB 原始体积建议 ≤ 2 MB（base64 再增约 1/3），整个 HTML 控制在 8 MB 以内。超了先减面、去内嵌贴图、只留需要的动画轨，或改用程序化几何体；不要交付一个打不开的页面。
7. **动画（可选）**：`gltf.animations` 非空时用 `AnimationMixer` 播放，并在 `requestAnimationFrame` 里按帧间隔 `update(delta)`；不要为每条轨单独建定时器。
8. **失败降级**：`parse` 失败（版本过高、Draco/Meshopt 压缩、文件被截断）时显示静态 2D 示意并在控制台留一条可读原因；页面不能白屏，也不能把整段 base64 打到日志里。

## 预览与自检

- 预览工作区里的页面用浏览器工具的 `navigate` 加 `path`（工作区相对路径）：内置预览由浏览器后端直接从工作区读盘应答，不需要起 HTTP 服务，也不受 `file://` 或明文 HTTP 限制。安卓版用系统 WebView 承载同一预览域名；不要在 proot 里另装 Playwright/Chromium 做验收，那里的 GPU 进程通常被系统拦截，WebGL 会直接丢失。
- 3D 页面必须实测交互：`drag` 传 `from_x/from_y/to_x/to_y` 拖动画布验证旋转，`scroll` 传 `delta_y`（可加 `x/y` 定位到画布）验证缩放。
- `console` 取当前标签页的控制台输出与未捕获异常，用来兑现「控制台无异常」这条验收；每条动作回执里也会带 `console_errors`。
- `screenshot` 会在同一轮返回画面摘要（`blank`、`distinct_colors`、`mean_luma`）：`blank: true` 说明场景没渲染出来，先查画布尺寸、渲染循环和控制台，再去评价画面好不好看。

## 兼容性与稳定性要求

- 目标是没有 WebGL 时仍可读、没有触摸事件时仍可用、没有高分辨率屏幕时不改变布局。不要以 `backdrop-filter`、复杂滤镜、Web Components、WebGL2 专属特性或 CSS 变量作为必需能力。
- 事件坐标要同时处理 `clientX/clientY`、鼠标按钮和触摸列表；释放或离开画布时清除拖拽状态。滚轮事件使用 `wheel` 并提供旧浏览器可接受的 `mousewheel` 兜底，避免阻止页面全部滚动。
- 纹理和图片必须是小尺寸内嵌数据或程序生成的颜色；不把大模型、视频或字体直接塞进首屏。若确实需要二进制资源，先评估 HTML 体积，并在加载失败时回退到颜色材质。
- WebGL 上下文丢失、窗口尺寸为零、设备内存较低等情况都要安全处理：上下文丢失时先停止循环并尝试恢复（监听 `webglcontextrestored`，约 2–3 秒仍不可用再降级），尺寸为零时暂停渲染，不能持续报错占满 CPU。
- 用户提供的文本要通过 `textContent` 写入，不把未转义文本拼到 `innerHTML`；HTML 内联脚本中不要记录密钥、真实身份或具体业务数据。

## 参考与资源

- 需要核对浏览器 API、渲染性能和事件细节时，读取 [references/browser-and-3d.md](references/browser-and-3d.md)。
- 需要把 Three.js 和 GLTFLoader 自动嵌入现有 HTML 时，运行 `python scripts/inline_three.py 输入.html 输出.html`；标记形式为 `<script>` 内唯一的 `<!-- INLINE:three.min.js -->`，或 `<script src="...assets/three.min.js"></script>` 引用（examples 源码即此形式），GLTFLoader 同理。
- 需要把 GLB 内嵌进页面时，运行 `python scripts/inline_glb.py 模型.glb --html 输入.html 输出.html`（或 `--js 片段.js` 只取脚本）；体积提示会直接打在命令行上。
- `examples/` 中的示例是**源码态**：Three.js 通过相对路径引用 `../assets/three.min.js`，文件小、便于阅读修改，可直接打开查看效果，但单独拷走不能运行；对外交付前必须用 `inline_three.py` 构建成内嵌单文件。
- 运行时依赖与许可文件在 `assets/three.min.js`、`assets/GLTFLoader.js`、`assets/LICENSE-THREE.txt`。这些文件是生成 HTML 时使用的资源，不要把它们作为最终独立交付物。
- 需要一个完整参考时，读取并直接打开 [examples/水滴演示.html](examples/水滴演示.html)。它展示了连续金属曲面水滴、轨道相机、材质灯光、曲率航行和控制面板和 WebGL/2D 降级；可复制为用户任务的起点，再替换模型与文案。

## 输出前检查清单

- [ ] 最终只有一个可直接打开的 `.html` 文件。
- [ ] 没有外部脚本、模块导入、网络请求和未内嵌的模型/纹理/字体。
- [ ] 用到 GLB 时：GLTFLoader 与模型都已内嵌，`parse` 走内存且失败有降级；模型已居中、缩放到目标尺寸并按包围盒定相机。
- [ ] 3D 模型层级、相机、材质、灯光和交互均已初始化，窗口变化不会拉伸或报错。
- [ ] WebGL 不可用时有可读的 2D 降级；鼠标、触摸和键盘主操作均有反馈。
- [ ] 默认场景满足低面数、低绘制调用和有限像素比预算，动画不会无界增长对象或缓存。
- [ ] 已保留 Three.js MIT 许可文本，控制台无未捕获错误，截图摘要不是全空白。
