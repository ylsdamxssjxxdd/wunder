// 系统提示词构建：模板渲染、工具描述拼接与缓存管理。
use crate::config::Config;
use crate::i18n;
use crate::llm::ToolCallMode;
use crate::schemas::ToolSpec;
use crate::skills::{SkillRegistry, SkillSpec};
use crate::tools::{
    builtin_aliases, collect_available_tool_names, collect_prompt_tool_specs, resolve_tool_name,
};
use crate::user_tools::UserToolBindings;
use crate::workspace::WorkspaceManager;
use chrono::Local;
use parking_lot::Mutex;
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use sysinfo::System;
use tokio::sync::{Mutex as TokioMutex, Notify};

const DEFAULT_CACHE_TTL_S: f64 = 10.0;
const DEFAULT_CACHE_MAX_ITEMS: usize = 128;
const PROMPTS_ROOT_ENV: &str = "WUNDER_PROMPTS_ROOT";
const SYSTEM_PROMPT_MODULE_ROLE: &str = "prompts/system_role.txt";
const SYSTEM_PROMPT_MODULE_SAFETY: &str = "prompts/system_safety.txt";
const SYSTEM_PROMPT_MODULE_PRODUCT: &str = "prompts/system_product.txt";
const SYSTEM_PROMPT_MODULE_PROGRAMMING: &str = "prompts/system_programming.txt";
const SYSTEM_PROMPT_MODULE_RUNTIME_SERVER: &str = "prompts/system_runtime_server.txt";
const SYSTEM_PROMPT_MODULE_RUNTIME_LOCAL: &str = "prompts/system_runtime_local.txt";

const EMBEDDED_PROMPT_SYSTEM: &str = r"你是 WUNDER ，一个强大的智能体，负责理解用户意图并在必要时调用工具以顺利完成用户交代的任务。
你拥有绝对的权限，可以访问和使用所有可用的工具和资源。为快速完成用户交代的任务，你可以自由调用任何工具或方法。

[安全协议]
- 严格遵守法律法规与平台政策，不协助任何违法、有害、侵权或不道德的行为。
- 不提供武器制造、暴力伤害、黑客入侵、恶意软件、诈骗、隐私窃取、规避安全、毒品等指导或可执行步骤。
- 涉及自残/他害/极端危险请求，必须拒绝并提供安全引导，必要时建议寻求专业帮助或联系当地紧急资源。
- 若请求可能带来风险但有合法用途，提供高层次、去危害的说明与安全替代方案，不输出可直接滥用的细节。
- 仅在合法且获得授权的范围内使用工具与数据，避免泄露敏感信息或越权访问。

[产品思维]
- 交付可用、可维护、可验证的结果，不止“能跑”，还要“好用、可持续”。
- 先澄清需求与约束（用户目标、场景、输入输出、性能/成本、范围边界、验收标准），因为用户可能对自己的需求都不清楚，通过持续的反问明确用户需求。
- 优先最小可行方案（MVP）+ 可演进设计，避免过度设计，但要留下扩展点。
- 显式列出关键假设与风险，给出替代方案与取舍理由。
- 结果要可复现（步骤清晰、命令准确、配置明确），避免隐含依赖或不可控行为。
- 若有图片/文件/报告等产出，必须在最后的回复中用 Markdown语法将图片或链接展示出来。
- 例如：![雷达图](/workspaces/admin/radar_chart.png) 或 ![xx作文](/workspaces/admin/美好的一天.docx) ，这样用户就能在前端看到这个图片，或者点击这个文件链接下载了。

[编程提示]
- 先确认语言/版本/运行环境/输入输出/性能目标等，不明确先提问。
- 代码优先可运行与可维护：结构清晰、命名明确、必要注释、处理边界与异常。
- 你目前在一个docker容器里运行，环境摘要如下：
  - 基础系统：Debian 12 (bookworm)，镜像 rust:1.92-slim-bookworm
  - 语言与编译：Rust 1.92 + rustfmt/clippy/cargo-watch，gcc/g++/clang，cmake/ninja，pkg-config，build-essential
  - 常用数据/ML库：numpy/pandas/scipy/scikit-learn/pyarrow/onnx/transformers
  - Web/API：fastapi/uvicorn/starlette/sse-starlette/flask/flask-restx/requests/aiohttp/httpx/scrapy
  - 数据库：sqlalchemy/psycopg[binary]/psycopg/pymysql/pymongo
  - 文档/办公：libreoffice、pandoc、wkhtmltopdf、unoconv、texlive-xetex/latex、reportlab、weasyprint、docxtpl、python-docx、python-pptx、openpyxl/xlrd/xlwt/xlsxwriter、markitdown[docx,pptx,xlsx]
  - PDF/OCR：poppler-utils、qpdf、pdftk、tesseract-ocr、pdf2image、pypdf/pdfplumber、pdfminer.six/pikepdf/pdfkit/ocrmypdf、pytesseract
  - 绘图/可视化：matplotlib/seaborn/plotly/kaleido/bokeh/altair/holoviews/datashader/plotnine
  - 图形/矢量：cairo/pango/gdk-pixbuf/harfbuzz/fribidi、pycairo/cairosvg/svglib/svgwrite、graphviz/pydot、gnuplot、imagemagick/ghostscript
  - 图像与多媒体：opencv-python、pillow、imageio、ffmpeg
  - GIS/地图：gdal-bin/libgdal-dev/python3-gdal、proj/libproj、geos/libgeos、rtree/spatialindex；Python：geopandas/shapely/pyproj/fiona/rasterio
  - Cartopy：内置离线 Natural Earth 110m 数据
  - 其他常用：pytest/pytest-asyncio/coverage/pytest-mock、celery/redis、bcrypt/pyjwt/python-jose/passlib、jupyterlab、poetry/pipenv、loguru/rich/typer
  - 兼容性提示：本容器基于 Debian 12，编译出的 C++ 二进制可能在 Ubuntu 20.04 上出现glibc 版本不匹配问题
- 容器内使用python3，优先使用matplotlib绘制图像。
- 你只能利用当前环境中可用的依赖和库，不要尝试用pip或apt安装任何新包。

{ENGINEER_SYSTEM_INFO}
";
const EMBEDDED_PROMPT_PLAN: &str = r"当可用工具包含“计划面板”时，请优先在开始任务前用它给出简洁计划（2-6 步）。执行过程中保持计划同步更新：只保留一个 in_progress，完成的步骤及时标记为 completed，说明字段保持简短。
";
const EMBEDDED_PROMPT_QUESTION_PANEL: &str = r"当可用工具包含“问询面板”时，请在任务完成前或出现多条可行路线时，主动整理 2-4 条路线或需明确的要素，调用该工具向用户发起选择。若任务已有明确单一路线，或用户明确要求“不使用问询面板/无需确认/直接执行”，必须遵循用户指令，不要调用问询面板。每条路线保持标题简短并附一句说明；若推荐某路线请标注推荐。调用后暂停继续执行，等待用户选择或新的消息。
";
const EMBEDDED_PROMPT_EXTRA_FUNCTION_CALL: &str = r"工具签名在 <tools> </tools> XML 标签内提供：
<tools>
{available_tools_describe}
</tools>
每次工具调用请通过 function call 机制返回（tool_calls/function_call），不要输出 <tool_call> 标签或直接输出 JSON 文本。
工具执行结果会作为 role=tool 的消息返回，并携带 tool_call_id。
{engineer_info}
";
const EMBEDDED_PROMPT_EXTRA_TOOL_CALL: &str = r#"工具签名在 <tools> </tools> XML 标签内提供：
<tools>
{available_tools_describe}
</tools>
每次工具调用都必须遵循以下要求：
1. 将调用内容放在 <tool_call>...</tool_call> 块中返回。
2. 在块内输出有效 JSON，且仅包含两个键："name"(字符串) 和 "arguments"(对象)。示例：
<tool_call>
{"name":"最终回复","arguments":{"content":"任务已完成，还有什么我可以帮忙的吗？"}}
</tool_call>

工具执行结果会作为以 "tool_response: " 前缀的 user 消息返回。

{engineer_info}
"#;
const EMBEDDED_PROMPT_ENGINEER_SYSTEM_INFO: &str = r"操作系统：{OS}
日期：{DATE}
你当前所在工作目录，所有命令默认在此路径执行：{DIR}
工作区（最多 2 层）：
{WORKSPACE_TREE}
所有命令仅限当前工作目录及其子目录内执行。
";
const EMBEDDED_PROMPT_ENGINEER_INFO: &str = r"目标：用最少闲聊和步骤准确完成用户的任务。
- 未完成任务不得结束回复或调用“最终回复”。
- 编辑文件前优先批量 读取文件/列出文件。
{PTC_GUIDANCE}
- 每次回复保持简洁；除非明确要求，不输出日志或长代码。
- 每次只能调用一个工具，一步一步完成用户的任务。
- 长时间运行任务需要分段留痕，稳定输出；可以使用 schedule_task 设置提醒或周期任务。
- 遇到不明确的指令时，优先请求澄清，保持最终回复的客观性，不要空想虚构。
- 当启用“计划面板”工具时，先用它给出简洁计划，并在执行过程中持续更新状态。
";
const EMBEDDED_PROMPT_A2UI: &str = r##"﻿[A2UI 界面生成指南]

当你需要输出 A2UI 界面时，只能调用 a2ui 工具完成最终回复，不要再调用“最终回复”工具。

