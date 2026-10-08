# -*- coding: utf-8 -*-
# Workspace create/edit dialog and delete confirmation (§6); the pick title is
# registered for the native directory chooser, which formats Rust-side strings.

ENTRIES = [
    ("ws_dlg_new", "新建工作区", "New workspace"),
    ("ws_dlg_edit", "编辑工作区", "Edit workspace"),
    ("ws_folder_label", "源文件夹", "Source folder"),
    ("ws_folder_add", "点击添加可读写文件夹", "Click to add a readable folder"),
    ("ws_folder_clear", "移除文件夹", "Remove folder"),
    ("ws_name_label", "工作区名称", "Workspace name"),
    ("ws_icon_label", "工作区图标", "Workspace icon"),
    ("ws_color_label", "工作区颜色", "Workspace color"),
    ("ws_validating", "正在校验文件夹…", "Validating folder…"),
    ("ws_btn_cancel", "取消", "Cancel"),
    ("ws_btn_create", "创建工作区", "Create workspace"),
    ("ws_btn_save", "保存", "Save"),
    ("ws_delete_body", "将解除工作区与文件夹的绑定，不会删除磁盘上的任何文件。请选择该工作区中线程的处理方式：",
     "The workspace binding is removed; nothing on disk is deleted. Choose what happens to its threads:"),
    ("ws_delete_archive", "归档全部线程", "Archive all threads"),
    ("ws_delete_archive_hint", "推荐，文件保持原样", "Recommended; files stay untouched"),
    ("ws_delete_records", "删除线程记录", "Delete thread records"),
    ("ws_delete_records_hint", "仅删除会话记录，磁盘文件保留", "Removes session records only; files stay"),
    ("ws_btn_delete", "删除", "Delete"),
    ("ws_pick_title", "选择工作区文件夹", "Choose a workspace folder"),
]
