---
name: 技能创建器
description: 创建有效技能的指南。当用户想要创建新技能(或更新现有技能)以扩展Wunder的能力,提供专业知识、工作流程或工具集成时应使用此技能。
---

# 技能创建器

本技能提供创建有效技能的指导。

## 关于技能

技能是模块化、自包含的包,通过提供专业知识、工作流程和工具来扩展Wunder的能力。可以将它们视为特定领域或任务的"入门指南"——它们将Wunder从通用智能体转变为配备程序性知识的专业智能体,而这些知识是任何模型都无法完全掌握的。

### 技能提供什么

1. 专业工作流程 - 特定领域的多步骤流程
2. 工具集成 - 使用特定文件格式或API的说明
3. 领域专业知识 - 公司特定知识、schema、业务逻辑
4. 打包资源 - 用于复杂和重复任务的scripts、references和assets

## 核心原则

### 简洁至上

context window是公共资源。技能与Wunder需要的其他所有内容共享context window:系统提示词、对话历史、其他技能的元数据以及实际的用户请求。

**默认假设: Wunder已经非常智能。** 只添加Wunder尚未拥有的上下文。质疑每一条信息:"Wunder真的需要这个解释吗?"以及"这段话是否值得其token成本?"

优先使用简洁示例而非冗长解释。

### 设置适当的自由度

将具体程度与任务的脆弱性和可变性相匹配:

**高自由度(基于文本的指令)**: 当多种方法都有效、决策取决于上下文或启发式方法指导方法时使用。

**中等自由度(伪代码或带参数的脚本)**: 当存在首选模式、允许一些变化或配置影响行为时使用。

**低自由度(特定脚本、少量参数)**: 当操作脆弱且容易出错、一致性至关重要或必须遵循特定序列时使用。

将Wunder想象为探索路径:悬崖间的窄桥需要特定护栏(低自由度),而开阔的田野允许多条路线(高自由度)。

### 技能结构

每个技能由必需的SKILL.md文件和可选的打包资源组成:

```
skill-name/
├── SKILL.md (必需)
│   ├── YAML frontmatter (必需)
│   │   ├── name: (必需)
│   │   └── description: (必需)
│   └── Markdown指令 (必需)
└── 打包资源 (可选)
    ├── scripts/          - 可执行代码(Python/Bash等)
    ├── references/       - 需要时加载到上下文的文档
    └── assets/           - 输出中使用的文件(模板、图标、字体等)
```

#### SKILL.md (必需)

每个SKILL.md由以下组成:

- **frontmatter**(YAML): 包含`name`和`description`字段。这些是Wunder用于确定何时使用技能的唯一字段,因此在描述技能是什么以及何时应该使用时必须清晰全面。
- **主体**(Markdown): 使用技能的指令和指导。仅在技能触发后加载(如果会触发的话)。

#### 打包资源 (可选)

##### scripts (`scripts/`)

用于需要确定性可靠性或被反复重写的任务的可执行代码(Python/Bash等)。

- **何时包含**: 当相同代码被反复重写或需要确定性可靠性时
- **示例**: 用于PDF旋转任务的`scripts/rotate_pdf.py`
- **优势**: token高效、确定性、可在不加载到context window的情况下执行
- **注意**: 脚本可能仍需要被Wunder读取以进行patching或环境特定调整

##### references (`references/`)

需要时加载到上下文中以指导Wunder过程和思考的文档和参考材料。

- **何时包含**: 用于Wunder在工作时应参考的文档
- **示例**: 用于财务schema的`references/finance.md`、用于公司NDA模板的`references/mnda.md`、用于公司政策的`references/policies.md`、用于API规范的`references/api_docs.md`
- **用例**: 数据库schema、API文档、领域知识、公司政策、详细工作流程指南
- **优势**: 保持SKILL.md精简,仅在Wunder确定需要时加载
- **最佳实践**: 如果文件很大(>10k词),在SKILL.md中包含grep搜索模式
- **避免重复**: 信息应该存在于SKILL.md或references文件中,而不是两者都有。对于详细信息,优先使用references文件,除非它对技能真正核心——这可以保持SKILL.md精简,同时使信息可发现而不占用context window。在SKILL.md中只保留必要的程序性指令和工作流程指导;将详细的参考材料、schema和示例移至references文件。

##### assets (`assets/`)

不打算加载到上下文中,而是在Wunder产生的输出中使用的文件。

- **何时包含**: 当技能需要在最终输出中使用的文件时
- **示例**: 用于品牌资产的`assets/logo.png`、用于PowerPoint模板的`assets/slides.pptx`、用于HTML/React样板代码的`assets/frontend-template/`、用于字体的`assets/font.ttf`
- **用例**: 模板、图像、图标、样板代码、字体、被复制或修改的示例文档
- **优势**: 将输出资源与文档分离,使Wunder能够在不将文件加载到context window的情况下使用它们

#### 技能中不应包含的内容

技能应该只包含直接支持其功能的必要文件。不要创建额外的文档或辅助文件,包括:

- README.md
- INSTALLATION_GUIDE.md
- QUICK_REFERENCE.md
- CHANGELOG.md
- 等等

技能应该只包含AI智能体完成手头工作所需的信息。它不应该包含关于创建过程的辅助上下文、设置和测试程序、面向用户的文档等。创建额外的文档文件只会增加混乱和困惑。

### 渐进式披露设计原则

技能使用三级加载系统来高效管理上下文:

1. **元数据(name + description)** - 始终在上下文中(~100词)
2. **SKILL.md主体** - 技能触发时(<5k词)
3. **打包资源** - Wunder根据需要(无限制,因为scripts可以在不读入context window的情况下执行)

#### 渐进式披露模式

保持SKILL.md主体精简,在500行以内以最小化上下文膨胀。当接近此限制时将内容拆分到单独的文件。将内容拆分到其他文件时,从SKILL.md引用它们并清楚描述何时读取非常重要,以确保技能读者知道它们的存在和使用时机。

**关键原则:** 当技能支持多种变体、框架或选项时,在SKILL.md中只保留核心工作流程和选择指导。将变体特定的细节(模式、示例、配置)移至单独的references文件。

**模式1: 带references的高级指南**

```markdown
# PDF处理

## 快速开始

使用pdfplumber提取文本:
[代码示例]

## 高级功能

- **表单填写**: 完整指南见[FORMS.md](FORMS.md)
- **API参考**: 所有方法见[REFERENCE.md](REFERENCE.md)
- **示例**: 常见模式见[EXAMPLES.md](EXAMPLES.md)
```

Wunder仅在需要时加载FORMS.md、REFERENCE.md或EXAMPLES.md。

**模式2: 按领域组织**

对于支持多个领域的技能,按领域组织内容以避免加载不相关的上下文:

```
bigquery-skill/
├── SKILL.md (概览和导航)
└── references/
    ├── finance.md (收入、账单指标)
    ├── sales.md (机会、管道)
    ├── product.md (API使用、功能)
    └── marketing.md (活动、归因)
```

当用户询问销售指标时,Wunder只读取sales.md。

类似地,对于支持多个框架或变体的技能,按变体组织:

```
cloud-deploy/
├── SKILL.md (工作流程 + 提供商选择)
└── references/
    ├── aws.md (AWS部署模式)
    ├── gcp.md (GCP部署模式)
    └── azure.md (Azure部署模式)
```

当用户选择AWS时,Wunder只读取aws.md。

**模式3: 条件性细节**

显示基本内容,链接到高级内容:

```markdown
# DOCX处理

## 创建文档

使用docx-js创建新文档。见[DOCX-JS.md](DOCX-JS.md)。

## 编辑文档

对于简单编辑,直接修改XML。

**对于tracked changes**: 见[REDLINING.md](REDLINING.md)
**对于OOXML细节**: 见[OOXML.md](OOXML.md)
```

Wunder仅在用户需要这些功能时读取REDLINING.md或OOXML.md。

**重要指南:**

- **避免深层嵌套引用** - 保持references与SKILL.md相隔一层。所有references文件应直接从SKILL.md链接。
- **构建较长的references文件** - 对于超过100行的文件,在顶部包含目录,以便Wunder在预览时能看到完整范围。

## 技能创建流程

技能创建涉及以下步骤:

1. 通过具体示例理解技能
2. 规划可复用技能内容(scripts、references、assets)
3. 初始化技能(运行init_skill.py)
4. 编辑技能(实现资源并编写SKILL.md)
5. 打包技能(运行package_skill.py)
6. 基于实际使用迭代

按顺序执行这些步骤,仅在明确不适用时跳过。

### 步骤1: 通过具体示例理解技能

仅当技能的使用模式已经清楚理解时跳过此步骤。即使在使用现有技能时,它仍然有价值。

要创建有效的技能,需要清楚理解技能将如何使用的具体示例。这种理解可以来自直接的用户示例或经用户反馈验证的生成示例。

例如,构建图像编辑器技能时,相关问题包括:

- "图像编辑器技能应该支持什么功能?编辑、旋转,还有其他吗?"
- "你能给出一些这个技能如何使用的示例吗?"
- "我可以想象用户会要求'消除这张图片的红眼'或'旋转这张图片'。你还能想象这个技能的其他使用方式吗?"
- "用户会说什么来触发这个技能?"

为避免让用户不知所措,避免在一条消息中问太多问题。从最重要的问题开始,根据需要跟进以提高效果。

当清楚了解技能应支持的功能时,结束此步骤。

### 步骤2: 规划可复用技能内容

要将具体示例转化为有效的技能,通过以下方式分析每个示例:

1. 考虑如何从头开始执行示例
2. 识别重复执行这些工作流程时有用的scripts、references和assets

示例:构建`pdf-editor`技能来处理"帮我旋转这个PDF"等查询时,分析显示:

1. 旋转PDF需要每次重写相同的代码
2. 在技能中存储`scripts/rotate_pdf.py`脚本会有帮助

