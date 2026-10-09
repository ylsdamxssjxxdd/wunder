//! 工具注册表：内置工具的最小元数据单一来源。
//!
//! 过去「参数别名兜底」「并行安全名单」「执行策略分档」散落在不同模块里，
//! 容易漂移。这里把它们收拢为一张描述符表，由 `pipeline`（参数对齐）、
//! `core::exec_policy`（写/执行分档）、`orchestrator::tool_parallel`（并发名单）
//! 共同读取。

/// 工具对宿主的影响分档，用于执行策略与审批。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ToolKind {
    /// 只读，无副作用。
    Read,
    /// 写入文件系统。
    Write,
    /// 执行命令 / 脚本。
    Exec,
    /// 控制桌面等宿主能力。
    Control,
}

/// 单个内置工具的描述符。
pub(crate) struct ToolDescriptor {
    /// 规范名（与 catalog / dispatch 使用的中文规范名一致）。
    pub canonical: &'static str,
    pub kind: ToolKind,
    /// 是否可在同一次模型回合内并行执行。
    pub parallel_safe: bool,
    /// (primary, alias)：当 primary 缺省而 alias 存在时，用 alias 回填 primary。
    /// 用于把 dsh 风格的参数名（file_path/command/pattern）平滑映射到既有约定。
    pub arg_aliases: &'static [(&'static str, &'static str)],
}

/// 内置工具描述符表。仅收录有明确元数据需求的基础工具；其余工具走既有分支。
pub(crate) const DESCRIPTORS: &[ToolDescriptor] = &[
    ToolDescriptor {
        canonical: "读取文件",
        kind: ToolKind::Read,
        parallel_safe: true,
        arg_aliases: &[("path", "file_path"), ("path", "file")],
    },
    ToolDescriptor {
        canonical: "写入文件",
        kind: ToolKind::Write,
        parallel_safe: false,
        arg_aliases: &[("path", "file_path")],
    },
    ToolDescriptor {
        // 由原 `编辑` 与 `str_replace_editor` 合并：以字面替换为主，
        // 兼容补丁（input）与子命令（command）两种形态。
        // 统一取 ("file_path", "path") 方向：子命令形态的 `path` 会被回填为 `file_path`。
        canonical: "文本编辑",
        kind: ToolKind::Write,
        parallel_safe: false,
        arg_aliases: &[("file_path", "path"), ("input", "patch")],
    },
    ToolDescriptor {
        canonical: "执行命令",
        kind: ToolKind::Exec,
        parallel_safe: false,
        arg_aliases: &[("content", "command")],
    },
    ToolDescriptor {
        canonical: "搜索内容",
        kind: ToolKind::Read,
        parallel_safe: true,
        arg_aliases: &[("query", "pattern"), ("glob", "include")],
    },
    ToolDescriptor {
        canonical: "列出文件",
        kind: ToolKind::Read,
        parallel_safe: true,
        arg_aliases: &[("path", "dir")],
    },
    ToolDescriptor {
        canonical: "glob",
        kind: ToolKind::Read,
        parallel_safe: true,
        arg_aliases: &[("path", "dir")],
    },
    ToolDescriptor {
        canonical: "读图工具",
        kind: ToolKind::Read,
        parallel_safe: true,
        arg_aliases: &[("path", "file_path")],
    },
];

/// 按名字（自动解析别名）查找描述符。
pub(crate) fn descriptor(name: &str) -> Option<&'static ToolDescriptor> {
    let canonical = crate::tools::resolve_tool_name(name.trim());
    DESCRIPTORS.iter().find(|item| item.canonical == canonical)
}

/// 并行安全工具的规范名迭代器，供 `tool_parallel` 合并白名单。
pub(crate) fn parallel_safe_canonicals() -> impl Iterator<Item = &'static str> {
    DESCRIPTORS
        .iter()
        .filter(|item| item.parallel_safe)
        .map(|item| item.canonical)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_alias_to_descriptor() {
        assert_eq!(
            descriptor("read_file").map(|d| d.canonical),
            Some("读取文件")
        );
        assert_eq!(descriptor("文本编辑").map(|d| d.kind), Some(ToolKind::Write));
        assert_eq!(descriptor("edit").map(|d| d.canonical), Some("文本编辑"));
        assert_eq!(descriptor("str_replace_editor").map(|d| d.canonical), Some("文本编辑"));
        assert!(descriptor("不存在的工具").is_none());
    }

    #[test]
    fn edit_tool_is_write_and_exclusive() {
        let edit = descriptor("文本编辑").expect("edit descriptor");
        assert_eq!(edit.kind, ToolKind::Write);
        assert!(!edit.parallel_safe);
        assert!(edit
            .arg_aliases
            .iter()
            .any(|(primary, alias)| *primary == "file_path" && *alias == "path"));
    }

    #[test]
    fn parallel_safe_names_cover_read_only_basics() {
        let names: Vec<&str> = parallel_safe_canonicals().collect();
        assert!(names.contains(&"读取文件"));
        assert!(names.contains(&"搜索内容"));
        assert!(!names.contains(&"写入文件"));
        assert!(!names.contains(&"文本编辑"));
    }
}
