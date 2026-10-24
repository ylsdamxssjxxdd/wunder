# Remotion 编排

只在修改 composition、场景、素材、动画或可编辑参数时读取本文件。

## 画面与时间线

- 视频不是网页。每个场景先决定一个视觉重点，避免堆叠无关装饰。
- 关键文字和元素应留在安全区内。以 1080px 宽画布为例，侧边约 80px、上下约 100px；主标题约 84px、重要辅助文字约 44px 起步，并随 composition 宽度缩放。
- 连续场景按时间轴顺序组织。每个独立可编辑镜头、标题、叠层、音轨或字幕块都使用独立 JSX 节点，以便定位、修剪与替换。
- 先让每个场景单独可读，再加转场；转场不能遮蔽节奏、文字或声画同步。

## 帧驱动动画

从 `remotion` 取得当前帧和配置：

```tsx
import {AbsoluteFill, Easing, interpolate, useCurrentFrame, useVideoConfig} from 'remotion';

export const Title = ({children}: {children: string}) => {
  const frame = useCurrentFrame();
  const {fps} = useVideoConfig();
  return (
    <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
      <div
        style={{
          opacity: interpolate(frame, [0, 0.5 * fps], [0, 1], {
            extrapolateLeft: 'clamp',
            extrapolateRight: 'clamp',
            easing: Easing.bezier(0.16, 1, 0.3, 1),
          }),
        }}
      >
        {children}
      </div>
    </AbsoluteFill>
  );
};
```

- 所有秒到帧的计算基于 `useVideoConfig().fps`，例如 `2 * fps`，不要散落 30、60 等硬编码帧率。
- 动画使用 `useCurrentFrame()`、`interpolate()`、`spring()` 和 `Easing`。CSS `transition`、CSS `animation` 和 Tailwind `animate-*` 不是可靠的渲染时间线。
- 把会随帧变化的标题、透明度、滤镜或音量放进最小的组件，避免让整条 composition 的无关子树在每一帧重新渲染。
- 对 `scale` 使用 CSS `scale` 属性而非拼接 `transform` 字符串；用钳制插值避免越界。

## 素材与媒体

本地素材置于项目根 `public/`，并以 `staticFile()` 引用：

```tsx
import {Audio, Video} from '@remotion/media';
import {CanvasImage, staticFile} from 'remotion';

<Video src={staticFile('clip.mp4')} />
<Audio src={staticFile('voice.mp3')} />
<CanvasImage src={staticFile('cover.png')} />
```

- 用 `@remotion/media` 的 `<Video>`、`<Audio>` 处理时间线上媒体；图片使用 `<CanvasImage>`，动图/动画图像按所用浏览器和格式选择 Remotion 支持组件。
- 不要把大媒体、二进制资源或远程不稳定 URL 内嵌到源码。远程素材应有可复现来源，优先下载到受控的 `public/` 后引用。
- 多段视频或音频剪辑必须阅读项目现有的时间线结构；每段独立节点处理 `from`、`durationInFrames`、`trimBefore`、音量和淡入淡出。不要在单个组件内手工管理无界片段数组。

## Composition 与参数

- 注册 composition 前，确定 `id`、尺寸、fps、默认 props 和时长。避免修改已有 `id`，除非用户明确要求，因为它影响 Studio 链接和渲染命令。
- 需要让用户在 Studio 调整文本、颜色、尺寸或时间时，使用 Remotion 当前版本支持的 interactivity/schema，保持每个可编辑项目的值在其 JSX 调用处清晰可见。
- 动态时长、尺寸或输入 props 时使用 `calculateMetadata`，并验证预览与渲染得到同一结果。

## 专项能力

- 字幕、旁白和音频元数据：读取 `media-captions.md`。
- 3D、地图、音频可视化、特效、转场或复杂剪辑：先确认当前 Remotion 版本和项目是否已有相关依赖；用 `npx remotion add <package>` 安装同版本包，随后只实现用户需要的最小效果。
- 复杂专用 API 需要先查阅当前官方文档；不要根据过期记忆猜测组件或包版本。

