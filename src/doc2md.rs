use crate::i18n;
use anyhow::{anyhow, Result};
use calamine::{open_workbook_auto, Data, Reader};
use encoding_rs::Encoding;
use html2md::parse_html;
use pdf_extract::extract_text as extract_pdf_text;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader as XmlReader;
use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

#[derive(Debug, Clone)]
pub struct Doc2mdResult {
    pub markdown: String,
    pub converter: String,
    pub warnings: Vec<String>,
}

const SUPPORTED_EXTENSIONS: &[&str] = &[
    ".c",
    ".cc",
    ".cfg",
    ".cpp",
    ".css",
    ".doc",
    ".docx",
    ".dps",
    ".et",
    ".h",
    ".hpp",
    ".htm",
    ".html",
    ".ini",
    ".js",
    ".json",
    ".log",
    ".markdown",
    ".md",
    ".odp",
    ".ods",
    ".odt",
    ".pdf",
    ".pptx",
    ".py",
    ".ts",
    ".txt",
    ".wps",
    ".xlsx",
];

pub fn supported_extensions() -> Vec<String> {
    let mut exts = SUPPORTED_EXTENSIONS
        .iter()
        .map(|ext| ext.to_string())
        .collect::<Vec<_>>();
    exts.sort();
    exts
}

pub async fn convert_path(path: &Path, extension: &str) -> Result<Doc2mdResult> {
    let path = path.to_path_buf();
    let extension = extension.to_string();
    tokio::task::spawn_blocking(move || convert_sync(&path, &extension))
        .await
        .map_err(|err| anyhow!(err.to_string()))?
}

fn convert_sync(path: &Path, extension: &str) -> Result<Doc2mdResult> {
    let ext = normalize_extension(extension);
    let (ext, mut sniff_warnings) = sniff_office_extension(path, &ext);
    let result = match ext.as_str() {
        ".md" | ".markdown" | ".txt" | ".log" => convert_text(path),
        ".html" | ".htm" => convert_html(path),
        ".py" => convert_code(path, "python"),
        ".c" | ".h" => convert_code(path, "c"),
        ".cpp" | ".cc" | ".hpp" => convert_code(path, "cpp"),
        ".json" => convert_code(path, "json"),
        ".js" => convert_code(path, "javascript"),
        ".ts" => convert_code(path, "typescript"),
        ".css" => convert_code(path, "css"),
        ".ini" | ".cfg" => convert_code(path, "ini"),
        ".docx" => convert_docx(path).or_else(|err| fallback_binary(path, "docx", err)),
        ".pdf" => convert_pdf(path).or_else(|err| fallback_binary(path, "pdf", err)),
        ".pptx" => convert_pptx(path).or_else(|err| fallback_binary(path, "pptx", err)),
        ".xlsx" | ".ods" => {
            convert_spreadsheet(path).or_else(|err| fallback_binary(path, "spreadsheet", err))
        }
        ".odt" => convert_odt(path).or_else(|err| fallback_binary(path, "odt", err)),
        ".odp" => convert_odp(path).or_else(|err| fallback_binary(path, "odp", err)),
        ".doc" | ".wps" => {
            convert_doc_binary(path).or_else(|err| fallback_binary(path, "doc", err))
        }
        ".dps" => convert_dps(path).or_else(|err| fallback_binary(path, "dps", err)),
        ".et" => convert_et(path).or_else(|err| fallback_binary(path, "et", err)),
        _ => Err(anyhow!(i18n::t_with_params(
            "error.unsupported_file_type",
            &std::collections::HashMap::from([("extension".to_string(), ext.clone(),)]),
        ))),
    }?;

    if result.markdown.trim().is_empty() {
        return Err(anyhow!(i18n::t("error.converter_empty_result")));
    }
    let mut result = result;
    if !sniff_warnings.is_empty() {
        result.warnings.append(&mut sniff_warnings);
    }
    Ok(result)
}

fn normalize_extension(extension: &str) -> String {
    let trimmed = extension.trim().to_lowercase();
    if trimmed.starts_with('.') {
        trimmed
    } else {
        format!(".{trimmed}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OfficeContainer {
    Zip,
    Ole,
    Unknown,
}

fn sniff_office_extension(path: &Path, extension: &str) -> (String, Vec<String>) {
    let mut warnings = Vec::new();
    let normalized = extension.to_lowercase();
    let container = sniff_office_container(path);
    let mut effective = normalized.clone();

    if container == OfficeContainer::Zip {
        if let Some(kind) = detect_zip_kind(path) {
            let inferred = match kind {
                "docx" => ".docx",
                "pptx" => ".pptx",
                "xlsx" => ".xlsx",
                "odt" => ".odt",
                "odp" => ".odp",
                "ods" => ".ods",
                _ => "",
            };
            if !inferred.is_empty() && inferred != normalized {
                warnings.push(format!(
                    "file header indicates {inferred} container; overriding extension"
                ));
                effective = inferred.to_string();
            }
        }
    } else if container == OfficeContainer::Ole {
        if normalized == ".docx" {
            warnings.push("file header indicates legacy .doc format; treating as .doc".to_string());
            effective = ".doc".to_string();
        }
    }

    (effective, warnings)
}

fn sniff_office_container(path: &Path) -> OfficeContainer {
    let mut header = [0u8; 8];
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(_) => return OfficeContainer::Unknown,
    };
    let read_len = match file.read(&mut header) {
        Ok(size) => size,
        Err(_) => return OfficeContainer::Unknown,
    };
    if read_len >= 4 {
        if header.starts_with(b"PK\x03\x04")
            || header.starts_with(b"PK\x05\x06")
            || header.starts_with(b"PK\x07\x08")
        {
            return OfficeContainer::Zip;
        }
    }
    if read_len >= 8 && header == [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1] {
        return OfficeContainer::Ole;
    }
    OfficeContainer::Unknown
}

fn detect_zip_kind(path: &Path) -> Option<&'static str> {
    let file = File::open(path).ok()?;
    let mut archive = ZipArchive::new(file).ok()?;
    if archive.by_name("word/document.xml").is_ok() {
        return Some("docx");
    }
    if archive.by_name("ppt/presentation.xml").is_ok()
        || archive.file_names().any(|name| name.starts_with("ppt/"))
    {
        return Some("pptx");
    }
    if archive.by_name("xl/workbook.xml").is_ok()
        || archive.file_names().any(|name| name.starts_with("xl/"))
    {
        return Some("xlsx");
    }
    if archive.by_name("content.xml").is_ok() {
        if let Ok(mut mimetype) = archive.by_name("mimetype") {
            let mut buffer = String::new();
            if mimetype.read_to_string(&mut buffer).is_ok() {
                let value = buffer.trim();
                if value.contains("opendocument.text") {
                    return Some("odt");
                }
                if value.contains("opendocument.presentation") {
                    return Some("odp");
                }
                if value.contains("opendocument.spreadsheet") {
                    return Some("ods");
                }
            }
        }
        return Some("odt");
    }
    None
}

fn convert_text(path: &Path) -> Result<Doc2mdResult> {
    let markdown = read_text(path)?;
    Ok(Doc2mdResult {
        markdown,
        converter: "text".to_string(),
        warnings: Vec::new(),
    })
}

fn convert_html(path: &Path) -> Result<Doc2mdResult> {
    let text = read_text(path)?;
    let mut markdown = parse_html(&text);
    if markdown.trim().is_empty() {
        markdown = strip_html_tags(&text);
    }
    Ok(Doc2mdResult {
        markdown,
        converter: "html".to_string(),
        warnings: Vec::new(),
    })
}

fn convert_code(path: &Path, language: &str) -> Result<Doc2mdResult> {
    let text = read_text(path)?;
    let markdown = wrap_code_block(&text, language);
    Ok(Doc2mdResult {
        markdown,
        converter: "code".to_string(),
        warnings: Vec::new(),
    })
}

fn convert_docx(path: &Path) -> Result<Doc2mdResult> {
    let xml = read_zip_entry(path, "word/document.xml")?;
    let markdown = parse_docx_xml(&xml)?;
    Ok(Doc2mdResult {
        markdown,
        converter: "doc2md".to_string(),
        warnings: Vec::new(),
    })
}