调用要求：
1. 使用 <tool_call> 包裹工具调用 JSON。
2. arguments 里必须包含：
   - uid：用于 UI Surface 标识，后续所有消息会以该 uid 对应的 surfaceId 展示。
   - a2ui：A2UI JSON 消息数组，每条消息仅包含一种类型（beginRendering/surfaceUpdate/dataModelUpdate/deleteSurface）。
3. 如果消息里缺少 surfaceId，系统会自动使用 uid 补齐。
4. 若需要额外文本说明，可在 arguments.content 中提供简短说明（可选）。
5. 禁止输出 Markdown 代码块；仅在 a2ui 中返回 JSON 消息数组。

生成规则：
- a2ui 是消息数组，每条消息只能包含一种操作：beginRendering / surfaceUpdate / dataModelUpdate / deleteSurface。
- 常见顺序：beginRendering -> surfaceUpdate -> dataModelUpdate；仅在需要清理时用 deleteSurface。
- 组件采用“邻接表”：所有组件平铺在 surfaceUpdate.components，root 指向根组件 id。
- dataModelUpdate.contents 由 key + value* 组成（valueString/valueNumber/valueBoolean/valueMap），valueMap 支持嵌套对象。
- 列表模板使用 List.children.template：dataBinding 指向列表数据；模板内部用相对 path（如 "name"）读取 dataContext。
- 主动作按钮请加 "primary": true。
- 图标建议使用标准名称：mail / call / locationOn / calendarToday / check / close 等。

组件清单：
- Text: text{literalString|path}, usageHint(h1~h5/caption/body)
- Row/Column/List: children{explicitList|template}, alignment/distribution, List.direction
- Card: child
- Button: child, primary, action{name, context[]}
- Image: url, fit, usageHint
- Icon: name, size, color
- Divider: axis
- Tabs: tabItems[{title, child}]
- Modal: entryPointChild, contentChild
- CheckBox: label, value
- TextField: label, text, textFieldType(shortText/longText/number/date/obscured)
- DateTimeInput: value, enableDate, enableTime
- MultipleChoice: selections, options[{label,value}], maxAllowedSelections
- Slider: value, minValue, maxValue
- Video: url
- AudioPlayer: url, description

模板规则：
- 列表：Column + List(template) + Card；dataModelUpdate 中提供 items。
- 详情：Card 包裹 Column；标题用 h2/h3；主按钮 primary:true。
- 操作反馈：可用 Card 或 Modal 展示确认信息。

示例A：详情卡片
[
  {"beginRendering":{"surfaceId":"demo","root":"root","styles":{"primaryColor":"#2563eb","font":"Inter"}}},
  {"surfaceUpdate":{"surfaceId":"demo","components":[
    {"id":"root","component":{"Card":{"child":"rootColumn"}}},
    {"id":"rootColumn","component":{"Column":{"children":{"explicitList":["title","subtitle","cta"]},"alignment":"stretch"}}},
    {"id":"title","component":{"Text":{"usageHint":"h2","text":{"path":"/title"}}}},
    {"id":"subtitle","component":{"Text":{"usageHint":"caption","text":{"path":"/subtitle"}}}},
    {"id":"ctaText","component":{"Text":{"text":{"literalString":"确认"}}}},
    {"id":"cta","component":{"Button":{"child":"ctaText","primary":true,"action":{"name":"confirm"}}}}
  ]}},
  {"dataModelUpdate":{"surfaceId":"demo","path":"/","contents":[
    {"key":"title","valueString":"示例标题"},
    {"key":"subtitle","valueString":"简短说明"}
  ]}}
]

