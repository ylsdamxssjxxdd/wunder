# -*- coding: utf-8 -*-
"""Patch catalog gaps found by build_i18n validation."""

# 1) about_text must keep slint \n escapes as literal backslash-n.
BS = chr(92)  # backslash
p = "scripts/catalog/part_common.py"
s = open(p, encoding="utf-8").read()
old = '("about_text", "心舰 · 蜂巢' + BS + BS + 'n' + BS + BS + 'n本地原生运行时", "Wunder · Hive' + BS + BS + 'n' + BS + BS + 'nLocal native runtime"),'
assert old in s, "about entry not found"
new = '("about_text", r"' + "心舰 · 蜂巢" + BS + BS + "n" + BS + BS + "n" + '本地原生运行时", r"' + "Wunder · Hive" + BS + BS + "n" + BS + BS + "n" + 'Local native runtime"),'
s = s.replace(old, new)
open(p, "w", encoding="utf-8", newline="").write(s)

# 2) files page upload/download (new parallel work) + settings current suffix.
p = "scripts/catalog/part_settings_files.py"
s = open(p, encoding="utf-8").read()
anchor = '    ("fl_target_path", "目标相对路径", "Target relative path"),'
assert anchor in s
s = s.replace(anchor, anchor + chr(10) + '    ("fl_upload", "上传文件", "Upload file"),' + chr(10) + '    ("fl_download", "下载到本机", "Download to this machine"),')
anchor2 = '    ("st_models_default_suffix", "  · 默认", "  · default"),'
assert anchor2 in s
s = s.replace(anchor2, anchor2 + chr(10) + '    ("st_current_suffix", "  · 当前", "  · current"),')
open(p, "w", encoding="utf-8", newline="").write(s)

# 3) channels accounts empty state.
p = "scripts/catalog/part_pages.py"
s = open(p, encoding="utf-8").read()
anchor = '    ("ch_no_channels", "暂无可用渠道", "No channels available"),'
assert anchor in s
s = s.replace(anchor, '    ("ch_accounts_empty", "暂无渠道账号", "No channel accounts"),' + chr(10) + anchor)
open(p, "w", encoding="utf-8", newline="").write(s)
print("catalog patched")