示例:设计`frontend-webapp-builder`技能来处理"构建一个待办应用"或"构建一个追踪我步数的仪表板"等查询时,分析显示:

1. 编写前端webapp每次需要相同的样板HTML/React
2. 在技能中存储包含样板HTML/React项目文件的`assets/hello-world/`模板会有帮助

示例:构建`big-query`技能来处理"今天有多少用户登录?"等查询时,分析显示:

1. 查询BigQuery需要每次重新发现table schema和关系
2. 在技能中存储记录table schema的`references/schema.md`文件会有帮助

要确定技能内容,分析每个具体示例以创建要包含的可复用资源列表:scripts、references和assets。

### 步骤3: 初始化技能

此时,是时候实际创建技能了。

仅当正在开发的技能已经存在且需要迭代或打包时跳过此步骤。在这种情况下,继续下一步。

从头创建新技能时,始终运行`init_skill.py`脚本。该脚本方便地生成新的模板技能目录,自动包含技能所需的一切,使技能创建过程更加高效可靠。

用法:

```bash
scripts/init_skill.py <skill-name> --path <output-directory>
```

脚本会:

- 在指定路径创建技能目录
- 生成带有正确frontmatter和TODO占位符的SKILL.md模板
- 创建示例资源目录:`scripts/`、`references/`和`assets/`
- 在每个目录中添加可以自定义或删除的示例文件

初始化后,根据需要自定义或删除生成的SKILL.md和示例文件。

### 步骤4: 编辑技能

编辑(新生成或现有)技能时,记住技能是为另一个Wunder实例使用而创建的。包含对Wunder有益且非显而易见的信息。考虑哪些程序性知识、领域特定细节或可复用资产可以帮助另一个Wunder实例更有效地执行这些任务。

#### 学习经过验证的设计模式

根据技能需求参考这些有用的指南:

- **多步骤流程**: 顺序工作流程和条件逻辑见references/workflows.md
- **特定输出格式或质量标准**: 模板和示例模式见references/output-patterns.md

这些文件包含有效技能设计的既定最佳实践。

#### 从可复用技能内容开始

要开始实现,从上面识别的可复用资源开始:`scripts/`、`references/`和`assets/`文件。注意此步骤可能需要用户输入。例如,实现`brand-guidelines`技能时,用户可能需要提供品牌assets或模板存储在`assets/`中,或文档存储在`references/`中。

添加的scripts必须通过实际运行进行测试,以确保没有错误且输出符合预期。如果有许多类似的scripts,只需要测试代表性样本以确保对它们都能工作的信心,同时平衡完成时间。

技能不需要的任何示例文件和目录应该删除。初始化脚本在`scripts/`、`references/`和`assets/`中创建示例文件以演示结构,但大多数技能不需要所有这些。

#### 更新SKILL.md

**编写指南:** 始终使用祈使/不定式形式。

##### frontmatter

编写带有`name`和`description`的YAML frontmatter:

- `name`: 技能名称
- `description`: 这是技能的主要触发机制,帮助Wunder理解何时使用技能。
  - 包括技能做什么以及何时使用的特定触发器/上下文。
  - 在此处包含所有"何时使用"信息 - 不在主体中。主体仅在触发后加载,因此主体中的"何时使用此技能"部分对Wunder没有帮助。
  - `docx`技能的描述示例: "全面的文档创建、编辑和分析,支持tracked changes、注释、格式保留和文本提取。当Wunder需要处理专业文档(.docx文件)时使用:(1) 创建新文档,(2) 修改或编辑内容,(3) 处理tracked changes,(4) 添加注释,或任何其他文档任务"

不要在YAML frontmatter中包含任何其他字段。

##### 主体

编写使用技能及其打包资源的指令。

### 步骤5: 打包技能

技能开发完成后,必须打包成可分发的.skill文件与用户共享。打包过程首先自动验证技能以确保满足所有要求:

```bash
scripts/package_skill.py <path/to/skill-folder>
```

可选指定输出目录:

```bash
scripts/package_skill.py <path/to/skill-folder> ./dist
```

打包脚本会:

1. **验证** 技能自动检查:

   - YAML frontmatter格式和必需字段
   - 技能命名约定和目录结构
   - 描述完整性和质量
   - 文件组织和资源引用

2. **打包** 技能(如果验证通过),创建以技能命名的.skill文件(例如`my-skill.skill`),包含所有文件并保持正确的目录结构以便分发。.skill文件是带有.skill扩展名的zip文件。

如果验证失败,脚本会报告错误并退出而不创建包。修复任何验证错误并再次运行打包命令。

### 步骤6: 迭代

测试技能后,用户可能请求改进。这通常在使用技能后立即发生,此时对技能表现有新鲜的上下文。

**迭代工作流程:**

1. 在实际任务中使用技能
2. 注意困难或低效之处
3. 识别SKILL.md或打包资源应如何更新
4. 实现更改并再次测试