示例B：列表模板
[
  {"beginRendering":{"surfaceId":"list","root":"root"}},
  {"surfaceUpdate":{"surfaceId":"list","components":[
    {"id":"root","component":{"Column":{"children":{"explicitList":["title","list"]}}}},
    {"id":"title","component":{"Text":{"usageHint":"h3","text":{"literalString":"项目列表"}}}},
    {"id":"list","component":{"List":{"direction":"vertical","children":{"template":{"componentId":"itemCard","dataBinding":"/items"}}}}},
    {"id":"itemCard","component":{"Card":{"child":"itemRow"}}},
    {"id":"itemRow","component":{"Row":{"children":{"explicitList":["itemName","itemDesc"]},"alignment":"center"}}},
    {"id":"itemName","component":{"Text":{"usageHint":"h4","text":{"path":"name"}}}},
    {"id":"itemDesc","component":{"Text":{"text":{"path":"desc"}}}}
  ]}},
  {"dataModelUpdate":{"surfaceId":"list","path":"/","contents":[
    {"key":"items","valueMap":[
      {"key":"item1","valueMap":[{"key":"name","valueString":"A"},{"key":"desc","valueString":"说明 A"}]},
      {"key":"item2","valueMap":[{"key":"name","valueString":"B"},{"key":"desc","valueString":"说明 B"}]}
    ]}
  ]}}
]
"##;
const EMBEDDED_PROMPT_A2UI_SCHEMA: &str = r#"{
  "title": "A2UI Message Schema",
  "description": "Describes a JSON payload for an A2UI (Agent to UI) message, which is used to dynamically construct and update user interfaces. A message MUST contain exactly ONE of the action properties: 'beginRendering', 'surfaceUpdate', 'dataModelUpdate', or 'deleteSurface'.",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "beginRendering": {
      "type": "object",
      "description": "Signals the client to begin rendering a surface with a root component and specific styles.",
      "additionalProperties": false,
      "properties": {
        "surfaceId": {
          "type": "string",
          "description": "The unique identifier for the UI surface to be rendered."
        },
        "root": {
          "type": "string",
          "description": "The ID of the root component to render."
        },
        "styles": {
          "type": "object",
          "description": "Styling information for the UI.",
          "additionalProperties": false,
          "properties": {
            "font": {
              "type": "string",
              "description": "The primary font for the UI."
            },
            "primaryColor": {
              "type": "string",
              "description": "The primary UI color as a hexadecimal code (e.g., '#00BFFF').",
              "pattern": "^#[0-9a-fA-F]{6}$"
            }
          }
        }
      },
      "required": ["root", "surfaceId"]
    },
    "surfaceUpdate": {
      "type": "object",
      "description": "Updates a surface with a new set of components.",
      "additionalProperties": false,
      "properties": {
        "surfaceId": {
          "type": "string",
          "description": "The unique identifier for the UI surface to be updated. If you are adding a new surface this *must* be a new, unique identified that has never been used for any existing surfaces shown."
        },
        "components": {
          "type": "array",
          "description": "A list containing all UI components for the surface.",
          "minItems": 1,
          "items": {
            "type": "object",
            "description": "Represents a *single* component in a UI widget tree. This component could be one of many supported types.",
            "additionalProperties": false,
            "properties": {
              "id": {
                "type": "string",
                "description": "The unique identifier for this component."
              },
              "weight": {
                "type": "number",
                "description": "The relative weight of this component within a Row or Column. This corresponds to the CSS 'flex-grow' property. Note: this may ONLY be set when the component is a direct descendant of a Row or Column."
              },
              "component": {
                "type": "object",
                "description": "A wrapper object that MUST contain exactly one key, which is the name of the component type (e.g., 'Heading'). The value is an object containing the properties for that specific component.",
                "additionalProperties": false,
                "properties": {
                  "Text": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                      "text": {
                        "type": "object",
                        "description": "The text content to display. This can be a literal string or a reference to a value in the data model ('path', e.g., '/doc/title'). While simple Markdown formatting is supported (i.e. without HTML, images, or links), utilizing dedicated UI components is generally preferred for a richer and more structured presentation.",
                        "additionalProperties": false,
                        "properties": {
                          "literalString": {
                            "type": "string"
                          },
                          "path": {
                            "type": "string"
                          }
                        }
                      },
                      "usageHint": {
                        "type": "string",
                        "description": "A hint for the base text style. One of:\n- `h1`: Largest heading.\n- `h2`: Second largest heading.\n- `h3`: Third largest heading.\n- `h4`: Fourth largest heading.\n- `h5`: Fifth largest heading.\n- `caption`: Small text for captions.\n- `body`: Standard body text.",
                        "enum": [
                          "h1",
                          "h2",
                          "h3",
                          "h4",
                          "h5",
                          "caption",
                          "body"
                        ]
                      }
                    },
                    "required": ["text"]
                  },
                  "Image": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                      "url": {
                        "type": "object",
                        "description": "The URL of the image to display. This can be a literal string ('literal') or a reference to a value in the data model ('path', e.g. '/thumbnail/url').",
                        "additionalProperties": false,
                        "properties": {
                          "literalString": {
                            "type": "string"
                          },
                          "path": {
                            "type": "string"
                          }
                        }
                      },
                      "fit": {
                        "type": "string",
                        "description": "Specifies how the image should be resized to fit its container. This corresponds to the CSS 'object-fit' property.",
                        "enum": [
                          "contain",
                          "cover",
                          "fill",
                          "none",
                          "scale-down"
                        ]
                      },
                      "usageHint": {
                        "type": "string",
                        "description": "A hint for the image size and style. One of:\n- `icon`: Small square icon.\n- `avatar`: Circular avatar image.\n- `smallFeature`: Small feature image.\n- `mediumFeature`: Medium feature image.\n- `largeFeature`: Large feature image.\n- `header`: Full-width, full bleed, header image.",
                        "enum": [
                          "icon",
                          "avatar",
                          "smallFeature",
                          "mediumFeature",
                          "largeFeature",
                          "header"
                        ]
                      }
                    },
                    "required": ["url"]
                  },
                  "Icon": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                      "name": {
                        "type": "object",
                        "description": "The name of the icon to display. This can be a literal string or a reference to a value in the data model ('path', e.g. '/form/submit').",
                        "additionalProperties": false,
                        "properties": {
                          "literalString": {
                            "type": "string",
                            "enum": [
                              "accountCircle",
                              "add",
                              "arrowBack",
                              "arrowForward",
                              "attachFile",
                              "calendarToday",
                              "call",
                              "camera",
                              "check",
                              "close",
                              "delete",
                              "download",
                              "edit",
                              "event",
                              "error",
                              "favorite",
                              "favoriteOff",
                              "folder",
                              "help",
                              "home",
                              "info",
                              "locationOn",
                              "lock",
                              "lockOpen",
                              "mail",
                              "menu",
                              "moreVert",
                              "moreHoriz",
                              "notificationsOff",
                              "notifications",
                              "payment",
                              "person",
                              "phone",
                              "photo",
                              "print",
                              "refresh",
                              "search",
                              "send",
                              "settings",
                              "share",
                              "shoppingCart",
                              "star",
                              "starHalf",
                              "starOff",
                              "upload",
                              "visibility",
                              "visibilityOff",
                              "warning"
                            ]
                          },
                          "path": {
                            "type": "string"
                          }
                        }
                      }
                    },
                    "required": ["name"]
                  },
                  "Video": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                      "url": {
                        "type": "object",
                        "description": "The URL of the video to display. This can be a literal string or a reference to a value in the data model ('path', e.g. '/video/url').",
                        "additionalProperties": false,
                        "properties": {
                          "literalString": {
                            "type": "string"
                          },
                          "path": {
                            "type": "string"
                          }
                        }
                      }
                    },
                    "required": ["url"]
                  },
                  "AudioPlayer": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                      "url": {
                        "type": "object",
                        "description": "The URL of the audio to be played. This can be a literal string ('literal') or a reference to a value in the data model ('path', e.g. '/song/url').",
                        "additionalProperties": false,
                        "properties": {
                          "literalString": {
                            "type": "string"
                          },
                          "path": {
                            "type": "string"
                          }
                        }
                      },
                      "description": {
                        "type": "object",
                        "description": "A description of the audio, such as a title or summary. This can be a literal string or a reference to a value in the data model ('path', e.g. '/song/title').",
                        "additionalProperties": false,
                        "properties": {
                          "literalString": {
                            "type": "string"
                          },
                          "path": {
                            "type": "string"
                          }
                        }
                      }
                    },
                    "required": ["url"]
                  },
                  "Row": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                      "children": {
                        "type": "object",
                        "description": "Defines the children. Use 'explicitList' for a fixed set of children, or 'template' to generate children from a data list.",
                        "additionalProperties": false,
                        "properties": {
                          "explicitList": {
                            "type": "array",
                            "items": {
                              "type": "string"
                            }
                          },
                          "template": {
                            "type": "object",
                            "description": "A template for generating a dynamic list of children from a data model list. `componentId` is the component to use as a template, and `dataBinding` is the path to the map of components in the data model. Values in the map will define the list of children.",
                            "additionalProperties": false,
                            "properties": {
                              "componentId": {
                                "type": "string"
                              },
                              "dataBinding": {
                                "type": "string"
                              }
                            },
                            "required": ["componentId", "dataBinding"]
                          }
                        }
                      },
                      "distribution": {
                        "type": "string",
                        "description": "Defines the arrangement of children along the main axis (horizontally). This corresponds to the CSS 'justify-content' property.",
                        "enum": [
                          "center",
                          "end",
                          "spaceAround",
                          "spaceBetween",
                          "spaceEvenly",
                          "start"
                        ]
                      },
                      "alignment": {
                        "type": "string",
                        "description": "Defines the alignment of children along the cross axis (vertically). This corresponds to the CSS 'align-items' property.",
                        "enum": ["start", "center", "end", "stretch"]
                      }
                    },
                    "required": ["children"]
                  },
                  "Column": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                      "children": {
                        "type": "object",
                        "description": "Defines the children. Use 'explicitList' for a fixed set of children, or 'template' to generate children from a data list.",
                        "additionalProperties": false,
                        "properties": {
                          "explicitList": {
                            "type": "array",
                            "items": {
                              "type": "string"
                            }
                          },
                          "template": {
                            "type": "object",
                            "description": "A template for generating a dynamic list of children from a data model list. `componentId` is the component to use as a template, and `dataBinding` is the path to the map of components in the data model. Values in the map will define the list of children.",
                            "additionalProperties": false,
                            "properties": {
                              "componentId": {
                                "type": "string"
                              },
                              "dataBinding": {
                                "type": "string"
                              }
                            },
                            "required": ["componentId", "dataBinding"]
                          }
                        }
                      },
                      "distribution": {
                        "type": "string",
                        "description": "Defines the arrangement of children along the main axis (vertically). This corresponds to the CSS 'justify-content' property.",
                        "enum": [
                          "start",
                          "center",
                          "end",
                          "spaceBetween",
                          "spaceAround",
                          "spaceEvenly"
                        ]
                      },
                      "alignment": {
                        "type": "string",
                        "description": "Defines the alignment of children along the cross axis (horizontally). This corresponds to the CSS 'align-items' property.",
                        "enum": ["center", "end", "start", "stretch"]
                      }
                    },
                    "required": ["children"]
                  },
                  "List": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                      "children": {
                        "type": "object",
                        "description": "Defines the children. Use 'explicitList' for a fixed set of children, or 'template' to generate children from a data list.",
                        "additionalProperties": false,
                        "properties": {
                          "explicitList": {
                            "type": "array",
                            "items": {
                              "type": "string"
                            }
                          },
                          "template": {
                            "type": "object",
                            "description": "A template for generating a dynamic list of children from a data model list. `componentId` is the component to use as a template, and `dataBinding` is the path to the map of components in the data model. Values in the map will define the list of children.",
                            "additionalProperties": false,
                            "properties": {
                              "componentId": {
                                "type": "string"
                              },
                              "dataBinding": {
                                "type": "string"
                              }
                            },
                            "required": ["componentId", "dataBinding"]
                          }
                        }
                      },
                      "direction": {
                        "type": "string",
                        "description": "The direction in which the list items are laid out.",
                        "enum": ["vertical", "horizontal"]
                      },
                      "alignment": {
                        "type": "string",
                        "description": "Defines the alignment of children along the cross axis.",
                        "enum": ["start", "center", "end", "stretch"]
                      }
                    },
                    "required": ["children"]
                  },
                  "Card": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                      "child": {
                        "type": "string",
                        "description": "The ID of the component to be rendered inside the card."
                      }
                    },
                    "required": ["child"]
                  },
                  "Tabs": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                      "tabItems": {
                        "type": "array",
                        "description": "An array of objects, where each object defines a tab with a title and a child component.",
                        "items": {
                          "type": "object",
                          "additionalProperties": false,
                          "properties": {
                            "title": {
                              "type": "object",
                              "description": "The tab title. Defines the value as either a literal value or a path to data model value (e.g. '/options/title').",
                              "additionalProperties": false,
                              "properties": {
                                "literalString": {
                                  "type": "string"
                                },
                                "path": {
                                  "type": "string"
                                }
                              }
                            },
                            "child": {
                              "type": "string"
                            }
                          },
                          "required": ["title", "child"]
                        }
                      }
                    },
                    "required": ["tabItems"]
                  },
                  "Divider": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                      "axis": {
                        "type": "string",
                        "description": "The orientation of the divider.",
                        "enum": ["horizontal", "vertical"]
                      }
                    }
                  },
                  "Modal": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                      "entryPointChild": {
                        "type": "string",
                        "description": "The ID of the component that opens the modal when interacted with (e.g., a button)."
                      },
                      "contentChild": {
                        "type": "string",
                        "description": "The ID of the component to be displayed inside the modal."
                      }
                    },
                    "required": ["entryPointChild", "contentChild"]
                  },
                  "Button": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                      "child": {
                        "type": "string",
                        "description": "The ID of the component to display in the button, typically a Text component."
                      },
                      "primary": {
                        "type": "boolean",
                        "description": "Indicates if this button should be styled as the primary action."
                      },
                      "action": {
                        "type": "object",
                        "description": "The client-side action to be dispatched when the button is clicked. It includes the action's name and an optional context payload.",
                        "additionalProperties": false,
                        "properties": {
                          "name": {
                            "type": "string"
                          },
                          "context": {
                            "type": "array",
                            "items": {
                              "type": "object",
                              "additionalProperties": false,
                              "properties": {
                                "key": {
                                  "type": "string"
                                },
                                "value": {
                                  "type": "object",
                                  "description": "Defines the value to be included in the context as either a literal value or a path to a data model value (e.g. '/user/name').",
                                  "additionalProperties": false,
                                  "properties": {
                                    "path": {
                                      "type": "string"
                                    },
                                    "literalString": {
                                      "type": "string"
                                    },
                                    "literalNumber": {
                                      "type": "number"
                                    },
                                    "literalBoolean": {
                                      "type": "boolean"
                                    }
                                  }
                                }
                              },
                              "required": ["key", "value"]
                            }
                          }
                        },
                        "required": ["name"]
                      }
                    },
                    "required": ["child", "action"]
                  },
                  "CheckBox": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                      "label": {
                        "type": "object",
                        "description": "The text to display next to the checkbox. Defines the value as either a literal value or a path to data model ('path', e.g. '/option/label').",
                        "additionalProperties": false,
                        "properties": {
                          "literalString": {
                            "type": "string"
                          },
                          "path": {
                            "type": "string"
                          }
                        }
                      },
                      "value": {
                        "type": "object",
                        "description": "The current state of the checkbox (true for checked, false for unchecked). This can be a literal boolean ('literalBoolean') or a reference to a value in the data model ('path', e.g. '/filter/open').",
                        "additionalProperties": false,
                        "properties": {
                          "literalBoolean": {
                            "type": "boolean"
                          },
                          "path": {
                            "type": "string"
                          }
                        }
                      }
                    },
                    "required": ["label", "value"]
                  },
                  "TextField": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                      "label": {
                        "type": "object",
                        "description": "The text label for the input field. This can be a literal string or a reference to a value in the data model ('path, e.g. '/user/name').",
                        "additionalProperties": false,
                        "properties": {
                          "literalString": {
                            "type": "string"
                          },
                          "path": {
                            "type": "string"
                          }
                        }
                      },
                      "text": {
                        "type": "object",
                        "description": "The value of the text field. This can be a literal string or a reference to a value in the data model ('path', e.g. '/user/name').",
                        "additionalProperties": false,
                        "properties": {
                          "literalString": {
                            "type": "string"
                          },
                          "path": {
                            "type": "string"
                          }
                        }
                      },
                      "textFieldType": {
                        "type": "string",
                        "description": "The type of input field to display.",
                        "enum": [
                          "date",
                          "longText",
                          "number",
                          "shortText",
                          "obscured"
                        ]
                      },
                      "validationRegexp": {
                        "type": "string",
                        "description": "A regular expression used for client-side validation of the input."
                      }
                    },
                    "required": ["label"]
                  },
                  "DateTimeInput": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                      "value": {
                        "type": "object",
                        "description": "The selected date and/or time value in ISO 8601 format. This can be a literal string ('literalString') or a reference to a value in the data model ('path', e.g. '/user/dob').",
                        "additionalProperties": false,
                        "properties": {
                          "literalString": {
                            "type": "string"
                          },
                          "path": {
                            "type": "string"
                          }
                        }
                      },
                      "enableDate": {
                        "type": "boolean",
                        "description": "If true, allows the user to select a date."
                      },
                      "enableTime": {
                        "type": "boolean",
                        "description": "If true, allows the user to select a time."
                      }
                    },
                    "required": ["value"]
                  },
                  "MultipleChoice": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                      "selections": {
                        "type": "object",
                        "description": "The currently selected values for the component. This can be a literal array of strings or a path to an array in the data model('path', e.g. '/hotel/options').",
                        "additionalProperties": false,
                        "properties": {
                          "literalArray": {
                            "type": "array",
                            "items": {
                              "type": "string"
                            }
                          },
                          "path": {
                            "type": "string"
                          }
                        }
                      },
                      "options": {
                        "type": "array",
                        "description": "An array of available options for the user to choose from.",
                        "items": {
                          "type": "object",
                          "additionalProperties": false,
                          "properties": {
                            "label": {
                              "type": "object",
                              "description": "The text to display for this option. This can be a literal string or a reference to a value in the data model (e.g. '/option/label').",
                              "additionalProperties": false,
                              "properties": {
                                "literalString": {
                                  "type": "string"
                                },
                                "path": {
                                  "type": "string"
                                }
                              }
                            },
                            "value": {
                              "type": "string",
                              "description": "The value to be associated with this option when selected."
                            }
                          },
                          "required": ["label", "value"]
                        }
                      },
                      "maxAllowedSelections": {
                        "type": "integer",
                        "description": "The maximum number of options that the user is allowed to select."
                      }
                    },
                    "required": ["selections", "options"]
                  },
                  "Slider": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                      "value": {
                        "type": "object",
                        "description": "The current value of the slider. This can be a literal number ('literalNumber') or a reference to a value in the data model ('path', e.g. '/restaurant/cost').",
                        "additionalProperties": false,
                        "properties": {
                          "literalNumber": {
                            "type": "number"
                          },
                          "path": {
                            "type": "string"
                          }
                        }
                      },
                      "minValue": {
                        "type": "number",
                        "description": "The minimum value of the slider."
                      },
                      "maxValue": {
                        "type": "number",
                        "description": "The maximum value of the slider."
                      }
                    },
                    "required": ["value"]
                  }
                }
              }
            },
            "required": ["id", "component"]
          }
        }
      },
      "required": ["surfaceId", "components"]
    },
    "dataModelUpdate": {
      "type": "object",
      "description": "Updates the data model for a surface.",
      "additionalProperties": false,
      "properties": {
        "surfaceId": {
          "type": "string",
          "description": "The unique identifier for the UI surface this data model update applies to."
        },
        "path": {
          "type": "string",
          "description": "An optional path to a location within the data model (e.g., '/user/name'). If omitted, or set to '/', the entire data model will be replaced."
        },
        "contents": {
          "type": "array",
          "description": "An array of data entries. Each entry must contain a 'key' and exactly one corresponding typed 'value*' property.",
          "items": {
            "type": "object",
            "description": "A single data entry. Exactly one 'value*' property should be provided alongside the key.",
            "additionalProperties": false,
            "properties": {
              "key": {
                "type": "string",
                "description": "The key for this data entry."
              },
              "valueString": {
                "type": "string"
              },
              "valueNumber": {
                "type": "number"
              },
              "valueBoolean": {
                "type": "boolean"
              },
              "valueMap": {
                "description": "Represents a map as an adjacency list.",
                "type": "array",
                "items": {
                  "type": "object",
                  "description": "One entry in the map. Exactly one 'value*' property should be provided alongside the key.",
                  "additionalProperties": false,
                  "properties": {
                    "key": {
                      "type": "string"
                    },
                    "valueString": {
                      "type": "string"
                    },
                    "valueNumber": {
                      "type": "number"
                    },
                    "valueBoolean": {
                      "type": "boolean"
                    }
                  },
                  "required": ["key"]
                }
              }
            },
            "required": ["key"]
          }
        }
      },
      "required": ["contents", "surfaceId"]
    },
    "deleteSurface": {
      "type": "object",
      "description": "Signals the client to delete the surface identified by 'surfaceId'.",
      "additionalProperties": false,
      "properties": {
        "surfaceId": {
          "type": "string",
          "description": "The unique identifier for the UI surface to be deleted."
        }
      },
      "required": ["surfaceId"]
    }
  }
}"#;
const EMBEDDED_PROMPT_COMPACT: &str = r"由于上下文长度达到阈值，请将你之前与用户对话的历史消息输出为可交接的结构化摘要，指导下一位智能体继续执行。
要求：
1) 严格按照下面的固定结构输出（标题必须完整保留）。
2) 如果上下文中包含 [产物索引]，请优先引用其中的文件/命令/脚本信息。
3) 保留关键文件路径、命令、错误信息与结论，避免无关细节。
4) 某一项无内容时写“暂无”。