fn convert_pptx(path: &Path) -> Result<Doc2mdResult> {
    let slides = read_pptx_slides(path)?;
    if slides.is_empty() {
        return Err(anyhow!(i18n::t("error.converter_doc2md_convert_failed")));
    }
    let mut blocks = Vec::new();
    for (index, xml) in slides {
        let paragraphs = parse_pptx_xml(&xml)?;
        if paragraphs.is_empty() {
            continue;
        }
        let mut block = format!("## Slide {index}");
        block.push_str("\n\n");
        block.push_str(&paragraphs.join("\n\n"));
        blocks.push(block);
    }
    if blocks.is_empty() {
        return Err(anyhow!(i18n::t("error.converter_doc2md_convert_failed")));
    }
    Ok(Doc2mdResult {
        markdown: blocks.join("\n\n"),
        converter: "doc2md".to_string(),
        warnings: Vec::new(),
    })
}

fn convert_pdf(path: &Path) -> Result<Doc2mdResult> {
    let text = extract_pdf_text(path).map_err(|err| anyhow!(format!("pdf parse failed: {err}")))?;
    let markdown = normalize_text(&text);
    if markdown.trim().is_empty() {
        return Err(anyhow!(i18n::t("error.converter_doc2md_convert_failed")));
    }
    Ok(Doc2mdResult {
        markdown,
        converter: "pdf".to_string(),
        warnings: Vec::new(),
    })
}

fn convert_doc_binary(path: &Path) -> Result<Doc2mdResult> {
    let markdown = read_word_binary_text(path)
        .ok_or_else(|| anyhow!(i18n::t("error.converter_doc2md_convert_failed")))?;
    if markdown.trim().is_empty() {
        return Err(anyhow!(i18n::t("error.converter_doc2md_convert_failed")));
    }
    Ok(Doc2mdResult {
        markdown,
        converter: "doc2md".to_string(),
        warnings: Vec::new(),
    })
}

fn convert_dps(path: &Path) -> Result<Doc2mdResult> {
    let markdown = read_dps_text(path)
        .ok_or_else(|| anyhow!(i18n::t("error.converter_doc2md_convert_failed")))?;
    if markdown.trim().is_empty() {
        return Err(anyhow!(i18n::t("error.converter_doc2md_convert_failed")));
    }
    Ok(Doc2mdResult {
        markdown,
        converter: "doc2md".to_string(),
        warnings: Vec::new(),
    })
}

fn convert_et(path: &Path) -> Result<Doc2mdResult> {
    let markdown = read_et_text(path)
        .ok_or_else(|| anyhow!(i18n::t("error.converter_doc2md_convert_failed")))?;
    if markdown.trim().is_empty() {
        return Err(anyhow!(i18n::t("error.converter_doc2md_convert_failed")));
    }
    Ok(Doc2mdResult {
        markdown,
        converter: "doc2md".to_string(),
        warnings: Vec::new(),
    })
}

fn convert_spreadsheet(path: &Path) -> Result<Doc2mdResult> {
    let mut workbook = open_workbook_auto(path)
        .map_err(|err| anyhow!(format!("spreadsheet open failed: {err}")))?;
    let sheet_names = workbook.sheet_names().to_owned();
    let mut blocks = Vec::new();
    for name in sheet_names {
        if let Ok(range) = workbook.worksheet_range(&name) {
            let table = range_to_markdown(&range);
            if table.trim().is_empty() {
                continue;
            }
            blocks.push(format!("## {name}\n\n{table}"));
        }
    }
    if blocks.is_empty() {
        return Err(anyhow!(i18n::t("error.converter_doc2md_convert_failed")));
    }
    Ok(Doc2mdResult {
        markdown: blocks.join("\n\n"),
        converter: "doc2md".to_string(),
        warnings: Vec::new(),
    })
}

fn convert_odt(path: &Path) -> Result<Doc2mdResult> {
    let xml = read_zip_entry(path, "content.xml")?;
    let blocks = parse_odt_xml(&xml)?;
    if blocks.is_empty() {
        return Err(anyhow!(i18n::t("error.converter_doc2md_convert_failed")));
    }
    Ok(Doc2mdResult {
        markdown: blocks.join("\n\n"),
        converter: "doc2md".to_string(),
        warnings: Vec::new(),
    })
}

fn convert_odp(path: &Path) -> Result<Doc2mdResult> {
    let xml = read_zip_entry(path, "content.xml")?;
    let slides = parse_odp_xml(&xml)?;
    if slides.is_empty() {
        return Err(anyhow!(i18n::t("error.converter_doc2md_convert_failed")));
    }
    let mut blocks = Vec::new();
    for (index, paragraphs) in slides.into_iter().enumerate() {
        if paragraphs.is_empty() {
            continue;
        }
        let mut block = format!("## Slide {}", index + 1);
        block.push_str("\n\n");
        block.push_str(&paragraphs.join("\n\n"));
        blocks.push(block);
    }
    if blocks.is_empty() {
        return Err(anyhow!(i18n::t("error.converter_doc2md_convert_failed")));
    }
    Ok(Doc2mdResult {
        markdown: blocks.join("\n\n"),
        converter: "doc2md".to_string(),
        warnings: Vec::new(),
    })
}

fn fallback_binary(path: &Path, label: &str, err: anyhow::Error) -> Result<Doc2mdResult> {
    let data = std::fs::read(path)?;
    let markdown = extract_text_from_bytes(&data);
    if markdown.trim().is_empty() {
        return Err(err);
    }
    Ok(Doc2mdResult {
        markdown,
        converter: "doc2md".to_string(),
        warnings: vec![format!("fallback to raw text ({label}): {err}")],
    })
}

fn read_text(path: &Path) -> Result<String> {
    let data =
        std::fs::read(path).map_err(|_| anyhow!(i18n::t("error.converter_read_text_failed")))?;
    for label in ["utf-8", "utf-8-sig", "gb18030", "latin-1"] {
        if let Some(encoding) = Encoding::for_label(label.as_bytes()) {
            let (decoded, _, _) = encoding.decode(&data);
            let text = decoded.to_string();
            if !text.is_empty() {
                return Ok(text);
            }
        }
    }
    Ok(String::from_utf8_lossy(&data).to_string())
}

fn strip_html_tags(text: &str) -> String {
    let mut output = String::new();
    let mut in_tag = false;
    for ch in text.chars() {
        if ch == '<' {
            in_tag = true;
            continue;
        }
        if ch == '>' {
            in_tag = false;
            continue;
        }
        if !in_tag {
            output.push(ch);
        }
    }
    output
}

fn wrap_code_block(text: &str, language: &str) -> String {
    let body = text.trim_end();
    format!("```{language}\n{body}\n```")
}

fn read_zip_entry(path: &Path, name: &str) -> Result<String> {
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(file).map_err(|_| anyhow!(i18n::t("error.zip_invalid")))?;
    let mut entry = archive
        .by_name(name)
        .map_err(|_| anyhow!(i18n::t("error.converter_doc2md_convert_failed")))?;
    let mut buffer = Vec::new();
    entry.read_to_end(&mut buffer)?;
    Ok(String::from_utf8_lossy(&buffer).to_string())
}

fn read_pptx_slides(path: &Path) -> Result<Vec<(usize, String)>> {
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(file).map_err(|_| anyhow!(i18n::t("error.zip_invalid")))?;
    let mut slides = Vec::new();
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|_| anyhow!(i18n::t("error.zip_invalid")))?;
        let name = file.name().to_string();
        if !name.starts_with("ppt/slides/slide") || !name.ends_with(".xml") {
            continue;
        }
        let index = name
            .trim_start_matches("ppt/slides/slide")
            .trim_end_matches(".xml")
            .parse::<usize>()
            .unwrap_or(0);
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;
        let xml = String::from_utf8_lossy(&buffer).to_string();
        if index > 0 {
            slides.push((index, xml));
        }
    }
    slides.sort_by_key(|item| item.0);
    Ok(slides)
}

