# 老浏览器与轻量 3D 参考

这份参考只在实现或排查 3D 网页时读取。它不要求使用完整引擎能力，优先解决单文件、低资源和可降级。

## 初始化骨架

```html
<canvas id="scene" aria-label="3D 场景"></canvas>
<div id="fallback" hidden>当前浏览器无法启用 3D，下面显示静态示意。</div>
<script>
(function () {
  var canvas = document.getElementById('scene');
  var fallback = document.getElementById('fallback');
  var gl = canvas && (canvas.getContext('webgl') || canvas.getContext('experimental-webgl'));
  if (!gl || typeof THREE === 'undefined') {
    if (canvas) { canvas.style.display = 'none'; }
    if (fallback) { fallback.hidden = false; }
    return;
  }
  var scene = new THREE.Scene();
  var camera = new THREE.PerspectiveCamera(42, 1, 0.1, 100);
  var renderer = new THREE.WebGLRenderer({ canvas: canvas, context: gl, antialias: false });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 1.5));
  camera.position.set(0, 0, 5);
  // Add low-poly geometry, reuse materials, then render only required frames.
}());
</script>
```

不要把这个片段原样当作成品；启动前要补齐 resize、降级、交互和资源清理。

## 兼容浏览器的 JavaScript

- 业务脚本使用 ES5 可读写法：`var`、普通函数、字符串拼接和显式条件。不要使用 `let/const`、箭头函数、模板字符串、`Promise`、模块和动态 import 作为必需能力。
- 用 `document.getElementById` 和事件监听，不依赖框架、构建产物或 polyfill CDN。`addEventListener` 不可用时可以停止增强并保留静态 HTML。
- 采用 `requestAnimationFrame`，并保留 `setTimeout` 低频重绘兜底。把 `resize` 处理合并到一次回调，避免连续事件反复分配渲染目标。

## 交互事件

鼠标和触摸分别注册，避免依赖 Pointer Events。记录开始点、上一点和当前缩放值；触摸结束、鼠标抬起、`mouseleave` 都要清除 `dragging`。只在指针实际移动时改变相机或模型，缩放值要夹在最小和最大范围之间。键盘至少提供重置视角或暂停动画的按钮，并给按钮设置 `aria-label`。

## 性能预算

- 首屏默认不超过约 20,000 个三角形、50 次绘制调用、1 个 renderer 和少量材质。复杂模型先合并静态几何或降采样。
- 不在 `animate` 中调用 `new THREE.*`、解析 JSON、创建纹理、遍历 DOM 或复制数组。对象创建集中在初始化和明确的状态切换中。
- 使用颜色材质和程序化几何替代大纹理。需要纹理时限制尺寸并内嵌为数据 URI；加载失败后使用纯色材质。
- `renderer.setSize` 只在实际尺寸变化时调用；设备像素比上限为 1.5。静态场景可以由交互事件触发 `render()`，不必常驻循环。

## 嵌入 GLB 的细节

- GLTFLoader（r124 非模块版）与 `three.min.js` 必须同版本；混用新版 loader 会拿到 `THREE.*` 里不存在的类，表现为首帧空白加一条 `x is not a constructor`。
- 用 `loader.parse(arrayBuffer, '', onLoad, onError)`。第二个参数是路径前缀，内嵌模型不需要它，传 `''`；`loader.load()` 会走 fetch，交付物里没有可请求的地址。
- base64 解码用 `atob` 后逐字节写入 `Uint8Array`。几 MB 的串一次性解码没问题，但不要每帧解码，也不要把 base64 写进 `innerHTML` 或日志。
- 解析成功后先归一化：`Box3().setFromObject` 取包围盒 → 居中 → 按最大边缩放到目标尺寸 → 底部落地 → 按尺寸设相机距离与 `near/far`。不同 DCC 导出的单位可能相差 100 倍，不归一化就只能看到一片空白或一个点。
- Draco / Meshopt / KTX2 压缩的 GLB 需要额外解码器，本技能不内置：遇到这类模型先要求未压缩版本，或退回程序化几何体，并在控制台说明原因。
- GLB 自带的贴图由 loader 解出，不产生网络请求；缺贴图时用 `baseColorFactor` 或自定义颜色兜底，不要因为发黑就判成渲染失败——先补环境光。

## 用浏览器工具验收

- 打开工作区页面：`navigate` 传 `path`（工作区相对路径），运行时从工作区读盘并通过内部预览域名提供，无需自建 HTTP 服务，也不受 `file://` 或明文 HTTP 限制。
- 安卓版直接用系统 WebView 的浏览器工具验收；proot 里的 Playwright/Chromium 受 GPU 进程限制，WebGL 上下文通常无法创建，不要把它当作渲染结论。
- 交互实测：`drag` 传 `from_x/from_y/to_x/to_y` 验证拖拽旋转；`scroll` 传 `delta_y`（可带 `x/y` 落在画布上）验证缩放。只读代码不能证明这两条能用。
- 控制台：`console` 返回该标签页的 log/warn/error 与未捕获异常；每条动作回执也带 `console_errors`。验收「控制台无异常」用这个，而不是靠肉眼。
- 截图摘要：`screenshot` 同轮返回 `frame`（`blank`、`distinct_colors`、`dominant_share`、`mean_luma`）。`blank: true` 或颜色数骤降时，先查画布尺寸、渲染循环、镜头位置与控制台，再谈画面细节。

## 降级与排错

WebGL 检测、renderer 构造、首次 render 都可能失败。每个阶段都应进入同一个降级函数：隐藏或冻结 3D 画布，显示 2D Canvas/DOM 示意、文本说明和重试/重置按钮。上下文丢失先暂停循环并等待 `webglcontextrestored`，约 2–3 秒仍不可用再降级；不要把 WebGL 错误全文输出给用户，详细信息可在开发时使用受控的 `console.warn`。

检查 HTML 时搜索 `src=`, `href=`, `url(`, `fetch(`, `XMLHttpRequest`, `import ` 和 `require(`。除明确允许的内嵌数据外，这些命中通常表示交付物仍依赖外部资源。
