# 项目建立与 Studio

只在需要新建项目或排查项目启动时读取本文件。

## 选择目录

- 先检查目标目录及隐藏文件。已有 `package.json` 且依赖 Remotion 时，视为已有项目，不要重新脚手架。
- 空目录可原地初始化；非空且没有项目时，在其下建立语义清晰的子目录。不要删除 `.env`、`.git`、用户素材或其他非临时文件。
- 项目、素材和输出应位于用户工作区或用户明确指定位置；技能目录只保存说明，不是视频工程的工作目录。

## 创建项目

确认 Node.js、包管理器和网络策略可用后，可使用 Remotion 官方脚手架：

```sh
npx create-video@latest --yes --blank --no-tailwind <project-dir>
```

如已在空目标目录内，可把 `<project-dir>` 改为 `.`。脚手架完成后按项目锁定的包管理器安装依赖；不要在已有项目中随意切换 npm、pnpm、yarn 或 bun。

创建的最小项目应具备：

- `package.json` 和锁文件；
- source 入口（常见为 `src/index.ts`/`src/Root.tsx`）；
- 注册了稳定 `id`、`width`、`height`、`fps`、`durationInFrames` 的 `<Composition>`；
- `public/` 作为本地图片、音频和视频素材目录；
- 面向用户可编辑的场景组件，而不是单个无法拆分的巨大组件。

## Studio 优先

安装完成、且 composition 存在后立即启动 Studio：

```sh
npx remotion studio --no-open
```

记录命令输出的实际 URL。把长驻进程放进命令会话并在后续修改期间保持它运行；若它已经启动，复用现有 URL。使用 `browser(action="navigate")` 打开这个 URL，随后观察页面确认 Studio 和 composition 都已加载。

Studio 无法启动时，先检查 Node 版本、依赖安装、入口编译错误、端口占用和命令日志。不要因为 Studio 未启动就直接开始最终渲染。

## 规格默认值

用户未指定时，先根据用途选择并在交付时明确写出：

| 用途 | 推荐画布 | fps |
| --- | --- | --- |
| 横向演示 | 1920×1080 | 30 |
| 竖屏短视频 | 1080×1920 | 30 |
| 方形社媒 | 1080×1080 | 30 |

时长由明确脚本、镜头或音频决定。没有可靠时长依据时，先做可预览的短版 composition，不要虚构精确成片时长。

