# Studio 浏览器预览与验收

只在需要通过 Playwright 浏览器确认 Remotion Studio 时读取本文件。

## 启动与连接

1. 在项目目录启动 `npx remotion studio --no-open`，并从实际输出取 URL。
2. 用 `browser(action="navigate", url=<Studio URL>)` 打开该地址。
3. 用 `browser(action="observe_refs")` 确认 Studio 已加载，并找到 composition、播放控制和错误信息的可访问性引用。
4. 页面导航、热更新或选 composition 后，旧 `ref` 失效，必须再次 `observe_refs`。

Studio 路由通常可访问 composition，但应使用运行时实际显示的 URL，不要假定端口或 composition 路径。若浏览器安全配置拒绝本机 URL，不要改安全配置或以别的方式绕过；记录准确 URL，改由用户本机浏览器预览。

## 验收方式

- 先检查 Studio 没有构建错误、composition 名称和画布尺寸正确、第一帧能显示。
- 对每个场景至少检查首帧、主内容出现后的帧、切场帧和末帧；长视频优先在 Studio 定位时间点或渲染少量帧图，不滚动播放后凭主观印象判断。
- 核对文字溢出、裁切、安全区、素材加载、视频比例、字幕可读性、音频静音/音量、场景衔接和末帧状态。
- 只在需要留存证据或视觉对比时使用 `browser(action="screenshot")`，并把截图写到工作区相对路径。截图不能证明音画同步或完整视频时长。

## 浏览器动作约束

- 使用 `navigate`、`observe_refs`、`observe_text`、`click`、`type`、`press`、`wait`、`screenshot` 和不超过 10 步的 `batch`；元素操作使用刚观察到的 `ref` 或一个稳定 selector。
- `wait` 优先等待可验证条件（URL、文本、选择器），不要使用任意长的固定休眠。
- `observe_text` 需要分页时沿用同一未变页面返回的 cursor；页面变化后重新观察。不要读取超出判断需要的页面正文。
- 浏览器仅支持 http/https，不支持 `file://`。不调用 `evaluate`、原始 JavaScript、旧 `browser_*` 工具或控制面接口。

## 不可用时的降级

当浏览器不可用时，仍可用 Studio 命令日志、类型检查和 `npx remotion render --frames` 的关键帧图片完成有限验证。明确说明没有完成交互预览或完整播放检查，不要宣称浏览器验收已通过。