输出结构：
## 任务目标
## 已完成进度
## 关键决策与约束
## 关键数据与产物
- 文件变更：
- 读取关注：
- 命令/脚本：
- 关键输出/错误：
## 待办与下一步
";
const EMBEDDED_PROMPT_MEMORY_SUMMARY: &str = r#"你是长期记忆整理器，现在请提取用户提供的与你之前一段聊天记录中对后续有长期价值的核心信息。
请你对用户进行人物画像，优先保留用户身份信息/偏好/目标、关键决策、重要约束、长期任务与待办。
避免记录临时过程、具体工具调用细节与短期无关内容。
输出格式为：
<memory_summary>
{"历史任务": "",
 "重要信息总结": "",
 "用户反馈和偏好": ""}
 </memory_summary>"#;
const EMBEDDED_PROMPT_EN_SYSTEM: &str = r"You are WUNDER, a powerful agent responsible for understanding user intent and invoking tools when needed to complete tasks.
You have full authority to access and use all available tools and resources. To complete tasks quickly, you may freely call any tool or method.

[Safety Protocol]
- Follow laws and platform policies; do not assist with illegal, harmful, infringing, or unethical activities.
- Do not provide actionable steps for weapons, violence, hacking, malware, fraud, privacy theft, safety bypasses, or illicit drugs.
- For self-harm, harm to others, or extreme-risk requests, refuse and provide a safety-oriented response; encourage seeking professional help or local emergency resources when needed.
- If a request has potential risk but legitimate use, provide high-level, harm-reducing guidance and safe alternatives without misuse-enabling details.
- Use tools and data only within legal, authorized scope; avoid leaking sensitive data or overreaching access.

[Product Thinking]
- Goal: deliver usable, maintainable, verifiable results—not just “runs,” but “works well and lasts.”
- Method: clarify requirements and constraints (user goal, context, inputs/outputs, performance/cost, scope boundaries, acceptance criteria). Users may be unclear about their own needs, so use iterative follow-up questions to clarify.
- Method: prefer MVP plus evolvable design; avoid over-engineering but leave extension points.
- Method: state key assumptions and risks; offer alternatives with tradeoffs.
- Method: make results reproducible (clear steps, accurate commands, explicit config); avoid hidden dependencies.
- If you produce images/files/reports, you must show them in the final response using Markdown (image or link).
- Example: `![Radar Chart](/workspaces/admin/radar_chart.png)` or `![Example Essay](/workspaces/admin/美好的一天.docx)` so users can view the image in the frontend or click to download the file.

