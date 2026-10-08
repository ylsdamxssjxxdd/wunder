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

## 降级与排错

WebGL 检测、renderer 构造、首次 render 都可能失败。每个阶段都应进入同一个降级函数：隐藏或冻结 3D 画布，显示 2D Canvas/DOM 示意、文本说明和重试/重置按钮。不要把 WebGL 错误全文输出给用户；详细信息可在开发时使用受控的 `console.warn`。

检查 HTML 时搜索 `src=`, `href=`, `url(`, `fetch(`, `XMLHttpRequest`, `import ` 和 `require(`。除明确允许的内嵌数据外，这些命中通常表示交付物仍依赖外部资源。
