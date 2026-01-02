# PPT 维护说明

本目录使用纯静态方式组织 PPT。每一页都是一个独立的 JS 文件，页面清单由 `manifest.js` 维护，通过动态 import 按顺序加载，方便单独新增、删除或调整顺序。

## 页面结构

- 入口页面：`docs/ppt/index.html`（只负责容器与 HUD）
- 启动脚本：`docs/ppt/slides/boot.js`（读取清单、动态加载页面、再加载翻页逻辑）
- 页面清单：`docs/ppt/slides/manifest.js`（页面顺序与路径在这里维护）
- 单页内容：`docs/ppt/slides/*.js`（每页一个文件，名称可自定义）
- 样式文件：`docs/ppt/styles.css`

## 新增页面

1. 复制任意一个单页文件作为模板，重命名为新的文件名（可用语义化名称）。
2. 修改 `data-title` 和页面内容。
3. 在 `docs/ppt/slides/manifest.js` 里新增该文件路径，并放到合适的位置。
4. 如涉及目录页或章节页，请同步更新对应内容。

## 删除页面

1. 删除对应的单页文件。
2. 从 `docs/ppt/slides/manifest.js` 移除该页面路径。
3. 若目录页或章节索引引用了该页，记得一并调整。

## 修改页面内容

- 每页必须以 `<section class="slide ...">` 作为根节点。
- 不要手动添加 `active` 类，翻页脚本会自动控制当前页。
- 需要分步出现的元素加上 `class="fragment"` 即可。
- 单页文件默认导出一个函数（`export default function`），返回该页的 DOM 元素。

## 资源与路径

- 图片、图标等资源统一放到 `docs/ppt/assets/`。
- 页面内使用相对路径引用，如：`assets/xxx.png`。
- 保持纯静态，不引入外部依赖，确保离线可用。

## 注意事项

- 单页内容使用模板字符串包裹，若需要展示 `${` 字符，请用 `&#36;{` 或 `\${` 代替。
- 页面样式统一在 `docs/ppt/styles.css` 内维护。
- 翻页逻辑位于 `docs/ppt/app.js`，一般无需修改。