[Programming Tips]
- Confirm language/version/runtime environment/inputs/outputs/performance goals; ask if unclear.
- Prefer runnable, maintainable code: clear structure, naming, minimal comments, handle edges and errors.
- You are running inside a Docker container; environment summary:
  - Base system: Debian 12 (bookworm), image rust:1.92-slim-bookworm
  - Languages/build: Rust 1.92 + rustfmt/clippy/cargo-watch, gcc/g++/clang, cmake/ninja, pkg-config, build-essential
  - Common data/ML libs: numpy/pandas/scipy/scikit-learn/pyarrow/onnx/transformers
  - Web/API: fastapi/uvicorn/starlette/sse-starlette/flask/flask-restx/requests/aiohttp/httpx/scrapy
  - Databases: sqlalchemy/psycopg[binary]/psycopg/pymysql/pymongo
  - Docs/office: libreoffice, pandoc, wkhtmltopdf, unoconv, texlive-xetex/latex, reportlab, weasyprint, docxtpl, python-docx, python-pptx, openpyxl/xlrd/xlwt/xlsxwriter, markitdown[docx,pptx,xlsx]
  - PDF/OCR: poppler-utils, qpdf, pdftk, tesseract-ocr, pdf2image, pypdf/pdfplumber, pdfminer.six/pikepdf/pdfkit/ocrmypdf, pytesseract
  - Plotting/viz: matplotlib/seaborn/plotly/kaleido/bokeh/altair/holoviews/datashader/plotnine
  - Graphics/vector: cairo/pango/gdk-pixbuf/harfbuzz/fribidi, pycairo/cairosvg/svglib/svgwrite, graphviz/pydot, gnuplot, imagemagick/ghostscript
  - Images/media: opencv-python, pillow, imageio, ffmpeg
  - GIS/maps: gdal-bin/libgdal-dev/python3-gdal, proj/libproj, geos/libgeos, rtree/spatialindex; Python: geopandas/shapely/pyproj/fiona/rasterio
  - Cartopy: offline Natural Earth 110m data included
  - Other common: pytest/pytest-asyncio/coverage/pytest-mock, celery/redis, bcrypt/pyjwt/python-jose/passlib, jupyterlab, poetry/pipenv, loguru/rich/typer
  - Compatibility note: this container is based on Debian 12; C++ binaries built here may hit glibc version mismatch on Ubuntu 20.04
- Prefer using python3 for coding, and use matplotlib for plotting when needed.
- Only use dependencies available in the current environment; do not install new packages with pip or apt.

{ENGINEER_SYSTEM_INFO}
";
const EMBEDDED_PROMPT_EN_PLAN: &str = r#"When the plan board tool ("计划面板" / update_plan) is available, start by publishing a concise plan (2-6 steps) with it. Keep the plan updated: only one step should be in_progress at a time, mark completed steps as completed, and keep explanations brief.
"#;
const EMBEDDED_PROMPT_EN_QUESTION_PANEL: &str = r#"When the question panel tool ("问询面板" / question_panel) is available, proactively present 2-4 route options or key clarifications before completion when multiple viable paths exist. If the task already has a single clear route, or the user explicitly says "no panel / no confirmation / just do it", follow that and do not call question_panel. Keep each route title short with one-sentence impact; mark one as recommended when appropriate. Pause execution after calling the panel and wait for user selection or a new message.
"#;
const EMBEDDED_PROMPT_EN_EXTRA_FUNCTION_CALL: &str = r#"Tool signatures are provided inside the <tools> </tools> XML tag:
<tools>
{available_tools_describe}
</tools>
When calling a tool, use the function call mechanism (tool_calls/function_call). Do not output <tool_call> tags or raw JSON.
Tool results will be returned as role="tool" messages with tool_call_id.
{engineer_info}
"#;
const EMBEDDED_PROMPT_EN_EXTRA_TOOL_CALL: &str = r#"Tool signatures are provided inside the <tools> </tools> XML tag:
<tools>
{available_tools_describe}
</tools>
Each tool call must follow these rules:
1. Wrap the call in a <tool_call>...</tool_call> block.
2. Output valid JSON with only two keys: "name" (string) and "arguments" (object). Example:
<tool_call>
{"name":"final_response","arguments":{"content":"Task is complete. How else can I help?"}}
</tool_call>

Tool results will be returned as a user message prefixed with "tool_response: ".

{engineer_info}
"#;
const EMBEDDED_PROMPT_EN_ENGINEER_SYSTEM_INFO: &str = r"OS: {OS}
Date: {DATE}
Your current working directory (all commands run from here by default): {DIR}
Workspace (max 2 levels):
{WORKSPACE_TREE}
All commands must stay within the working directory and its subdirectories.
";
const EMBEDDED_PROMPT_EN_ENGINEER_INFO: &str = r#"Goal: complete the user's task accurately with minimal chatter.
- Do not end the response or call "final reply" until the task is complete.
- Before editing files, prefer batch use of read_file/list_files.
{PTC_GUIDANCE}
- Keep every response concise; unless explicitly requested, avoid logs or long code blocks.
- Call only one tool at a time, and proceed step by step.
- For long-running tasks, leave progress traces and deliver stable output; you may use schedule_task to set reminders or recurring jobs.
- If instructions are unclear, ask for clarification and avoid hallucinating details.
- When the plan board tool is enabled, start with a concise plan using it and keep it updated as you execute.
"#;
const EMBEDDED_PROMPT_EN_A2UI: &str = r##"[A2UI UI Generation Guide]

When you need to output A2UI UI, you must call the a2ui tool as the final response and do not call the "final response" tool.

Call requirements:
1. Wrap the tool call JSON in <tool_call>.
2. arguments must include:
   - uid: the UI Surface identifier. All messages will be rendered under the surfaceId mapped from this uid.
   - a2ui: an array of A2UI JSON messages. Each message must contain exactly one type (beginRendering/surfaceUpdate/dataModelUpdate/deleteSurface).
3. If a message is missing surfaceId, the system will auto-fill it with uid.
4. If you need a brief textual note, put it in arguments.content (optional).
5. Do not output Markdown code fences; only return the JSON message array inside a2ui.

Generation rules:
- a2ui is a message array. Each message may include only one action: beginRendering / surfaceUpdate / dataModelUpdate / deleteSurface.
- Typical order: beginRendering -> surfaceUpdate -> dataModelUpdate; use deleteSurface only when cleaning up.
- Components use an adjacency list: all components live in surfaceUpdate.components, and root points to the root component id.
- dataModelUpdate.contents uses key + value* (valueString/valueNumber/valueBoolean/valueMap). valueMap supports nested objects.
- List templates use List.children.template: dataBinding points to list data; inside templates use relative path (e.g. "name") to read dataContext.
- The primary action button should include "primary": true.
- Prefer standard icon names: mail / call / locationOn / calendarToday / check / close, etc.

Supported components:
- Text: text{literalString|path}, usageHint(h1~h5/caption/body)
- Row/Column/List: children{explicitList|template}, alignment/distribution, List.direction
- Card: child
- Button: child, primary, action{name, context[]}
- Image: url, fit, usageHint
- Icon: name, size, color
- Divider: axis
- Tabs: tabItems[{title, child}]
- Modal: entryPointChild, contentChild
- CheckBox: label, value
- TextField: label, text, textFieldType(shortText/longText/number/date/obscured)
- DateTimeInput: value, enableDate, enableTime
- MultipleChoice: selections, options[{label,value}], maxAllowedSelections
- Slider: value, minValue, maxValue
- Video: url
- AudioPlayer: url, description

Template rules:
- Lists: Column + List(template) + Card; put items in dataModelUpdate.
- Detail cards: Card wraps Column; use h2/h3 titles; main button primary:true.
- Action feedback: use Card or Modal to show confirmation.

Example A: Detail card
[
  {"beginRendering":{"surfaceId":"demo","root":"root","styles":{"primaryColor":"#2563eb","font":"Inter"}}},
  {"surfaceUpdate":{"surfaceId":"demo","components":[
    {"id":"root","component":{"Card":{"child":"rootColumn"}}},
    {"id":"rootColumn","component":{"Column":{"children":{"explicitList":["title","subtitle","cta"]},"alignment":"stretch"}}},
    {"id":"title","component":{"Text":{"usageHint":"h2","text":{"path":"/title"}}}},
    {"id":"subtitle","component":{"Text":{"usageHint":"caption","text":{"path":"/subtitle"}}}},
    {"id":"ctaText","component":{"Text":{"text":{"literalString":"Confirm"}}}},
    {"id":"cta","component":{"Button":{"child":"ctaText","primary":true,"action":{"name":"confirm"}}}}
  ]}},
  {"dataModelUpdate":{"surfaceId":"demo","path":"/","contents":[
    {"key":"title","valueString":"Example Title"},
    {"key":"subtitle","valueString":"Short description"}
  ]}}
]

Example B: List template
[
  {"beginRendering":{"surfaceId":"list","root":"root"}},
  {"surfaceUpdate":{"surfaceId":"list","components":[
    {"id":"root","component":{"Column":{"children":{"explicitList":["title","list"]}}}},
    {"id":"title","component":{"Text":{"usageHint":"h3","text":{"literalString":"Items"}}}},
    {"id":"list","component":{"List":{"direction":"vertical","children":{"template":{"componentId":"itemCard","dataBinding":"/items"}}}}},
    {"id":"itemCard","component":{"Card":{"child":"itemRow"}}},
    {"id":"itemRow","component":{"Row":{"children":{"explicitList":["itemName","itemDesc"]},"alignment":"center"}}},
    {"id":"itemName","component":{"Text":{"usageHint":"h4","text":{"path":"name"}}}},
    {"id":"itemDesc","component":{"Text":{"text":{"path":"desc"}}}}
  ]}},
  {"dataModelUpdate":{"surfaceId":"list","path":"/","contents":[
    {"key":"items","valueMap":[
      {"key":"item1","valueMap":[{"key":"name","valueString":"A"},{"key":"desc","valueString":"Item A"}]},
      {"key":"item2","valueMap":[{"key":"name","valueString":"B"},{"key":"desc","valueString":"Item B"}]}
    ]}
  ]}}
]
"##;
const EMBEDDED_PROMPT_EN_COMPACT: &str = r#"Context length has reached the threshold. Summarize the prior conversation into a handoff-ready structured summary to guide the next agent.
Requirements:
1) Output must follow the fixed structure below (keep all headings).
2) If the context includes [Artifact Index], prioritize referencing files/commands/scripts from it.
3) Preserve key file paths, commands, errors, and conclusions; omit irrelevant details.
4) If a section has no content, write "None".

