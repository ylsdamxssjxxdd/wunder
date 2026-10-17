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

# Sidebar working-directory region: tree rows, usage line and the
# offline/empty states. Copy mirrors the web left-rail where the web has one;
# load-more and pull-open are desktop-specific.
ENTRIES.extend([
    ("wf_title", "工作目录", "Working directory"),
    ("wf_stats", "已用 {} · {} 个文件", "Used {} · {} files", ["used", "count"]),
    ("wf_stats_dirs", "{} 个文件夹", "{} folders", ["count"]),
    ("wf_stats_truncated", "统计为大目录下界", "Lower bound of a large directory"),
    ("wf_empty", "当前目录没有文件", "No files in this directory"),
    ("wf_empty_hint", "拖入文件，或新建目录和文件", "Drop files here, or create a folder or file"),
    ("wf_offline", "连接云端后，这里显示你的云端工作目录", "Sign in to the cloud to browse your workspace"),
    ("wf_connect", "去连接", "Connect"),
    ("wf_loading", "加载中…", "Loading…"),
    ("wf_reload", "重新加载", "Reload"),
    ("wf_more", "加载更多（剩余 {} 项）", "Load more ({} remaining)", ["count"]),
    ("wf_loading_dir", "加载中", "Loading"),
    ("wf_open_hint", "点击文件拉取到本地工作区并打开；拖出即下载，拖入即上传", "Click a file to pull and open it; drag out to download, drag in to upload"),
    # Context menu (right-click), aligned with the web tree menus.
    ("wf_menu_open", "打开", "Open"),
    ("wf_menu_download", "下载", "Download"),
    ("wf_menu_rename", "重命名", "Rename"),
    ("wf_menu_delete", "删除", "Delete"),
    ("wf_menu_reference", "引用到聊天", "Reference in chat"),
    ("wf_menu_new_dir", "新建目录", "New folder"),
    ("wf_menu_new_file", "新建文件", "New file"),
    ("wf_menu_refresh", "刷新", "Refresh"),
    ("wf_menu_open_ws", "打开本地工作区", "Open local workspace"),
    ("wf_menu_preview", "预览", "Preview"),
    ("wf_menu_archive", "打包下载", "Download as zip"),
    ("wf_menu_archive_root", "下载当前目录", "Download this folder"),
    ("wf_menu_clear", "清空", "Clear"),
    # Prompts and confirmations (rename / new dir / new file / delete / L2 approval).
    ("wf_rename_title", "重命名", "Rename"),
    ("wf_new_dir_title", "新建目录", "New folder"),
    ("wf_new_file_title", "新建文件", "New file"),
    ("wf_delete_title", "删除", "Delete"),
    ("wf_name_label", "名称", "Name"),
    ("wf_delete_body", "删除「{}」？云端文件不可恢复。", "Delete \"{}\"? Cloud files cannot be recovered.", ["name"]),
    ("wf_cloud_confirm_body", "云端要求确认后才会执行此操作。", "The cloud asks for confirmation before running this."),
    ("wf_btn_exec", "确认执行", "Confirm"),
])
