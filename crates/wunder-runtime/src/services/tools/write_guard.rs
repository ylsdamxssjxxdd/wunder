//! 落盘前的两条保护：BOM 不被静默改掉、已知二进制文件不被文本写坏。
//!
//! 背景：同一个文件在「写入」与「撤销」两条路径上必须得到同一份字节。曾经的
//! 实际情况是写入把 BOM 弄丢了、撤销又把快照里的 BOM 写回去，于是语法校验在
//! 两条路径上给出相反结论（`OK (json)` / `ERROR column 1`），用户看到的是
//! 「我什么都没改，文件却被判语法错」。策略定为**全程保留 BOM**：旧文件带 BOM
//! 就继续带，校验器自己剥掉 BOM 再判语法。
//!
//! 第二条保护针对误指定路径：往 `logo.png` 里写一段文本，等于把图片毁掉且没有
//! 任何提示。已知二进制扩展名一律要求显式 `overwrite_binary: true`。

/// 已知二进制扩展名：这些文件的字节不是文本，文本写入必然造成损坏。
const BINARY_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "tif", "tiff", "psd", "heic", "avif", "pdf",
    "zip", "gz", "tgz", "bz2", "xz", "zst", "7z", "rar", "jar", "apk", "doc", "docx", "xls",
    "xlsx", "ppt", "pptx", "odt", "ods", "odp", "woff", "woff2", "ttf", "otf", "eot", "mp3", "wav",
    "flac", "ogg", "m4a", "aac", "mp4", "m4v", "mov", "avi", "mkv", "webm", "exe", "dll", "so",
    "dylib", "o", "a", "lib", "bin", "wasm", "pyc", "class", "node", "sqlite", "sqlite3", "db",
    "mdb",
];

/// U+FEFF（BOM）；用 `from_u32` 生成，源码里不出现不可见字符。
fn bom_char() -> char {
    char::from_u32(0xFEFF).expect("U+FEFF is a valid scalar")
}

/// 剥掉文本开头的 BOM：语法校验必须看「内容本身」，不能因为 BOM 判语法错。
pub(crate) fn strip_bom(text: &str) -> &str {
    text.strip_prefix(bom_char()).unwrap_or(text)
}

/// 文件路径的二进制类型；不是已知二进制就返回 `None`。
///
/// 只看扩展名不读内容：写入前读整个文件太贵，而「扩展名说是图片」本身就足够
/// 让人停一下问清楚。
pub(crate) fn binary_kind(path: &str) -> Option<&'static str> {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|value| value.to_str())?
        .to_ascii_lowercase();
    BINARY_EXTENSIONS
        .iter()
        .copied()
        .find(|candidate| *candidate == ext)
}

/// 保留 BOM：旧内容带 BOM、新内容没带，就把 BOM 补回去。
///
/// 只补不剥：新内容自己带了 BOM 就按新内容走，用户想加 BOM 也允许。
pub(crate) fn preserve_bom(previous: Option<&str>, next: &str) -> String {
    let bom = bom_char();
    let previous_had = previous.is_some_and(|text| text.starts_with(bom));
    if previous_had && !next.starts_with(bom) {
        let mut out = String::with_capacity(next.len() + bom.len_utf8());
        out.push(bom);
        out.push_str(next);
        return out;
    }
    next.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_extensions_are_recognised_case_insensitively() {
        assert_eq!(binary_kind("assets/logo.PNG"), Some("png"));
        assert_eq!(binary_kind("a/b/font.woff2"), Some("woff2"));
        assert_eq!(binary_kind("notes.txt"), None);
        assert_eq!(binary_kind("Makefile"), None);
    }

    #[test]
    fn bom_is_kept_when_the_new_text_forgets_it() {
        let bom = bom_char().to_string();
        let previous = bom.clone() + "alpha\n";
        assert_eq!(
            preserve_bom(Some(&previous), "beta\n"),
            bom.clone() + "beta\n"
        );
        // 没有 BOM 的文件不会凭空长出一个。
        assert_eq!(preserve_bom(Some("alpha\n"), "beta\n"), "beta\n");
        // 新文件、新内容自带 BOM 都按原样写。
        assert_eq!(preserve_bom(None, "plain\n"), "plain\n");
        assert_eq!(
            preserve_bom(None, &(bom.clone() + "plain\n")),
            bom.clone() + "plain\n"
        );
    }

    #[test]
    fn strip_bom_leaves_ordinary_text_alone() {
        let bom = bom_char().to_string();
        assert_eq!(strip_bom(&(bom.clone() + "a: 1\n")), "a: 1\n");
        assert_eq!(strip_bom("a: 1\n"), "a: 1\n");
        assert_eq!(strip_bom(&bom), "");
    }
}