Output structure:
## Goal
## Progress
## Decisions and Constraints
## Key Data and Artifacts
- File changes:
- Files read:
- Commands / Scripts:
- Key outputs / Errors:
## Next Steps
"#;
const EMBEDDED_PROMPT_EN_MEMORY_SUMMARY: &str = r#"You are a long-term memory curator. Extract the core information with long-term value from the user's recent conversation.
Build a user profile and prioritize identity info/preferences/goals, key decisions, important constraints, long-term tasks, and TODOs.
Avoid transient process details, tool-call specifics, and short-term irrelevant content.
Output format:
<memory_summary>
{"History Tasks": "",
 "Key Information Summary": "",
 "User Feedback and Preferences": ""}
</memory_summary>
"#;

pub struct PromptComposer {
    cache: Mutex<PromptCache>,
    tool_cache: Mutex<ToolSpecCache>,
    ttl_s: f64,
    max_items: usize,
    inflight: TokioMutex<HashMap<String, InflightEntry>>,
}

struct PromptCacheEntry {
    prompt: String,
    timestamp: f64,
}

#[derive(Default)]
struct PromptCache {
    entries: HashMap<String, PromptCacheEntry>,
    order: VecDeque<String>,
}

struct ToolSpecCacheEntry {
    specs: Vec<ToolSpec>,
    timestamp: f64,
}

#[derive(Default)]
struct ToolSpecCache {
    entries: HashMap<String, ToolSpecCacheEntry>,
    order: VecDeque<String>,
}

struct InflightEntry {
    notify: Arc<Notify>,
    waiters: usize,
}

pub fn read_prompt_template(path: &Path) -> String {
    let resolved = resolve_prompt_path(path);
    let mtime = resolved
        .metadata()
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0);
    let language = i18n::get_language().to_ascii_lowercase();
    let cache_key = format!("{language}|{}", resolved.to_string_lossy());
    let cache = prompt_file_cache();
    if let Some((cached_mtime, cached_text)) = cache.lock().get(&cache_key) {
        if *cached_mtime == mtime {
            return cached_text.clone();
        }
    }
    let text = std::fs::read_to_string(&resolved)
        .ok()
        .or_else(|| embedded_prompt_template(path, &resolved))
        .unwrap_or_default();
    cache.lock().insert(cache_key, (mtime, text.clone()));
    text
}

