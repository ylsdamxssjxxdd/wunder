---
title: 桌面控制
summary: 桌面控制器和桌面监视器提供完整的桌面自动化能力，基于坐标框执行桌面动作和截图观察。
read_when:
  - 你要在 Desktop 模式下自动化桌面操作
  - 你需要了解 bbox 坐标框的使用方法
source_docs:
  - src/services/tools/desktop_control.rs
  - src/services/tools/catalog.rs
---

# 桌面控制

桌面自动化由两个核心工具组成：
- **桌面控制器**：执行各种桌面动作
- **桌面监视器**：观察桌面状态变化

---

## 桌面控制器

### 功能说明

基于 `bbox + action` 执行桌面操作，支持鼠标、键盘、拖拽等完整操作。

**别名**：
- `desktop_controller`
- `desktop_control`
- `desktop`

### 参数说明

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `bbox` | array | ✅ | 目标区域坐标，支持 2 或 4 个整数 |
| `action` | string | ✅ | 要执行的动作 |
| `description` | string | ✅ | 动作说明，用于日志和审计 |
| `key` | string | ❌ | 按键名称（press_key 动作使用） |
| `text` | string | ❌ | 要输入的文本（type_text 动作使用） |
| `delay_ms` | integer | ❌ | 延迟毫秒数 |
| `duration_ms` | integer | ❌ | 持续时间毫秒数 |
| `scroll_steps` | integer | ❌ | 滚动步数 |
| `to_bbox` | array | ❌ | 拖拽目标坐标（drag_drop 动作使用） |

### 支持的动作

| 动作 | 说明 | 必填附加参数 |
|------|------|--------------|
| `left_click` | 左键单击 | - |
| `left_double_click` | 左键双击 | - |
| `right_click` | 右键单击 | - |
| `middle_click` | 中键单击 | - |
| `left_hold` | 左键按住 | `duration_ms` |
| `right_hold` | 右键按住 | `duration_ms` |
| `middle_hold` | 中键按住 | `duration_ms` |
| `left_release` | 左键释放 | - |
| `right_release` | 右键释放 | - |
| `middle_release` | 中键释放 | - |
| `scroll_down` | 向下滚动 | `scroll_steps` |
| `scroll_up` | 向上滚动 | `scroll_steps` |
| `press_key` | 按键 | `key` |
| `type_text` | 输入文本 | `text` |
| `delay` | 延迟等待 | `delay_ms` |
| `move_mouse` | 移动鼠标 | - |
| `drag_drop` | 拖拽 | `to_bbox` |

### 使用示例

#### 左键单击
```json
{
  "bbox": [100, 200, 300, 400],
  "action": "left_click",
  "description": "点击开始按钮"
}
```

#### 输入文本
```json
{
  "bbox": [500, 300],
  "action": "type_text",
  "text": "Hello World!",
  "description": "在输入框中输入文本"
}
```

#### 按键操作
```json
{
  "bbox": [200, 150],
  "action": "press_key",
  "key": "enter",
  "description": "按下回车键"
}
```

#### 拖拽操作
```json
{
  "bbox": [100, 100, 200, 200],
  "action": "drag_drop",
  "to_bbox": [300, 300, 400, 400],
  "description": "拖拽文件到目标位置"
}
```

---

## 桌面监视器

### 功能说明

等待指定时间后返回桌面截图，用于观察桌面状态变化。

**别名**：
- `desktop_monitor`
- `desktop_screenshot`

### 参数说明

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `wait_ms` | integer | ✅ | 等待毫秒数，最大 30000 毫秒（30 秒） |
| `note` | string | ❌ | 观察说明 |

### 使用示例

```json
{
  "wait_ms": 2000,
  "note": "等待 2 秒后截图观察界面变化"
}
```

---

## 完整智能体循环示例

### 打开记事本并输入文本

```json
// 1. 先截图确认当前状态
{
  "wait_ms": 500,
  "note": "初始状态观察"
}

// 2. 点击开始菜单
{
  "bbox": [50, 1050, 150, 1080],
  "action": "left_click",
  "description": "点击开始菜单"
}

// 3. 等待菜单打开
{
  "wait_ms": 1000,
  "note": "等待菜单打开"
}

// 4. 输入记事本
{
  "bbox": [100, 200],
  "action": "type_text",
  "text": "记事本",
  "description": "搜索记事本"
}

// 5. 按回车打开
{
  "bbox": [150, 250],
  "action": "press_key",
  "key": "enter",
  "description": "打开记事本"
}

// 6. 等待记事本打开
{
  "wait_ms": 2000,
  "note": "等待记事本打开"
}

// 7. 在记事本中输入文本
{
  "bbox": [300, 300, 800, 600],
  "action": "type_text",
  "text": "Hello from wunder!",
  "description": "在记事本中输入文本"
}

// 8. 截图确认结果
{
  "wait_ms": 1000,
  "note": "确认操作完成观察"
}
```

---

## 注意事项

1. **`bbox` 坐标格式说明：
   - 4 个整数：`[x1, y1, x2, y2]` - 矩形区域
   - 2 个整数：`[x, y]` - 点坐标

2. **`description` 必须填写**：
   - 桌面操作比文件工具更危险
   - 便于日志、审计和后续排障

3. **可见性限制**：
   - 仅在 Desktop 模式下可用
   - Server 和 CLI 模式不可用

4. **安全提示**：
   - 操作前先用 `桌面监视器` 确认状态
   - 操作后再次确认结果

---

## 延伸阅读

- [浏览器](/docs/zh-CN/tools/browser/)
- [Desktop 界面](/docs/zh-CN/surfaces/desktop-ui/)
