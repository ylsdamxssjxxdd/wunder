---
name: 文稿校对
description: 用于校对和修订 Word 文稿（.docx），尤其是需要保留原文档格式、原位修改文字、在输出 docx 中高亮/批注标注修改位置的场景；也可检查常见公文格式、错别字、术语、标点和语病。
---

# 文稿校对（格式保留 + 标注修订）

## 技能目标
- 在原 `.docx` 基础上做原位文字修改，尽量保留原文档样式、字体、段落、表格、图片、页眉页脚和排版结构。
- 默认只生成**标注修订版 `.docx`**：修改后的文字高亮，旁边带 Word 批注，不在文末追加修改清单。
- 只有用户明确要求时，才额外生成清洁修订版、修改清单或校对报告。
- 继续支持格式检查、错别字、术语、标点和常见文本问题报告。

## 适用范围
- 适用于 Word `.docx` 文稿。
- 若用户提供的是 `.doc`，先提示其另存为 `.docx` 后再校对。
- 用户明确要求“改好后给我 Word”“标出修改位置”“像审阅一样看修改”“保留格式”时，必须优先交付修订版 `.docx`，不要只给文字报告。
- 不要把整篇文稿重新生成成新文档；必须先抽取带 `block_id` 的文本块，再按原位置应用精确修改。

## 快速流程（必须按顺序）
1. 获取待校对文档路径。
2. 运行脚本抽取文本块 JSON，获得稳定 `block_id`、位置和原文。
3. 基于文本块生成内部结构化修改清单 `edits.json`。每条修改必须包含 `block_id/before/after/reason/severity/category`。
4. 运行脚本在原 `.docx` 上原位应用修改，生成标注修订版。
5. 只向用户交付标注修订版 `.docx` 文件链接，并简要说明已完成；不要默认展示修改清单、校对报告或内部 JSON。

## 脚本
脚本路径：`{{SKILL_ROOT}}/scripts/proofread_docx.py`

### 1. 抽取可定位文本块
```bash
python {{SKILL_ROOT}}/scripts/proofread_docx.py \
  "C:/path/to/input.docx" \
  --extract-blocks-only \
  --output-blocks-json temp_dir/proofread_blocks.json
```

文本块结果中的 `blocks[]` 是模型生成修改清单的唯一依据。使用 `block_id` 定位，不要用“第几页”或模糊描述定位。

### 2. 生成内部 edits.json
结构：
```json
{
  "edits": [
    {
      "block_id": "p0001",
      "before": "原文中的精确片段",
      "after": "修订后的文本",
      "reason": "修改原因，写给用户审阅",
      "severity": "low",
      "category": "错别字",
      "occurrence": 1
    }
  ]
}
```

要求：
- `before` 必须是目标 `block_id` 的原文精确子串；不要改写整段再交给脚本。
- 同一段出现多个相同 `before` 时，用 `occurrence` 指定第几次出现。
- 修改应小而准：错别字、术语、标点、语序和病句局部替换优先。
- 不确定的内容不要强行改；只在最终回复中用一句话提示存在需人工复核项，不展开内部清单。
- 暂不把复杂格式调整写入 `edits.json`；除非用户明确要求批量规范样式。
- 判断中文字体时必须看 OOXML `w:eastAsia` 或样式中文字体，不要把 `Times New Roman` 这类西文字体当成中文字体错误。

### 3. 生成标注修订版
```bash
python {{SKILL_ROOT}}/scripts/proofread_docx.py \
  "C:/path/to/input.docx" \
  --apply-edits temp_dir/edits.json \
  --output-docx temp_dir/input-标注修订版.docx
```

输出说明：
- 标注修订版：修改后的文字会高亮，并为每处修改添加批注。
- 默认不要传 `--output-clean-docx`、`--output-changes-json`、`--output-changes-md` 或 `--append-change-list`。
- 需要内部核验时可以输出 `proofread_changes.json`，但不要默认交付给用户；若 `skipped_edit_count` 大于 0，只在最终回复中概括说明有少量修改未落地。

### 4. 仅做传统检查报告（用户明确要求时）
用户只要求检查、不要求直接修订时，可运行：
```bash
python {{SKILL_ROOT}}/scripts/proofread_docx.py \
  "C:/path/to/input.docx" \
  --output-json temp_dir/proofread_result.json \
  --output-md temp_dir/proofread_result.md
```

常用参数：
- `--output-json`：输出结构化校对结果（内部核验用，不默认交付用户）。
- `--output-md`：输出 Markdown 报告（用户明确要求报告时才交付）。
- `--output-blocks-json`：输出可定位文本块。
- `--apply-edits`：应用模型生成的修改清单。
- `--output-docx`：输出标注修订版 `.docx`。
- `--output-clean-docx`：输出清洁修订版 `.docx`，仅在用户明确要求时使用。
- `--output-changes-json` / `--output-changes-md`：输出修改落地结果和清单，默认只用于内部核验。
- `--append-change-list`：把修改清单表格追加到标注版文末，默认禁止，除非用户明确要求文末清单。
- `--max-findings`：限制最大问题条目数量（默认 `200`）。

## 交付回复模板
已生成修订版 Word：
- 标注修订版：`[文件名](文件链接)`。黄色高亮为已修改文字，批注说明修改原因。
- 已完成文稿校对与原位修订。

若存在未应用项，补充：
- 有少量建议未能自动落地，建议人工复核对应位置。

## 传统报告模板
1. 总体结论：合规等级、评分、是否建议直接使用。
2. 格式问题：列出位置、现状、标准要求、修改建议。
3. 错别字与文本问题：列出“原文片段 -> 建议修改”。
4. 优先修复清单：给出最影响质量的 3 条。
5. 复核建议：说明是否需要人工终审。

## 质量要求
- 用户要求 Word 交付时，不只报问题，必须生成可下载的 `.docx`。
- 默认只交付标注修订版；清洁修订版、修改清单、校对报告和内部 JSON 都必须等用户明确要求。
- 标注修订版不要在文末追加修改清单表格，除非用户明确要求。
- 优先使用原位替换，避免重建整篇文档导致格式丢失。
- 修改位置必须可追踪：高亮和批注必须保留。
- 结论必须与问题数量和严重度一致。
- 若未发现明显问题，明确写“本次未检出明显格式/错字问题”，并提示人工终审。