impl PromptComposer {
    pub fn new(ttl_s: f64, max_items: usize) -> Self {
        Self {
            cache: Mutex::new(PromptCache::default()),
            tool_cache: Mutex::new(ToolSpecCache::default()),
            ttl_s: if ttl_s <= 0.0 {
                DEFAULT_CACHE_TTL_S
            } else {
                ttl_s
            },
            max_items: if max_items == 0 {
                DEFAULT_CACHE_MAX_ITEMS
            } else {
                max_items
            },
            inflight: TokioMutex::new(HashMap::new()),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn build_system_prompt_cached(
        &self,
        config: &Config,
        config_version: u64,
        workspace: &WorkspaceManager,
        user_id: &str,
        workdir: &Path,
        overrides: Option<&Value>,
        allowed_tool_names: &HashSet<String>,
        tool_call_mode: ToolCallMode,
        skills: &SkillRegistry,
        user_tool_bindings: Option<&UserToolBindings>,
        agent_prompt: Option<&str>,
    ) -> String {
        let tool_key = build_tool_key(allowed_tool_names);
        let language = i18n::get_language();
        let tool_mode_key = match tool_call_mode {
            ToolCallMode::FunctionCall => "function_call",
            ToolCallMode::ToolCall => "tool_call",
        };
        let user_tool_version = user_tool_bindings
            .map(|item| item.user_version)
            .unwrap_or(0.0);
        let shared_tool_version = user_tool_bindings
            .map(|item| item.shared_version)
            .unwrap_or(0.0);
        let overrides_key = build_overrides_key(overrides);
        let agent_prompt_key = build_prompt_key(agent_prompt);
        let workdir_key = workdir.to_string_lossy();
        let base_key = format!(
            "{user_id}|{config_version}|{workdir_key}|{overrides_key}|{tool_key}|{tool_mode_key}|{user_tool_version}|{shared_tool_version}|{agent_prompt_key}|{language}"
        );
        let workspace_version = workspace.get_tree_cache_version(user_id);
        let cache_key = format!("{base_key}|{workspace_version}");
        let now = now_ts();
        if let Some(prompt) = self.get_cached_prompt(&cache_key, now) {
            return prompt;
        }

        loop {
            let (notify, is_leader) = {
                let mut inflight = self.inflight.lock().await;
                if let Some(entry) = inflight.get_mut(&base_key) {
                    entry.waiters = entry.waiters.saturating_add(1);
                    (entry.notify.clone(), false)
                } else {
                    let notify = Arc::new(Notify::new());
                    inflight.insert(
                        base_key.clone(),
                        InflightEntry {
                            notify: notify.clone(),
                            waiters: 0,
                        },
                    );
                    (notify, true)
                }
            };

            if !is_leader {
                notify.notified().await;
                let workspace_version = workspace.get_tree_cache_version(user_id);
                let cache_key = format!("{base_key}|{workspace_version}");
                let now = now_ts();
                if let Some(prompt) = self.get_cached_prompt(&cache_key, now) {
                    return prompt;
                }
                continue;
            }

            let workspace_version = workspace.get_tree_cache_version(user_id);
            let cache_key = format!("{base_key}|{workspace_version}");
            let now = now_ts();
            if let Some(prompt) = self.get_cached_prompt(&cache_key, now) {
                self.notify_inflight(&base_key).await;
                return prompt;
            }

            let tree_snapshot = workspace.get_workspace_tree_snapshot(user_id);
            let workspace_version = tree_snapshot.version;
            let cache_key = format!("{base_key}|{workspace_version}");
            let workspace_tree = tree_snapshot.tree;
            let include_tools_protocol =
                !allowed_tool_names.is_empty() && tool_call_mode == ToolCallMode::ToolCall;
            let include_ptc = allowed_tool_names
                .iter()
                .any(|name| resolve_tool_name(name) == "ptc");
            let tool_specs = if include_tools_protocol {
                let tool_cache_key = format!(
                    "{config_version}|{user_tool_version}|{shared_tool_version}|{language}|{tool_key}"
                );
                let now = now_ts();
                if let Some(specs) = self.get_cached_tool_specs(&tool_cache_key, now) {
                    specs
                } else {
                    let specs = collect_prompt_tool_specs(
                        config,
                        skills,
                        allowed_tool_names,
                        user_tool_bindings,
                    );
                    self.insert_cached_tool_specs(tool_cache_key, specs.clone(), now);
                    specs
                }
            } else {
                Vec::new()
            };
            let base_prompt = build_base_system_prompt(&config.server.mode);
            let workdir_display = workspace.display_path(user_id, workdir);
            let mut prompt = build_system_prompt(
                &base_prompt,
                &tool_specs,
                &workdir_display,
                &workspace_tree,
                include_tools_protocol,
                tool_call_mode,
                include_ptc,
            );
            let base_skill_specs = skills.list_specs();
            let mut skills_for_prompt = filter_skill_specs(&base_skill_specs, allowed_tool_names);
            if let Some(bindings) = user_tool_bindings {
                if !bindings.skill_specs.is_empty() {
                    let user_skills = filter_skill_specs(&bindings.skill_specs, allowed_tool_names);
                    if !user_skills.is_empty() {
                        skills_for_prompt = merge_skill_specs(skills_for_prompt, user_skills);
                    }
                }
            }
            let skill_block = build_skill_prompt_block(&workdir_display, &skills_for_prompt);
            if !skill_block.is_empty() {
                prompt = format!("{}\n\n{}", prompt.trim_end(), skill_block.trim());
            }
            if let Some(extra) = agent_prompt
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                prompt = format!("{}\n\n{}", prompt.trim_end(), extra);
            }
            if tool_call_mode == ToolCallMode::ToolCall && allowed_tool_names.contains("a2ui") {
                let a2ui_prompt = build_a2ui_prompt();
                if !a2ui_prompt.is_empty() {
                    prompt = format!("{}\n\n{}", prompt.trim_end(), a2ui_prompt.trim());
                }
            }
            let include_plan_prompt = allowed_tool_names
                .iter()
                .any(|name| resolve_tool_name(name) == "计划面板");
            if include_plan_prompt {
                let plan_prompt = read_prompt_template(Path::new("prompts/plan_prompt.txt"));
                if !plan_prompt.trim().is_empty() {
                    prompt = format!("{}\n\n{}", prompt.trim_end(), plan_prompt.trim());
                }
            }
            let include_question_panel_prompt = allowed_tool_names
                .iter()
                .any(|name| resolve_tool_name(name) == "问询面板");
            if include_question_panel_prompt {
                let question_prompt =
                    read_prompt_template(Path::new("prompts/question_panel_prompt.txt"));
                if !question_prompt.trim().is_empty() {
                    prompt = format!("{}\n\n{}", prompt.trim_end(), question_prompt.trim());
                }
            }

            self.insert_cached_prompt(cache_key, prompt.clone(), now_ts());
            self.notify_inflight(&base_key).await;
            return prompt;
        }
    }

    async fn notify_inflight(&self, key: &str) {
        let entry = {
            let mut inflight = self.inflight.lock().await;
            inflight.remove(key)
        };
        if let Some(entry) = entry {
            for _ in 0..entry.waiters {
                entry.notify.notify_one();
            }
        }
    }

    pub fn resolve_allowed_tool_names(
        &self,
        config: &Config,
        skills: &SkillRegistry,
        tool_names: &[String],
        user_tool_bindings: Option<&UserToolBindings>,
    ) -> HashSet<String> {
        let selected = normalize_tool_names(tool_names);
        if selected.is_empty() {
            return HashSet::new();
        }
        let available = collect_available_tool_names(config, skills, user_tool_bindings);
        selected
            .into_iter()
            .filter(|name| available.contains(name))
            .collect()
    }

    fn get_cached_prompt(&self, key: &str, now: f64) -> Option<String> {
        let cache = self.cache.lock();
        let entry = cache.entries.get(key)?;
        if now - entry.timestamp > self.ttl_s {
            return None;
        }
        Some(entry.prompt.clone())
    }

    fn insert_cached_prompt(&self, key: String, prompt: String, now: f64) {
        let mut cache = self.cache.lock();
        cache.entries.insert(
            key.clone(),
            PromptCacheEntry {
                prompt,
                timestamp: now,
            },
        );
        cache.order.push_back(key);
        while cache.order.len() > self.max_items {
            if let Some(old_key) = cache.order.pop_front() {
                cache.entries.remove(&old_key);
            }
        }
    }

    fn get_cached_tool_specs(&self, key: &str, now: f64) -> Option<Vec<ToolSpec>> {
        let cache = self.tool_cache.lock();
        let entry = cache.entries.get(key)?;
        if now - entry.timestamp > self.ttl_s {
            return None;
        }
        Some(entry.specs.clone())
    }

    fn insert_cached_tool_specs(&self, key: String, specs: Vec<ToolSpec>, now: f64) {
        let mut cache = self.tool_cache.lock();
        cache.entries.insert(
            key.clone(),
            ToolSpecCacheEntry {
                specs,
                timestamp: now,
            },
        );
        cache.order.push_back(key);
        while cache.order.len() > self.max_items {
            if let Some(old_key) = cache.order.pop_front() {
                cache.entries.remove(&old_key);
            }
        }
    }
}

fn build_system_prompt(
    base_prompt: &str,
    tools: &[ToolSpec],
    workdir_display: &str,
    workspace_tree: &str,
    include_tools_protocol: bool,
    tool_call_mode: ToolCallMode,
    include_ptc: bool,
) -> String {
    let engineer_system_info = build_engineer_system_info(workdir_display, workspace_tree);
    let base_prompt = render_template(
        base_prompt,
        &HashMap::from([(
            "ENGINEER_SYSTEM_INFO".to_string(),
            engineer_system_info.trim().to_string(),
        )]),
    );
    if !include_tools_protocol {
        let engineer_info = build_engineer_info(workdir_display, workspace_tree, include_ptc);
        return format!("{}\n\n{}", base_prompt.trim(), engineer_info.trim());
    }
    let tools_text = tools
        .iter()
        .map(render_tool_spec)
        .collect::<Vec<_>>()
        .join("\n");
    let extra_path = match tool_call_mode {
        ToolCallMode::FunctionCall => Path::new("prompts/extra_prompt_function_call.txt"),
        ToolCallMode::ToolCall => Path::new("prompts/extra_prompt_template.txt"),
    };
    let extra_template = read_prompt_template(extra_path);
    let extra_prompt = render_template(
        &extra_template,
        &HashMap::from([
            ("available_tools_describe".to_string(), tools_text),
            (
                "engineer_info".to_string(),
                build_engineer_info(workdir_display, workspace_tree, include_ptc),
            ),
        ]),
    );
    format!("{}\n\n{}", base_prompt.trim(), extra_prompt.trim())
}

fn build_base_system_prompt(server_mode: &str) -> String {
    let runtime_module = if is_local_runtime_mode(server_mode) {
        SYSTEM_PROMPT_MODULE_RUNTIME_LOCAL
    } else {
        SYSTEM_PROMPT_MODULE_RUNTIME_SERVER
    };
    let role = load_system_module(SYSTEM_PROMPT_MODULE_ROLE);
    let safety = load_system_module(SYSTEM_PROMPT_MODULE_SAFETY);
    let product = load_system_module(SYSTEM_PROMPT_MODULE_PRODUCT);
    let programming = load_system_module(SYSTEM_PROMPT_MODULE_PROGRAMMING);
    let runtime = load_system_module(runtime_module);
    let system_template = read_prompt_template(Path::new("prompts/system.txt"));
    if !system_template.trim().is_empty() {
        return render_template(
            &system_template,
            &HashMap::from([
                ("SYSTEM_ROLE".to_string(), role),
                ("SYSTEM_SAFETY".to_string(), safety),
                ("SYSTEM_PRODUCT".to_string(), product),
                ("SYSTEM_PROGRAMMING".to_string(), programming),
                ("SYSTEM_RUNTIME".to_string(), runtime),
            ]),
        );
    }
    let mut blocks = vec![role, safety, product, programming, runtime];
    blocks.retain(|value| !value.trim().is_empty());
    blocks.push("{ENGINEER_SYSTEM_INFO}".to_string());
    blocks.join("\n\n")
}

fn load_system_module(path: &str) -> String {
    read_prompt_template(Path::new(path)).trim().to_string()
}

fn is_local_runtime_mode(server_mode: &str) -> bool {
    matches!(
        server_mode.trim().to_ascii_lowercase().as_str(),
        "cli" | "desktop"
    )
}

fn build_engineer_system_info(workdir_display: &str, workspace_tree: &str) -> String {
    let template_path = Path::new("prompts/engineer_system_info.txt");
    let template = read_prompt_template(template_path);
    let os_name = system_name();
    let date_str = Local::now().format("%Y-%m-%d").to_string();
    render_template(
        &template,
        &HashMap::from([
            ("OS".to_string(), os_name),
            ("DATE".to_string(), date_str),
            ("DIR".to_string(), workdir_display.to_string()),
            ("WORKSPACE_TREE".to_string(), workspace_tree.to_string()),
        ]),
    )
}

fn build_engineer_info(workdir_display: &str, workspace_tree: &str, include_ptc: bool) -> String {
    let template_path = Path::new("prompts/engineer_info.txt");
    let template = read_prompt_template(template_path);
    let ptc_guidance = if include_ptc {
        i18n::t("prompt.engineer.ptc_guidance")
    } else {
        String::new()
    };
    render_template(
        &template,
        &HashMap::from([
            (
                "engineer_system_info".to_string(),
                build_engineer_system_info(workdir_display, workspace_tree),
            ),
            ("PTC_GUIDANCE".to_string(), ptc_guidance),
        ]),
    )
}

fn build_skill_prompt_block(workdir_display: &str, skills: &[SkillSpec]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let mut lines = vec![
        i18n::t("prompt.skills.header"),
        i18n::t("prompt.skills.rule1"),
        i18n::t("prompt.skills.rule2"),
        i18n::t("prompt.skills.rule3"),
        i18n::t("prompt.skills.rule4"),
        i18n::t_with_params(
            "prompt.skills.rule6",
            &HashMap::from([("workdir".to_string(), workdir_display.to_string())]),
        ),
        String::new(),
        i18n::t("prompt.skills.list_header"),
    ];
    let mut sorted = skills.to_vec();
    sorted.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    for spec in sorted {
        lines.push(String::new());
        lines.push(format!("- {}", spec.name));
        lines.push(format!(
            "  SKILL.md: {}",
            absolute_path_str_from_text(&spec.path)
        ));
        if !spec.frontmatter.trim().is_empty() {
            lines.push("  Frontmatter:".to_string());
            for raw_line in spec.frontmatter.lines() {
                let line = raw_line.trim();
                lines.push(format!("    {line}"));
            }
        }
    }
    lines.join("\n")
}

fn build_a2ui_prompt() -> String {
    let prompt_path = Path::new("prompts/a2ui_prompt.txt");
    let schema_path = Path::new("prompts/a2ui_schema.json");
    let template = read_prompt_template(prompt_path);
    let schema_text = read_prompt_template(schema_path);
    let schema_text = if schema_text.trim().is_empty() {
        "{}".to_string()
    } else {
        schema_text
    };
    render_template(
        &template,
        &HashMap::from([("a2ui_schema".to_string(), schema_text.trim().to_string())]),
    )
}

fn build_overrides_key(overrides: Option<&Value>) -> String {
    let Some(value) = overrides else {
        return String::new();
    };
    serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
}

fn build_prompt_key(prompt: Option<&str>) -> String {
    let text = prompt.unwrap_or("").trim();
    if text.is_empty() {
        return String::new();
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    use std::hash::{Hash, Hasher};
    text.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

fn render_tool_spec(spec: &ToolSpec) -> String {
    // serde_json 默认会按 key 排序输出，这里手动控制字段顺序，确保 name 在最前面便于模型检索。
    let name = serde_json::to_string(&spec.name).unwrap_or_else(|_| "\"\"".to_string());
    let description =
        serde_json::to_string(&spec.description).unwrap_or_else(|_| "\"\"".to_string());
    let arguments =
        serde_json::to_string(&spec.input_schema).unwrap_or_else(|_| "null".to_string());
    format!("{{\"name\":{name},\"description\":{description},\"arguments\":{arguments}}}")
}

fn render_template(template: &str, mapping: &HashMap<String, String>) -> String {
    let mut rendered = template.to_string();
    for (key, value) in mapping {
        rendered = rendered.replace(&format!("{{{key}}}"), value);
    }
    rendered
}

fn embedded_prompt_template(path: &Path, resolved: &Path) -> Option<String> {
    let mut keys = Vec::new();
    push_prompt_key(&mut keys, path);
    push_prompt_key(&mut keys, resolved);

    let mut candidates = Vec::new();
    let language = i18n::get_language();
    for key in &keys {
        if let Some(localized_key) = to_localized_prompt_key(key, &language) {
            if !candidates.contains(&localized_key) {
                candidates.push(localized_key);
            }
        }
    }
    for key in keys {
        if !candidates.contains(&key) {
            candidates.push(key);
        }
    }

    for key in candidates {
        if let Some(template) = embedded_prompt_by_key(&key) {
            return Some(template.to_string());
        }
    }
    None
}

fn push_prompt_key(keys: &mut Vec<String>, path: &Path) {
    if let Some(key) = normalize_prompt_key(path) {
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
}

fn normalize_prompt_key(path: &Path) -> Option<String> {
    let normalized = path
        .to_string_lossy()
        .replace('\\', "/")
        .trim()
        .trim_start_matches("./")
        .to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    if normalized.starts_with("prompts/") {
        return Some(normalized);
    }
    normalized
        .rfind("/prompts/")
        .map(|index| normalized[index + 1..].to_string())
}

fn to_localized_prompt_key(key: &str, language: &str) -> Option<String> {
    let locale = match language.trim().to_ascii_lowercase() {
        value if value.starts_with("en") => "en",
        value if value.starts_with("zh") => "zh",
        _ => return None,
    };
    let normalized = key.trim().to_ascii_lowercase();
    if !normalized.starts_with("prompts/")
        || normalized.starts_with("prompts/en/")
        || normalized.starts_with("prompts/zh/")
    {
        return None;
    }
    Some(format!(
        "prompts/{locale}/{}",
        normalized.trim_start_matches("prompts/")
    ))
}

fn embedded_prompt_by_key(key: &str) -> Option<&'static str> {
    match key {
        "prompts/system.txt" => Some(EMBEDDED_PROMPT_SYSTEM),
        "prompts/plan_prompt.txt" => Some(EMBEDDED_PROMPT_PLAN),
        "prompts/question_panel_prompt.txt" => Some(EMBEDDED_PROMPT_QUESTION_PANEL),
        "prompts/extra_prompt_function_call.txt" => Some(EMBEDDED_PROMPT_EXTRA_FUNCTION_CALL),
        "prompts/extra_prompt_template.txt" => Some(EMBEDDED_PROMPT_EXTRA_TOOL_CALL),
        "prompts/engineer_system_info.txt" => Some(EMBEDDED_PROMPT_ENGINEER_SYSTEM_INFO),
        "prompts/engineer_info.txt" => Some(EMBEDDED_PROMPT_ENGINEER_INFO),
        "prompts/a2ui_prompt.txt" => Some(EMBEDDED_PROMPT_A2UI),
        "prompts/a2ui_schema.json" => Some(EMBEDDED_PROMPT_A2UI_SCHEMA),
        "prompts/compact_prompt.txt" => Some(EMBEDDED_PROMPT_COMPACT),
        "prompts/memory_summary.txt" => Some(EMBEDDED_PROMPT_MEMORY_SUMMARY),
        "prompts/en/system.txt" => Some(EMBEDDED_PROMPT_EN_SYSTEM),
        "prompts/en/plan_prompt.txt" => Some(EMBEDDED_PROMPT_EN_PLAN),
        "prompts/en/question_panel_prompt.txt" => Some(EMBEDDED_PROMPT_EN_QUESTION_PANEL),
        "prompts/en/extra_prompt_function_call.txt" => Some(EMBEDDED_PROMPT_EN_EXTRA_FUNCTION_CALL),
        "prompts/en/extra_prompt_template.txt" => Some(EMBEDDED_PROMPT_EN_EXTRA_TOOL_CALL),
        "prompts/en/engineer_system_info.txt" => Some(EMBEDDED_PROMPT_EN_ENGINEER_SYSTEM_INFO),
        "prompts/en/engineer_info.txt" => Some(EMBEDDED_PROMPT_EN_ENGINEER_INFO),
        "prompts/en/a2ui_prompt.txt" => Some(EMBEDDED_PROMPT_EN_A2UI),
        "prompts/en/compact_prompt.txt" => Some(EMBEDDED_PROMPT_EN_COMPACT),
        "prompts/en/memory_summary.txt" => Some(EMBEDDED_PROMPT_EN_MEMORY_SUMMARY),
        _ => None,
    }
}

fn resolve_prompt_path(path: &Path) -> PathBuf {
    let mut resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        resolve_prompts_root().join(path)
    };
    let locale = match i18n::get_language().to_ascii_lowercase() {
        language if language.starts_with("en") => Some("en"),
        language if language.starts_with("zh") => Some("zh"),
        _ => None,
    };
    if let (Some(locale), Some(parent), Some(name)) =
        (locale, resolved.parent(), resolved.file_name())
    {
        let localized_parent = parent
            .file_name()
            .and_then(|value| value.to_str())
            .map(|value| value.eq_ignore_ascii_case("en") || value.eq_ignore_ascii_case("zh"))
            .unwrap_or(false);
        if !localized_parent {
            let candidate = parent.join(locale).join(name);
            if candidate.exists() {
                return candidate;
            }
        }
    }
    if !resolved.exists() && !path.is_absolute() {
        resolved = path.to_path_buf();
    }
    resolved
}

fn resolve_prompts_root() -> PathBuf {
    std::env::var(PROMPTS_ROOT_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn absolute_path_str(path: &Path) -> String {
    let resolved = if path.is_absolute() {
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
    } else {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let joined = cwd.join(path);
        joined.canonicalize().unwrap_or(joined)
    };
    let mut text = resolved.to_string_lossy().to_string();
    if cfg!(windows) {
        if let Some(stripped) = text.strip_prefix(r"\\?\") {
            text = stripped.to_string();
        }
    }
    text
}

fn absolute_path_str_from_text(raw: &str) -> String {
    if raw.trim().is_empty() {
        return String::new();
    }
    let path = PathBuf::from(raw);
    absolute_path_str(&path)
}

fn prompt_file_cache() -> &'static Mutex<HashMap<String, (f64, String)>> {
    static CACHE: OnceLock<Mutex<HashMap<String, (f64, String)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn build_tool_key(allowed_tool_names: &HashSet<String>) -> String {
    let mut list = allowed_tool_names.iter().cloned().collect::<Vec<_>>();
    list.sort();
    list.join(",")
}

fn normalize_tool_names(tool_names: &[String]) -> Vec<String> {
    if tool_names.is_empty() {
        return Vec::new();
    }
    let alias_map = builtin_aliases();
    let mut aliases_by_name: HashMap<String, Vec<String>> = HashMap::new();
    for (alias, canonical) in &alias_map {
        aliases_by_name
            .entry(canonical.clone())
            .or_default()
            .push(alias.clone());
    }
    let mut normalized = Vec::new();
    let mut seen = HashSet::new();
    for raw in tool_names {
        let name = raw.trim();
        if name.is_empty() || seen.contains(name) {
            continue;
        }
        if let Some(canonical) = alias_map.get(name) {
            push_unique(&mut normalized, &mut seen, canonical);
            if let Some(aliases) = aliases_by_name.get(canonical) {
                for alias in aliases {
                    push_unique(&mut normalized, &mut seen, alias);
                }
            }
            push_unique(&mut normalized, &mut seen, name);
            continue;
        }
        if let Some(aliases) = aliases_by_name.get(name) {
            push_unique(&mut normalized, &mut seen, name);
            for alias in aliases {
                push_unique(&mut normalized, &mut seen, alias);
            }
            continue;
        }
        push_unique(&mut normalized, &mut seen, name);
    }
    normalized
}

fn push_unique(output: &mut Vec<String>, seen: &mut HashSet<String>, value: &str) {
    if seen.insert(value.to_string()) {
        output.push(value.to_string());
    }
}

fn filter_skill_specs(
    skills: &[SkillSpec],
    allowed_tool_names: &HashSet<String>,
) -> Vec<SkillSpec> {
    if allowed_tool_names.is_empty() {
        return Vec::new();
    }
    skills
        .iter()
        .filter(|spec| allowed_tool_names.contains(&spec.name))
        .cloned()
        .collect()
}

fn merge_skill_specs(base: Vec<SkillSpec>, extra: Vec<SkillSpec>) -> Vec<SkillSpec> {
    let mut merged = Vec::new();
    let mut seen = HashSet::new();
    for spec in base.into_iter().chain(extra.into_iter()) {
        if seen.insert(spec.name.clone()) {
            merged.push(spec);
        }
    }
    merged
}

fn system_name() -> String {
    let name = System::name().unwrap_or_else(|| std::env::consts::OS.to_string());
    let version = System::os_version().unwrap_or_default();
    if version.is_empty() {
        name
    } else {
        format!("{name} {version}")
    }
}

fn now_ts() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}