fn parse_docx_xml(xml: &str) -> Result<String> {
    let mut reader = XmlReader::from_str(xml);
    reader.trim_text(true);
    let mut buf = Vec::new();
    let mut blocks = Vec::new();

    let mut in_paragraph = false;
    let mut in_text = false;
    let mut in_table = false;
    let mut in_cell = false;

    let mut current_para = String::new();
    let mut para_style: Option<String> = None;

    let mut current_cell = String::new();
    let mut current_row: Vec<String> = Vec::new();
    let mut table_rows: Vec<Vec<String>> = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = e.name();
                let local = local_name(name.as_ref());
                match local {
                    b"p" => {
                        if in_cell {
                            if !current_cell.ends_with('\n') && !current_cell.is_empty() {
                                current_cell.push('\n');
                            }
                        } else {
                            in_paragraph = true;
                            current_para.clear();
                            para_style = None;
                        }
                    }
                    b"tbl" => {
                        in_table = true;
                        table_rows.clear();
                    }
                    b"tr" => {
                        if in_table {
                            current_row = Vec::new();
                        }
                    }
                    b"tc" => {
                        if in_table {
                            in_cell = true;
                            current_cell.clear();
                        }
                    }
                    b"pStyle" => {
                        if in_paragraph && !in_cell {
                            if let Some(value) = attr_value(&reader, e, b"val") {
                                para_style = Some(value);
                            }
                        }
                    }
                    b"t" => {
                        in_text = true;
                    }
                    b"tab" => append_text(&mut current_para, &mut current_cell, in_cell, "\t"),
                    b"br" => append_text(&mut current_para, &mut current_cell, in_cell, "\n"),
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let name = e.name();
                let local = local_name(name.as_ref());
                match local {
                    b"pStyle" => {
                        if in_paragraph && !in_cell {
                            if let Some(value) = attr_value(&reader, e, b"val") {
                                para_style = Some(value);
                            }
                        }
                    }
                    b"tab" => append_text(&mut current_para, &mut current_cell, in_cell, "\t"),
                    b"br" => append_text(&mut current_para, &mut current_cell, in_cell, "\n"),
                    _ => {}
                }
            }
            Ok(Event::Text(e)) => {
                if in_text {
                    if let Ok(text) = e.unescape() {
                        append_text(&mut current_para, &mut current_cell, in_cell, text.as_ref());
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.name();
                let local = local_name(name.as_ref());
                match local {
                    b"t" => in_text = false,
                    b"p" => {
                        if in_cell {
                            if !current_cell.ends_with('\n') {
                                current_cell.push('\n');
                            }
                        } else if in_paragraph {
                            let text = normalize_text(&current_para);
                            if !text.is_empty() {
                                if let Some(level) = heading_level_from_style(para_style.as_deref())
                                {
                                    blocks.push(format!("{} {text}", "#".repeat(level)));
                                } else {
                                    blocks.push(text);
                                }
                            }
                            in_paragraph = false;
                            current_para.clear();
                            para_style = None;
                        }
                    }
                    b"tc" => {
                        if in_cell {
                            in_cell = false;
                            let text = normalize_cell_text(&current_cell);
                            current_row.push(text);
                        }
                    }
                    b"tr" => {
                        if in_table {
                            if !current_row.is_empty() {
                                table_rows.push(std::mem::take(&mut current_row));
                            }
                        }
                    }
                    b"tbl" => {
                        if in_table {
                            in_table = false;
                            let table_md = render_table(&table_rows);
                            if !table_md.is_empty() {
                                blocks.push(table_md);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(err) => return Err(anyhow!(err.to_string())),
            _ => {}
        }
        buf.clear();
    }

    Ok(blocks.join("\n\n"))
}

fn parse_pptx_xml(xml: &str) -> Result<Vec<String>> {
    let mut reader = XmlReader::from_str(xml);
    reader.trim_text(true);
    let mut buf = Vec::new();
    let mut paragraphs = Vec::new();
    let mut current = String::new();
    let mut in_text = false;
    let mut in_paragraph = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = e.name();
                let local = local_name(name.as_ref());
                match local {
                    b"p" => {
                        in_paragraph = true;
                        current.clear();
                    }
                    b"t" => in_text = true,
                    b"br" => {
                        if in_paragraph {
                            current.push('\n');
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let name = e.name();
                let local = local_name(name.as_ref());
                if local == b"br" && in_paragraph {
                    current.push('\n');
                }
            }
            Ok(Event::Text(e)) => {
                if in_text {
                    if let Ok(text) = e.unescape() {
                        current.push_str(text.as_ref());
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.name();
                let local = local_name(name.as_ref());
                match local {
                    b"t" => in_text = false,
                    b"p" => {
                        let text = normalize_text(&current);
                        if !text.is_empty() {
                            paragraphs.push(text);
                        }
                        in_paragraph = false;
                        current.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(err) => return Err(anyhow!(err.to_string())),
            _ => {}
        }
        buf.clear();
    }

    Ok(paragraphs)
}

fn parse_odt_xml(xml: &str) -> Result<Vec<String>> {
    let mut reader = XmlReader::from_str(xml);
    reader.trim_text(true);
    let mut buf = Vec::new();
    let mut blocks = Vec::new();
    let mut current = String::new();
    let mut in_text = false;
    let mut heading_level = 0;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = e.name();
                let (prefix, local) = split_tag_name(name.as_ref());
                if prefix == Some(b"text") && local == b"p" {
                    current.clear();
                    in_text = true;
                    heading_level = 0;
                } else if prefix == Some(b"text") && local == b"h" {
                    current.clear();
                    in_text = true;
                    heading_level = parse_outline_level(&reader, e).unwrap_or(1);
                }
            }
            Ok(Event::Text(e)) => {
                if in_text {
                    if let Ok(text) = e.unescape() {
                        current.push_str(text.as_ref());
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.name();
                let (prefix, local) = split_tag_name(name.as_ref());
                if prefix == Some(b"text") && (local == b"p" || local == b"h") {
                    let text = normalize_text(&current);
                    if !text.is_empty() {
                        if heading_level > 0 {
                            blocks.push(format!("{} {text}", "#".repeat(heading_level)));
                        } else {
                            blocks.push(text);
                        }
                    }
                    current.clear();
                    in_text = false;
                    heading_level = 0;
                }
            }
            Ok(Event::Eof) => break,
            Err(err) => return Err(anyhow!(err.to_string())),
            _ => {}
        }
        buf.clear();
    }

    Ok(blocks)
}

fn parse_odp_xml(xml: &str) -> Result<Vec<Vec<String>>> {
    let mut reader = XmlReader::from_str(xml);
    reader.trim_text(true);
    let mut buf = Vec::new();

    let mut slides: Vec<Vec<String>> = Vec::new();
    let mut current_slide: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_text = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = e.name();
                let (prefix, local) = split_tag_name(name.as_ref());
                if prefix == Some(b"draw") && local == b"page" {
                    if !current_slide.is_empty() {
                        slides.push(std::mem::take(&mut current_slide));
                    }
                }
                if prefix == Some(b"text") && (local == b"p" || local == b"h") {
                    current.clear();
                    in_text = true;
                }
            }
            Ok(Event::Text(e)) => {
                if in_text {
                    if let Ok(text) = e.unescape() {
                        current.push_str(text.as_ref());
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.name();
                let (prefix, local) = split_tag_name(name.as_ref());
                if prefix == Some(b"text") && (local == b"p" || local == b"h") {
                    let text = normalize_text(&current);
                    if !text.is_empty() {
                        current_slide.push(text);
                    }
                    current.clear();
                    in_text = false;
                }
            }
            Ok(Event::Eof) => break,
            Err(err) => return Err(anyhow!(err.to_string())),
            _ => {}
        }
        buf.clear();
    }

    if !current_slide.is_empty() {
        slides.push(current_slide);
    }

    Ok(slides)
}

fn append_text(current_para: &mut String, current_cell: &mut String, in_cell: bool, text: &str) {
    if in_cell {
        current_cell.push_str(text);
    } else {
        current_para.push_str(text);
    }
}

fn normalize_text(text: &str) -> String {
    let mut output = String::new();
    let mut last_space = false;
    for ch in text.chars() {
        if ch == '\r' {
            continue;
        }
        if ch == '\n' {
            if !output.ends_with('\n') {
                output.push('\n');
            }
            last_space = false;
            continue;
        }
        if ch.is_whitespace() {
            if !last_space {
                output.push(' ');
                last_space = true;
            }
            continue;
        }
        output.push(ch);
        last_space = false;
    }
    output.trim().to_string()
}

fn normalize_cell_text(text: &str) -> String {
    normalize_text(text).replace('\n', "<br>")
}

fn heading_level_from_style(style: Option<&str>) -> Option<usize> {
    let style = style?.trim();
    if style.is_empty() {
        return None;
    }
    let lowered = style.to_lowercase();
    if lowered.starts_with("heading") || lowered.starts_with("title") {
        let digits: String = lowered.chars().filter(|ch| ch.is_ascii_digit()).collect();
        if let Ok(value) = digits.parse::<usize>() {
            if value > 0 && value <= 6 {
                return Some(value);
            }
        }
        return Some(1);
    }
    None
}

fn render_table(rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let max_cols = rows.iter().map(|row| row.len()).max().unwrap_or(0);
    if max_cols == 0 {
        return String::new();
    }
    let mut normalized = Vec::new();
    for row in rows {
        let mut cells = row.clone();
        cells.resize(max_cols, String::new());
        normalized.push(cells);
    }
    let mut out = String::new();
    out.push('|');
    for cell in &normalized[0] {
        out.push(' ');
        out.push_str(&sanitize_table_cell(cell));
        out.push(' ');
        out.push('|');
    }
    out.push('\n');
    out.push('|');
    for _ in 0..max_cols {
        out.push_str(" --- |");
    }
    out.push('\n');
    for row in normalized.iter().skip(1) {
        out.push('|');
        for cell in row {
            out.push(' ');
            out.push_str(&sanitize_table_cell(cell));
            out.push(' ');
            out.push('|');
        }
        out.push('\n');
    }
    out.trim_end().to_string()
}

fn sanitize_table_cell(cell: &str) -> String {
    cell.trim().replace('|', "\\|")
}

fn range_to_markdown(range: &calamine::Range<Data>) -> String {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut max_cols = 0;
    for row in range.rows() {
        let mut values = row
            .iter()
            .map(|cell| cell_to_string(cell))
            .collect::<Vec<_>>();
        if let Some(last) = values.iter().rposition(|value| !value.trim().is_empty()) {
            values.truncate(last + 1);
        } else {
            continue;
        }
        max_cols = max_cols.max(values.len());
        rows.push(values);
    }
    if rows.is_empty() || max_cols == 0 {
        return String::new();
    }
    for row in rows.iter_mut() {
        row.resize(max_cols, String::new());
    }
    render_table(&rows)
}

fn cell_to_string(cell: &Data) -> String {
    match cell {
        Data::Empty => String::new(),
        Data::String(text) => text.to_string(),
        _ => cell.to_string(),
    }
}

fn extract_text_from_bytes(data: &[u8]) -> String {
    let utf16 = extract_utf16_text(data);
    if !utf16.is_empty() {
        return utf16;
    }
    if let Some(decoded) = decode_with_labels(
        data,
        &[
            "utf-8",
            "utf-8-sig",
            "gb18030",
            "big5",
            "shift_jis",
            "windows-1252",
        ],
    ) {
        let chunks = split_lines(&decoded);
        let combined = combine_candidate_chunks(chunks);
        if !combined.is_empty() {
            return combined;
        }
    }
    String::from_utf8_lossy(data).to_string()
}

fn split_lines(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch == '\r' {
            continue;
        }
        if ch == '\n' {
            lines.push(current);
            current = String::new();
        } else {
            current.push(ch);
        }
    }
    lines.push(current);
    lines
}

fn format_markdown_list(text: &str) -> String {
    let mut out = Vec::new();
    for line in split_lines(text) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        out.push(format!("- {trimmed}"));
    }
    out.join("\n")
}

fn decode_with_labels(data: &[u8], labels: &[&str]) -> Option<String> {
    for label in labels {
        let Some(encoding) = Encoding::for_label(label.as_bytes()) else {
            continue;
        };
        let (decoded, _, _) = encoding.decode(data);
        let text = decoded.to_string();
        if !text.trim().is_empty() {
            return Some(text);
        }
    }
    None
}

fn latin1_to_string(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len());
    for &byte in data {
        out.push(byte as char);
    }
    out
}

fn decode_with_code_page(data: &[u8], code_page: u16) -> Option<String> {
    let label = match code_page {
        936 => "gb18030",
        950 => "big5",
        932 => "shift_jis",
        1252 => "windows-1252",
        _ => return None,
    };
    let encoding = Encoding::for_label(label.as_bytes())?;
    let (decoded, _, _) = encoding.decode(data);
    let text = decoded.to_string();
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

fn decode_with_code_pages(data: &[u8], code_pages: &[u16]) -> Option<String> {
    for code_page in code_pages {
        if let Some(text) = decode_with_code_page(data, *code_page) {
            return Some(text);
        }
    }
    None
}

fn is_valid_utf8(data: &[u8]) -> bool {
    let mut expected = 0;
    for &byte in data {
        if expected == 0 {
            if (byte & 0x80) == 0 {
                continue;
            } else if (byte & 0xE0) == 0xC0 {
                expected = 1;
            } else if (byte & 0xF0) == 0xE0 {
                expected = 2;
            } else if (byte & 0xF8) == 0xF0 {
                expected = 3;
            } else {
                return false;
            }
        } else {
            if (byte & 0xC0) != 0x80 {
                return false;
            }
            expected -= 1;
        }
    }
    expected == 0
}

fn has_field_instruction_prefix(text: &str, keyword: &str) -> bool {
    if !text.starts_with(keyword) {
        return false;
    }
    let next = text.chars().nth(keyword.len());
    matches!(next, None | Some(' ') | Some('\\') | Some('"') | Some('\t'))
}

fn looks_like_field_instruction(text: &str) -> bool {
    ["HYPERLINK", "INCLUDEPICTURE", "MERGEFIELD", "PAGEREF"]
        .iter()
        .any(|keyword| has_field_instruction_prefix(text, keyword))
}

fn looks_like_document_text(chunk: &str) -> bool {
    let trimmed = chunk.trim();
    if trimmed.len() < 2 || trimmed.len() > 1024 {
        return false;
    }
    if matches!(
        trimmed,
        "Root Entry"
            | "SummaryInformation"
            | "DocumentSummaryInformation"
            | "WordDocument"
            | "0Table"
            | "1Table"
            | "Normal.dotm"
            | "WpsCustomData"
            | "KSOProductBuildVer"
            | "KSOTemplateDocerSaveRecord"
    ) {
        return false;
    }
    true
}

fn chunk_score(chunk: &str) -> i32 {
    let mut cjk = 0;
    let mut digits = 0;
    let mut ascii_alpha = 0;
    for ch in chunk.chars() {
        if ('\u{4E00}'..='\u{9FFF}').contains(&ch) {
            cjk += 1;
        }
        if ch.is_ascii_digit() {
            digits += 1;
        }
        if ch.is_ascii_alphabetic() {
            ascii_alpha += 1;
        }
    }
    let mut score = cjk * 5 + digits * 3 - ascii_alpha;
    if digits >= 6 && digits >= cjk && digits > ascii_alpha {
        score += digits * 10;
    }
    score
}

fn combine_candidate_chunks(chunks: Vec<String>) -> String {
    let mut seen = HashSet::new();
    let mut filtered = Vec::new();
    for chunk in chunks {
        let trimmed = chunk.trim();
        if trimmed.is_empty() || !looks_like_document_text(trimmed) {
            continue;
        }
        if looks_like_field_instruction(trimmed) {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            filtered.push(trimmed.to_string());
        }
    }
    if filtered.len() > 1 {
        let mut scores = Vec::with_capacity(filtered.len());
        let mut best = i32::MIN;
        for item in &filtered {
            let score = chunk_score(item);
            scores.push(score);
            best = best.max(score);
        }
        let cutoff = if best > 0 { best - 4 } else { best };
        let mut prioritized = Vec::new();
        for (item, score) in filtered.into_iter().zip(scores) {
            if score >= cutoff && score > 0 {
                prioritized.push(item);
            }
        }
        if !prioritized.is_empty() {
            return prioritized.join("\n");
        }
        return String::new();
    }
    filtered.join("\n")
}

fn extract_utf16_text(data: &[u8]) -> String {
    if data.is_empty() {
        return String::new();
    }
    let mut chunks: Vec<String> = Vec::new();
    let mut current: Vec<u16> = Vec::new();
    let mut reading = false;
    let mut offset = 0;
    while offset + 1 < data.len() {
        let value = u16::from_le_bytes([data[offset], data[offset + 1]]);
        if (0xD800..=0xDFFF).contains(&value) {
            if reading && current.len() >= 3 {
                chunks.push(String::from_utf16_lossy(&current));
            }
            current.clear();
            reading = false;
            offset += 2;
            continue;
        }
        if value >= 0x20 && value != 0xFFFF && value != 0xFFFE {
            reading = true;
            if value == 0x000D || value == 0x000A {
                current.push(0x000A);
            } else {
                current.push(value);
            }
        } else if reading {
            if current.len() >= 3 {
                chunks.push(String::from_utf16_lossy(&current));
            }
            current.clear();
            reading = false;
        }
        offset += 2;
    }
    if reading && current.len() >= 3 {
        chunks.push(String::from_utf16_lossy(&current));
    }
    combine_candidate_chunks(chunks)
}

#[derive(Debug, Clone)]
struct FibInfo {
    use_table1: bool,
    fc_min: u32,
    fc_mac: u32,
    fc_clx: u32,
    lcb_clx: u32,
}

#[derive(Debug, Clone)]
struct TextPiece {
    cp_start: u32,
    cp_end: u32,
    file_offset: u32,
    unicode: bool,
}

fn read_u16_le(data: &[u8], offset: usize) -> Option<u16> {
    data.get(offset..offset + 2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32_le(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset + 4)
        .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_u64_le(data: &[u8], offset: usize) -> Option<u64> {
    data.get(offset..offset + 8).map(|bytes| {
        u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ])
    })
}

fn parse_fib(word_stream: &[u8]) -> Option<FibInfo> {
    if word_stream.len() < 256 {
        return None;
    }
    if read_u16_le(word_stream, 0)? != 0xA5EC {
        return None;
    }
    let flags = read_u16_le(word_stream, 0x0A)?;
    let use_table1 = (flags & 0x0200) != 0;
    let fc_min = read_u32_le(word_stream, 0x18)?;
    let fc_mac = read_u32_le(word_stream, 0x1C)?;

    let mut pos = 32usize;
    let csw = read_u16_le(word_stream, pos)? as usize;
    pos += 2 + csw * 2;
    let cslw = read_u16_le(word_stream, pos)? as usize;
    pos += 2 + cslw * 4;
    let cb_rg_fc_lcb = read_u16_le(word_stream, pos)? as usize;
    pos += 2;
    if word_stream.len() < pos + cb_rg_fc_lcb * 8 {
        return None;
    }
    let mut fc_clx = 0;
    let mut lcb_clx = 0;
    let idx = 33usize;
    if cb_rg_fc_lcb > idx {
        let offset = pos + idx * 8;
        fc_clx = read_u32_le(word_stream, offset)?;
        lcb_clx = read_u32_le(word_stream, offset + 4)?;
    }
    Some(FibInfo {
        use_table1,
        fc_min,
        fc_mac,
        fc_clx,
        lcb_clx,
    })
}

fn parse_text_pieces(table_stream: &[u8], fc_clx: u32, lcb_clx: u32) -> Vec<TextPiece> {
    if fc_clx == 0 || lcb_clx == 0 {
        return Vec::new();
    }
    let end = fc_clx as usize + lcb_clx as usize;
    if end > table_stream.len() {
        return Vec::new();
    }
    let clx = &table_stream[fc_clx as usize..end];
    let mut pos = 0usize;
    let mut pieces = Vec::new();
    while pos < clx.len() {
        let clxt = clx[pos];
        pos += 1;
        if clxt == 0x01 {
            if pos + 4 > clx.len() {
                break;
            }
            let lcb = read_u32_le(clx, pos).unwrap_or(0) as usize;
            pos += 4;
            if lcb == 0 || pos + lcb > clx.len() {
                break;
            }
            let plc = &clx[pos..pos + lcb];
            if lcb < 4 {
                break;
            }
            let piece_count = (lcb - 4) / 12;
            if piece_count == 0 {
                break;
            }
            let mut cps = Vec::with_capacity(piece_count + 1);
            for i in 0..=piece_count {
                let offset = i * 4;
                cps.push(read_u32_le(plc, offset).unwrap_or(0));
            }
            let pcd = &plc[(piece_count + 1) * 4..];
            for i in 0..piece_count {
                let offset = i * 8;
                let fc = read_u32_le(pcd, offset + 2).unwrap_or(0);
                let unicode = (fc & 0x4000_0000) == 0;
                let file_offset = if unicode { fc } else { (fc & 0x3FFF_FFFF) / 2 };
                pieces.push(TextPiece {
                    cp_start: cps[i],
                    cp_end: cps[i + 1],
                    file_offset,
                    unicode,
                });
            }
            break;
        } else if clxt == 0x02 {
            if pos + 2 > clx.len() {
                break;
            }
            let cb = read_u16_le(clx, pos).unwrap_or(0) as usize;
            pos += 2 + cb;
        } else {
            break;
        }
    }
    pieces
}

fn decode_pieces(word_stream: &[u8], pieces: &[TextPiece]) -> String {
    if pieces.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for piece in pieces {
        if piece.cp_end <= piece.cp_start {
            continue;
        }
        let char_count = (piece.cp_end - piece.cp_start) as usize;
        let byte_count = if piece.unicode {
            char_count * 2
        } else {
            char_count
        };
        let start = piece.file_offset as usize;
        if start + byte_count > word_stream.len() {
            continue;
        }
        let slice = &word_stream[start..start + byte_count];
        if piece.unicode {
            let mut u16s = Vec::with_capacity(char_count);
            let mut idx = 0;
            while idx + 1 < slice.len() {
                u16s.push(u16::from_le_bytes([slice[idx], slice[idx + 1]]));
                idx += 2;
            }
            out.push_str(&String::from_utf16_lossy(&u16s));
        } else {
            out.push_str(&latin1_to_string(slice));
        }
    }
    out
}

fn decode_simple_range(word_stream: &[u8], fc_min: u32, fc_mac: u32) -> String {
    if fc_mac <= fc_min || fc_min as usize >= word_stream.len() {
        return String::new();
    }
    let limit = std::cmp::min(fc_mac as usize, word_stream.len());
    let mut span = limit.saturating_sub(fc_min as usize);
    if span < 4 {
        return String::new();
    }
    if span % 2 != 0 {
        span -= 1;
    }
    let slice = &word_stream[fc_min as usize..fc_min as usize + span];
    let mut u16s = Vec::with_capacity(span / 2);
    let mut idx = 0;
    while idx + 1 < slice.len() {
        u16s.push(u16::from_le_bytes([slice[idx], slice[idx + 1]]));
        idx += 2;
    }
    String::from_utf16_lossy(&u16s)
}

fn split_tab_line(line: &str) -> Vec<String> {
    line.split('\t')
        .map(|part| part.trim().to_string())
        .collect()
}

fn is_tabular_line(line: &str) -> Option<usize> {
    if !line.contains('\t') {
        return None;
    }
    let cells = split_tab_line(line);
    if cells.len() < 2 {
        return None;
    }
    let non_empty = cells.iter().filter(|cell| !cell.is_empty()).count();
    if non_empty == 0 {
        return None;
    }
    Some(cells.len())
}

fn expand_flattened_tab_rows(tokens: &[String]) -> Option<Vec<Vec<String>>> {
    if tokens.len() < 4 {
        return None;
    }
    let max_columns = std::cmp::min(32, tokens.len() / 2);
    let mut best_columns = 0usize;
    let mut best_score = 0.0f64;
    for candidate in 2..=max_columns {
        if tokens.len() % candidate != 0 {
            continue;
        }
        let rows = tokens.len() / candidate;
        if rows < 2 {
            continue;
        }
        let mut density = 0.0f64;
        for r in 0..rows {
            let mut non_empty = 0usize;
            for c in 0..candidate {
                if !tokens[r * candidate + c].is_empty() {
                    non_empty += 1;
                }
            }
            density += non_empty as f64 / candidate as f64;
        }
        density /= rows as f64;
        if density > best_score + 1e-6 {
            best_score = density;
            best_columns = candidate;
        }
    }
    if best_columns == 0 {
        return None;
    }
    let rows = tokens.len() / best_columns;
    let mut expanded = Vec::with_capacity(rows);
    for r in 0..rows {
        let start = r * best_columns;
        let end = start + best_columns;
        expanded.push(tokens[start..end].to_vec());
    }
    Some(expanded)
}

fn convert_lines_with_tables(lines: &[String]) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut blocks = Vec::new();
    let mut index = 0usize;
    while index < lines.len() {
        if let Some(column_count) = is_tabular_line(&lines[index]) {
            let mut rows = Vec::new();
            let mut raw_row_tokens = Vec::new();
            let mut max_columns = column_count;
            let mut cursor = index;
            while cursor < lines.len() {
                let row_columns = match is_tabular_line(&lines[cursor]) {
                    Some(count) => count,
                    None => break,
                };
                let cells = split_tab_line(&lines[cursor]);
                if cells.is_empty() {
                    break;
                }
                max_columns = max_columns.max(row_columns);
                raw_row_tokens.push(cells.clone());
                rows.push(cells);
                cursor += 1;
            }
            if rows.len() == 1 {
                if let Some(expanded) = expand_flattened_tab_rows(&raw_row_tokens[0]) {
                    rows = expanded;
                    max_columns = 0;
                    for row in &rows {
                        max_columns = max_columns.max(row.len());
                    }
                }
            }
            if rows.len() >= 2 && max_columns >= 2 {
                for row in &mut rows {
                    row.resize(max_columns, String::new());
                }
                let table = render_table(&rows);
                if !table.is_empty() {
                    blocks.push(table);
                }
            } else {
                blocks.push(lines[index].clone());
                cursor = index + 1;
            }
            index = cursor;
        } else {
            blocks.push(lines[index].clone());
            index += 1;
        }
    }
    blocks.join("\n\n")
}

fn normalize_word_text(raw: &str) -> String {
    if raw.is_empty() {
        return String::new();
    }
    let mut cleaned = Vec::with_capacity(raw.len());
    let mut field_instruction_stack = Vec::new();
    let mut pending_instruction_fields = 0usize;
    let mut idx = 0usize;
    let bytes = raw.as_bytes();
    while idx < bytes.len() {
        let byte = bytes[idx];
        match byte {
            0x00 => {}
            0x13 => {
                field_instruction_stack.push(false);
                pending_instruction_fields += 1;
            }
            0x14 => {
                if let Some(last) = field_instruction_stack.last_mut() {
                    if !*last {
                        *last = true;
                        if pending_instruction_fields > 0 {
                            pending_instruction_fields -= 1;
                        }
                    }
                }
            }
            0x15 => {
                if let Some(last) = field_instruction_stack.pop() {
                    if !last && pending_instruction_fields > 0 {
                        pending_instruction_fields -= 1;
                    }
                }
            }
            0x07 => {
                let mut run = 0usize;
                while idx < bytes.len() && bytes[idx] == 0x07 {
                    run += 1;
                    idx += 1;
                }
                if run == 1 {
                    cleaned.push(b'\t');
                } else if run > 1 {
                    for _ in 0..run.saturating_sub(1) {
                        cleaned.push(b'\t');
                    }
                    cleaned.push(b'\n');
                }
                continue;
            }
            0x0D | 0x0B | 0x0C | 0x1E | 0x1F => {
                cleaned.push(b'\n');
            }
            _ => {
                if pending_instruction_fields > 0 {
                    idx += 1;
                    continue;
                }
                if byte < 0x20 && byte != 0x09 {
                    idx += 1;
                    continue;
                }
                cleaned.push(byte);
            }
        }
        idx += 1;
    }
    let text = String::from_utf8_lossy(&cleaned);
    let lines = split_lines(&text);
    let mut filtered = Vec::new();
    for line in lines {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            filtered.push(trimmed.to_string());
        }
    }
    convert_lines_with_tables(&filtered)
}

fn read_word_binary_text(path: &Path) -> Option<String> {
    if let Some(text) = read_word_via_ole(path) {
        if !text.trim().is_empty() {
            return Some(text);
        }
    }
    let data = std::fs::read(path).ok()?;
    let fallback = extract_utf16_text(&data);
    if fallback.trim().is_empty() {
        None
    } else {
        Some(fallback)
    }
}

fn read_word_via_ole(path: &Path) -> Option<String> {
    let word_stream = read_ole_stream(path, "WordDocument")?;
    let fib = parse_fib(&word_stream)?;
    let table_name = if fib.use_table1 { "1Table" } else { "0Table" };
    let table_stream = read_ole_stream(path, table_name);
    let mut raw = String::new();
    if let Some(table_stream) = table_stream {
        if fib.fc_clx != 0 && fib.lcb_clx != 0 {
            let pieces = parse_text_pieces(&table_stream, fib.fc_clx, fib.lcb_clx);
            raw = decode_pieces(&word_stream, &pieces);
        }
    }
    if raw.is_empty() {
        raw = decode_simple_range(&word_stream, fib.fc_min, fib.fc_mac);
    }
    let normalized = normalize_word_text(&raw);
    if normalized.trim().is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn detect_biff_code_page(stream: &[u8]) -> Option<u16> {
    let mut offset = 0usize;
    while offset + 4 <= stream.len() {
        let record_type = read_u16_le(stream, offset)?;
        let size = read_u16_le(stream, offset + 2)? as usize;
        offset += 4;
        if offset + size > stream.len() {
            break;
        }
        if record_type == 0x0042 && size >= 2 {
            return read_u16_le(stream, offset);
        }
        offset += size;
    }
    None
}

fn read_et_text(path: &Path) -> Option<String> {
    if let Ok(result) = convert_spreadsheet(path) {
        if !result.markdown.trim().is_empty() {
            return Some(result.markdown);
        }
    }
    let stream = read_ole_stream(path, "Workbook");
    let mut text = String::new();
    if let Some(stream) = stream {
        if let Some(code_page) = detect_biff_code_page(&stream) {
            if code_page == 1200 {
                text = extract_utf16_text(&stream);
            } else if let Some(decoded) = decode_with_code_page(&stream, code_page) {
                text = decoded;
            }
        }
        if text.trim().is_empty() {
            text = extract_utf16_text(&stream);
        }
    }
    if text.trim().is_empty() {
        let data = std::fs::read(path).ok()?;
        text = extract_utf16_text(&data);
    }
    if text.trim().is_empty() {
        return None;
    }
    Some(format!("## ET Workbook\n\n{text}"))
}

fn is_likely_printable_word_char(ch: u16) -> bool {
    if ch == 0x0000 || ch == 0xFFFF || ch == 0xFFFE {
        return false;
    }
    if ch == 0x0009 || ch == 0x000A || ch == 0x000D || ch == 0x3000 {
        return true;
    }
    if (0x20..=0xD7FF).contains(&ch) {
        return true;
    }
    if (0xE000..=0xF8FF).contains(&ch) {
        return false;
    }
    ch < 0xF000
}

fn is_ppt_text_record_type(record_type: u16) -> bool {
    matches!(record_type, 0x0FA0 | 0x0FA8 | 0x0FBA | 0x0D45)
}

fn looks_utf16_text_payload(payload: &[u8]) -> bool {
    if payload.len() < 4 || payload.len() % 2 != 0 {
        return false;
    }
    let char_count = payload.len() / 2;
    if char_count == 0 {
        return false;
    }
    let mut printable = 0usize;
    let mut idx = 0usize;
    while idx + 1 < payload.len() {
        let value = u16::from_le_bytes([payload[idx], payload[idx + 1]]);
        if value == 0x0000 {
            idx += 2;
            continue;
        }
        if value == 0x0009 || value == 0x000A || value == 0x000D {
            printable += 1;
            idx += 2;
            continue;
        }
        if is_likely_printable_word_char(value) {
            printable += 1;
        }
        idx += 2;
    }
    printable * 2 >= char_count
}

fn looks_latin_text_payload(payload: &[u8]) -> bool {
    if payload.len() < 3 {
        return false;
    }
    let printable = payload
        .iter()
        .filter(|&&byte| {
            (byte >= 32 && byte <= 126) || byte == b'\r' || byte == b'\n' || byte == b'\t'
        })
        .count();
    printable * 2 >= payload.len()
}

fn decode_ppt_bytes(payload: &[u8]) -> String {
    if is_valid_utf8(payload) {
        return String::from_utf8_lossy(payload).to_string();
    }
    if let Some(decoded) = decode_with_code_pages(payload, &[936, 950, 932, 1252]) {
        return decoded;
    }
    latin1_to_string(payload)
}

fn decode_ppt_text_record(record_type: u16, payload: &[u8]) -> Option<String> {
    if !is_ppt_text_record_type(record_type) || payload.is_empty() {
        return None;
    }
    let decoded = if looks_utf16_text_payload(payload) {
        let mut u16s = Vec::with_capacity(payload.len() / 2);
        let mut idx = 0usize;
        while idx + 1 < payload.len() {
            u16s.push(u16::from_le_bytes([payload[idx], payload[idx + 1]]));
            idx += 2;
        }
        String::from_utf16_lossy(&u16s)
    } else if looks_latin_text_payload(payload) {
        decode_ppt_bytes(payload)
    } else {
        return None;
    };

    let mut cleaned = Vec::with_capacity(decoded.len());
    for &byte in decoded.as_bytes() {
        if byte == 0x00 {
            continue;
        }
        if byte == 0x0D || byte == 0x0B {
            cleaned.push(b'\n');
        } else if byte == 0x09 {
            cleaned.push(b' ');
        } else if byte >= 0x20 || (byte as i8) < 0 {
            cleaned.push(byte);
        }
    }
    let cleaned = String::from_utf8_lossy(&cleaned).to_string();
    if cleaned.trim().is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

#[derive(Default)]
struct PptTextBucket {
    lines: Vec<String>,
    seen: HashSet<String>,
}

fn collect_ppt_text_records(
    data: &[u8],
    offset: usize,
    length: usize,
    slides: &mut Vec<PptTextBucket>,
    loose: &mut PptTextBucket,
    current_slide_index: Option<usize>,
) {
    let end = offset.saturating_add(length);
    let mut pos = offset;
    while pos + 8 <= end && pos + 8 <= data.len() {
        let ver_inst = read_u16_le(data, pos).unwrap_or(0);
        let record_type = read_u16_le(data, pos + 2).unwrap_or(0);
        let size = read_u32_le(data, pos + 4).unwrap_or(0) as usize;
        let body_start = pos + 8;
        let body_end = body_start.saturating_add(size);
        if body_end > end || body_end > data.len() {
            break;
        }
        let rec_ver = ver_inst & 0x000F;
        let treat_as_slide_container = size > 0
            && ((rec_ver == 0x000F && matches!(record_type, 0x03EE | 0x03F8 | 0x0FF0))
                || record_type == 0x0FF1);
        if treat_as_slide_container {
            slides.push(PptTextBucket::default());
            let new_index = slides.len().saturating_sub(1);
            collect_ppt_text_records(data, body_start, size, slides, loose, Some(new_index));
            if slides
                .last()
                .map_or(false, |bucket| bucket.lines.is_empty())
            {
                slides.pop();
            }
        } else if rec_ver == 0x000F && size > 0 {
            collect_ppt_text_records(data, body_start, size, slides, loose, current_slide_index);
        } else if size > 0 {
            if let Some(decoded) = decode_ppt_text_record(record_type, &data[body_start..body_end])
            {
                for line in split_lines(&decoded) {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed.contains('\u{FFFD}') {
                        continue;
                    }
                    if !looks_like_document_text(trimmed) {
                        continue;
                    }
                    if let Some(idx) = current_slide_index {
                        if let Some(bucket) = slides.get_mut(idx) {
                            if bucket.seen.insert(trimmed.to_string()) {
                                bucket.lines.push(trimmed.to_string());
                            }
                        } else if loose.seen.insert(trimmed.to_string()) {
                            loose.lines.push(trimmed.to_string());
                        }
                    } else if loose.seen.insert(trimmed.to_string()) {
                        loose.lines.push(trimmed.to_string());
                    }
                }
            }
        }
        pos = body_end;
    }
}

fn format_dps_slides(slides: &[PptTextBucket]) -> Option<String> {
    let mut sections = Vec::new();
    let mut index = 1usize;
    for slide in slides {
        if slide.lines.is_empty() {
            continue;
        }
        let list = format_markdown_list(&slide.lines.join("\n"));
        if list.is_empty() {
            continue;
        }
        sections.push(format!("## Slide {index}\n\n{list}"));
        index += 1;
    }
    if sections.is_empty() {
        None
    } else {
        Some(sections.join("\n\n"))
    }
}

fn read_dps_via_ppt_binary(path: &Path) -> Option<String> {
    let stream = read_ole_stream(path, "PowerPoint Document")?;
    let mut slides = Vec::new();
    let mut loose = PptTextBucket::default();
    collect_ppt_text_records(&stream, 0, stream.len(), &mut slides, &mut loose, None);
    if let Some(structured) = format_dps_slides(&slides) {
        return Some(structured);
    }
    if loose.lines.is_empty() {
        None
    } else {
        Some(loose.lines.join("\n"))
    }
}

fn read_dps_text(path: &Path) -> Option<String> {
    if let Some(text) = read_dps_via_ppt_binary(path) {
        if !text.trim().is_empty() {
            return Some(text);
        }
    }
    let stream = read_ole_stream(path, "PowerPoint Document")?;
    let mut text = extract_utf16_text(&stream);
    if text.trim().is_empty() {
        if let Some(decoded) = decode_with_code_pages(&stream, &[936, 950, 932, 1252]) {
            text = decoded;
        }
    }
    if text.trim().is_empty() {
        return None;
    }
    if text.starts_with("## Slide ") {
        return Some(text);
    }
    let list = format_markdown_list(&text);
    if list.is_empty() {
        Some(text)
    } else {
        Some(format!("## DPS Slides\n\n{list}"))
    }
}

const OLE_FREE_SECTOR: u32 = 0xFFFF_FFFF;
const OLE_END_OF_CHAIN: u32 = 0xFFFF_FFFE;

#[derive(Debug, Clone)]
struct OleDirectoryEntry {
    name: String,
    entry_type: u8,
    start_sector: u32,
    size: u64,
}

struct OleReader {
    data: Vec<u8>,
    fat: Vec<u32>,
    mini_fat: Vec<u32>,
    entries: Vec<OleDirectoryEntry>,
    mini_stream: Vec<u8>,
    sector_shift: u16,
    mini_sector_shift: u16,
    mini_stream_cutoff: u32,
    first_dir_sector: u32,
    first_mini_fat_sector: u32,
    num_mini_fat_sectors: u32,
    major_version: u16,
}

impl OleReader {
    fn load(path: &Path) -> Option<Self> {
        let data = std::fs::read(path).ok()?;
        if data.len() < 512 {
            return None;
        }
        let signature = read_u64_le(&data, 0)?;
        if signature != 0xE11A_B1A1_E011_CFD0 {
            return None;
        }
        let major_version = read_u16_le(&data, 0x1A)?;
        let byte_order = read_u16_le(&data, 0x1C)?;
        if byte_order != 0xFFFE {
            return None;
        }
        let sector_shift = read_u16_le(&data, 0x1E)?;
        let mini_sector_shift = read_u16_le(&data, 0x20)?;
        let mini_stream_cutoff = read_u32_le(&data, 0x38)?;
        let first_dir_sector = read_u32_le(&data, 0x30)?;
        let first_mini_fat_sector = read_u32_le(&data, 0x3C)?;
        let num_mini_fat_sectors = read_u32_le(&data, 0x40)?;
        let first_difat_sector = read_u32_le(&data, 0x44)?;
        let mut num_difat_sectors = read_u32_le(&data, 0x48)?;

        let mut reader = OleReader {
            data,
            fat: Vec::new(),
            mini_fat: Vec::new(),
            entries: Vec::new(),
            mini_stream: Vec::new(),
            sector_shift,
            mini_sector_shift,
            mini_stream_cutoff,
            first_dir_sector,
            first_mini_fat_sector,
            num_mini_fat_sectors,
            major_version,
        };

        let mut difat = Vec::new();
        let difat_head = 0x4C;
        for i in 0..109 {
            let entry = read_u32_le(&reader.data, difat_head + i * 4)?;
            if entry != OLE_FREE_SECTOR {
                difat.push(entry);
            }
        }
        let mut difat_sector = first_difat_sector;
        while num_difat_sectors > 0 && difat_sector != OLE_END_OF_CHAIN {
            let block = reader.sector_data(difat_sector)?.to_vec();
            let ints_per_sector = block.len() / 4;
            let mut offset = 0usize;
            for _ in 0..ints_per_sector.saturating_sub(1) {
                let value = read_u32_le(&block, offset)?;
                if value != OLE_FREE_SECTOR {
                    difat.push(value);
                }
                offset += 4;
            }
            difat_sector = read_u32_le(&block, block.len().saturating_sub(4))?;
            num_difat_sectors = num_difat_sectors.saturating_sub(1);
        }

        if !reader.build_fat(&difat) {
            return None;
        }
        if !reader.build_mini_fat() {
            return None;
        }
        if !reader.build_directory() {
            return None;
        }
        if !reader.build_mini_stream() {
            return None;
        }
        Some(reader)
    }

    fn sector_size(&self) -> usize {
        1usize << self.sector_shift
    }

    fn mini_sector_size(&self) -> usize {
        1usize << self.mini_sector_shift
    }

    fn sector_offset(&self, sector: u32) -> Option<usize> {
        if sector == OLE_END_OF_CHAIN {
            return None;
        }
        let offset = 512usize + sector as usize * self.sector_size();
        if offset + self.sector_size() > self.data.len() {
            return None;
        }
        Some(offset)
    }

    fn sector_data(&self, sector: u32) -> Option<&[u8]> {
        let offset = self.sector_offset(sector)?;
        Some(&self.data[offset..offset + self.sector_size()])
    }

    fn build_fat(&mut self, difat: &[u32]) -> bool {
        if difat.is_empty() {
            return false;
        }
        let ints_per_sector = self.sector_size() / 4;
        for &sector in difat {
            let block = match self.sector_data(sector) {
                Some(data) => data.to_vec(),
                None => continue,
            };
            for i in 0..ints_per_sector {
                if let Some(value) = read_u32_le(&block, i * 4) {
                    self.fat.push(value);
                }
            }
        }
        !self.fat.is_empty()
    }

    fn build_mini_fat(&mut self) -> bool {
        self.mini_fat.clear();
        if self.first_mini_fat_sector == OLE_END_OF_CHAIN || self.num_mini_fat_sectors == 0 {
            return true;
        }
        let ints_per_sector = self.sector_size() / 4;
        let mut sector = self.first_mini_fat_sector;
        let mut remaining = self.num_mini_fat_sectors;
        while remaining > 0 && sector != OLE_END_OF_CHAIN {
            let block = match self.sector_data(sector) {
                Some(data) => data.to_vec(),
                None => break,
            };
            for i in 0..ints_per_sector {
                if let Some(value) = read_u32_le(&block, i * 4) {
                    self.mini_fat.push(value);
                }
            }
            sector = self.next_sector(sector);
            remaining = remaining.saturating_sub(1);
        }
        true
    }

    fn build_directory(&mut self) -> bool {
        let dir_stream = match self.read_stream(self.first_dir_sector, 0, false) {
            Some(stream) => stream,
            None => return false,
        };
        let entry_size = 128usize;
        let count = dir_stream.len() / entry_size;
        for i in 0..count {
            let base = &dir_stream[i * entry_size..(i + 1) * entry_size];
            let name_len = read_u16_le(base, 64).unwrap_or(0);
            if name_len < 2 {
                continue;
            }
            let char_count = std::cmp::min(32usize, (name_len as usize / 2).saturating_sub(1));
            let mut u16s = Vec::with_capacity(char_count);
            let mut idx = 0usize;
            while idx < char_count * 2 {
                let value = read_u16_le(base, idx).unwrap_or(0);
                u16s.push(value);
                idx += 2;
            }
            let name = String::from_utf16_lossy(&u16s);
            let entry_type = base.get(66).copied().unwrap_or(0);
            let start_sector = read_u32_le(base, 116).unwrap_or(OLE_END_OF_CHAIN);
            let size = if self.major_version >= 4 {
                read_u64_le(base, 120).unwrap_or(0)
            } else {
                read_u32_le(base, 120).unwrap_or(0) as u64
            };
            self.entries.push(OleDirectoryEntry {
                name,
                entry_type,
                start_sector,
                size,
            });
        }
        !self.entries.is_empty()
    }

    fn build_mini_stream(&mut self) -> bool {
        let root = self.entries.iter().find(|entry| entry.entry_type == 5);
        let root = match root {
            Some(entry) => entry,
            None => return false,
        };
        let stream = match self.read_stream(root.start_sector, root.size as usize, false) {
            Some(stream) => stream,
            None => return false,
        };
        self.mini_stream = stream;
        true
    }

    fn read_stream(&self, start_sector: u32, size: usize, use_mini: bool) -> Option<Vec<u8>> {
        if start_sector == OLE_END_OF_CHAIN {
            return None;
        }
        if use_mini {
            let mut buffer = Vec::new();
            let mut sector = start_sector;
            let mut remaining = size;
            let mini_size = self.mini_sector_size();
            while sector != OLE_END_OF_CHAIN && (remaining > 0 || size == 0) {
                let offset = sector as usize * mini_size;
                if offset + mini_size > self.mini_stream.len() {
                    break;
                }
                let chunk = if size == 0 {
                    mini_size
                } else {
                    std::cmp::min(remaining, mini_size)
                };
                buffer.extend_from_slice(&self.mini_stream[offset..offset + chunk]);
                if size != 0 {
                    remaining = remaining.saturating_sub(chunk);
                    if remaining == 0 {
                        break;
                    }
                }
                sector = if (sector as usize) < self.mini_fat.len() {
                    self.mini_fat[sector as usize]
                } else {
                    OLE_END_OF_CHAIN
                };
            }
            if size != 0 && buffer.len() > size {
                buffer.truncate(size);
            }
            return Some(buffer);
        }

        let mut buffer = Vec::new();
        let mut sector = start_sector;
        let mut remaining = size;
        let sector_size = self.sector_size();
        while sector != OLE_END_OF_CHAIN && (remaining > 0 || size == 0) {
            let offset = match self.sector_offset(sector) {
                Some(offset) => offset,
                None => break,
            };
            let chunk = if size == 0 {
                sector_size
            } else {
                std::cmp::min(remaining, sector_size)
            };
            buffer.extend_from_slice(&self.data[offset..offset + chunk]);
            if size != 0 {
                remaining = remaining.saturating_sub(chunk);
                if remaining == 0 {
                    break;
                }
            }
            sector = self.next_sector(sector);
        }
        if size != 0 && buffer.len() > size {
            buffer.truncate(size);
        }
        Some(buffer)
    }

    fn next_sector(&self, current: u32) -> u32 {
        let idx = current as usize;
        if idx >= self.fat.len() {
            OLE_END_OF_CHAIN
        } else {
            self.fat[idx]
        }
    }

    fn stream_by_name(&self, name: &str) -> Option<Vec<u8>> {
        for entry in &self.entries {
            if entry.entry_type != 2 {
                continue;
            }
            if !entry.name.eq_ignore_ascii_case(name) {
                continue;
            }
            let use_mini =
                entry.size < self.mini_stream_cutoff as u64 && !self.mini_stream.is_empty();
            return self.read_stream(entry.start_sector, entry.size as usize, use_mini);
        }
        None
    }
}

fn read_ole_stream(path: &Path, name: &str) -> Option<Vec<u8>> {
    if let Ok(file) = File::open(path) {
        if let Ok(mut ole) = cfb::CompoundFile::open(file) {
            if let Ok(mut stream) = ole.open_stream(name) {
                let mut data = Vec::new();
                if stream.read_to_end(&mut data).is_ok() {
                    return Some(data);
                }
            }
        }
    }
    let reader = OleReader::load(path)?;
    reader.stream_by_name(name)
}

fn split_tag_name(name: &[u8]) -> (Option<&[u8]>, &[u8]) {
    if let Some(idx) = name.iter().position(|b| *b == b':') {
        (Some(&name[..idx]), &name[idx + 1..])
    } else {
        (None, name)
    }
}

fn local_name(name: &[u8]) -> &[u8] {
    split_tag_name(name).1
}

fn attr_value<B: std::io::BufRead>(
    reader: &XmlReader<B>,
    element: &BytesStart,
    key: &[u8],
) -> Option<String> {
    for attr in element.attributes().with_checks(false) {
        let attr = attr.ok()?;
        let (_, local) = split_tag_name(attr.key.as_ref());
        if local == key {
            if let Ok(value) = attr.decode_and_unescape_value(reader) {
                return Some(value.into_owned());
            }
        }
    }
    None
}

fn parse_outline_level<B: std::io::BufRead>(
    reader: &XmlReader<B>,
    element: &BytesStart,
) -> Option<usize> {
    for attr in element.attributes().with_checks(false) {
        let attr = attr.ok()?;
        let (_, local) = split_tag_name(attr.key.as_ref());
        if local == b"outline-level" {
            if let Ok(value) = attr.decode_and_unescape_value(reader) {
                if let Ok(level) = value.parse::<usize>() {
                    return Some(level.max(1).min(6));
                }
            }
        }
    }
    None
}
