//! Native single-file pickers for the desktop settings flow. Windows uses the
//! classic common dialog through the already-linked windows-sys bindings; other
//! desktop platforms fall back to the dialog helpers that ship with the desktop
//! environment, and return None when none is available.

/// Pick one supplement archive; `None` means the user cancelled or no native
/// picker is available on this platform.
pub fn pick_supplement_archive() -> Option<String> {
    #[cfg(windows)]
    {
        pick_windows()
    }
    #[cfg(not(windows))]
    {
        pick_linux()
    }
}

/// Pick one skill archive for the tools page import; the accepted list matches
/// the web skill upload accept attribute.
pub fn pick_skill_archive() -> Option<String> {
    #[cfg(windows)]
    {
        pick_windows_skill()
    }
    #[cfg(not(windows))]
    {
        pick_linux_skill()
    }
}

/// Pick one companion (dynamic avatar) package; the web dialog accepts zips.
pub fn pick_companion_package() -> Option<String> {
    #[cfg(windows)]
    {
        pick_windows_companion()
    }
    #[cfg(not(windows))]
    {
        pick_linux_companion()
    }
}

/// Pick an executable file (python / git / rg / ffmpeg / browser / shell) for a
/// runtime tool path; `None` means the user cancelled or no native picker is
/// available.
pub fn pick_executable(tool: &str) -> Option<String> {
    let filter_label = match tool {
        "python" => "Python 可执行文件",
        "git" => "Git 可执行文件",
        "rg" => "ripgrep 可执行文件",
        "ffmpeg" => "ffmpeg 可执行文件",
        "browser" => "浏览器可执行文件",
        "shell" => "shell 可执行文件",
        _ => "可执行文件",
    };
    #[cfg(windows)]
    {
        pick_windows_executable(filter_label)
    }
    #[cfg(not(windows))]
    {
        pick_linux_executable(tool, filter_label)
    }
}

#[cfg(windows)]
fn pick_windows() -> Option<String> {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::UI::Controls::Dialogs::{
        GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_HIDEREADONLY, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST,
        OPENFILENAMEW,
    };

    let filter = OsString::from("补充包\0*.zip;*.tar.gz;*.tgz;*.tar\0所有文件\0*.*\0\0")
        .encode_wide()
        .collect::<Vec<u16>>();
    // The buffer doubles as the multi-selection output; one file is enough.
    let mut file_buffer = vec![0u16; 32768];
    let mut dialog: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    dialog.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    dialog.lpstrFilter = filter.as_ptr();
    dialog.lpstrFile = file_buffer.as_mut_ptr();
    dialog.nMaxFile = file_buffer.len() as u32;
    dialog.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_HIDEREADONLY | OFN_NOCHANGEDIR;
    let picked = unsafe { GetOpenFileNameW(&mut dialog) };
    if picked == 0 {
        return None;
    }
    let end = file_buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(file_buffer.len());
    let path = OsString::from_wide(&file_buffer[..end])
        .to_string_lossy()
        .into_owned();
    if path.trim().is_empty() {
        None
    } else {
        Some(path)
    }
}

#[cfg(windows)]
fn pick_windows_skill() -> Option<String> {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::UI::Controls::Dialogs::{
        GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_HIDEREADONLY, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST,
        OPENFILENAMEW,
    };

    let filter = OsString::from(concat!(
        "技能压缩包\0*.zip;*.skill;*.rar;*.7z;*.tar;*.tgz;*.tar.gz;*.tbz2;*.tar.bz2;*.txz;*.tar.xz\0",
        "所有文件\0*.*\0\0"
    ))
    .encode_wide()
    .collect::<Vec<u16>>();
    let mut file_buffer = vec![0u16; 32768];
    let mut dialog: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    dialog.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    dialog.lpstrFilter = filter.as_ptr();
    dialog.lpstrFile = file_buffer.as_mut_ptr();
    dialog.nMaxFile = file_buffer.len() as u32;
    dialog.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_HIDEREADONLY | OFN_NOCHANGEDIR;
    let picked = unsafe { GetOpenFileNameW(&mut dialog) };
    if picked == 0 {
        return None;
    }
    let end = file_buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(file_buffer.len());
    let path = OsString::from_wide(&file_buffer[..end])
        .to_string_lossy()
        .into_owned();
    if path.trim().is_empty() {
        None
    } else {
        Some(path)
    }
}

#[cfg(windows)]
fn pick_windows_companion() -> Option<String> {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::UI::Controls::Dialogs::{
        GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_HIDEREADONLY, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST,
        OPENFILENAMEW,
    };

    let filter = OsString::from("形象包\0*.zip\0所有文件\0*.*\0\0")
        .encode_wide()
        .collect::<Vec<u16>>();
    let mut file_buffer = vec![0u16; 32768];
    let mut dialog: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    dialog.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    dialog.lpstrFilter = filter.as_ptr();
    dialog.lpstrFile = file_buffer.as_mut_ptr();
    dialog.nMaxFile = file_buffer.len() as u32;
    dialog.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_HIDEREADONLY | OFN_NOCHANGEDIR;
    let picked = unsafe { GetOpenFileNameW(&mut dialog) };
    if picked == 0 {
        return None;
    }
    let end = file_buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(file_buffer.len());
    let path = OsString::from_wide(&file_buffer[..end])
        .to_string_lossy()
        .into_owned();
    if path.trim().is_empty() {
        None
    } else {
        Some(path)
    }
}

#[cfg(windows)]
fn pick_windows_executable(filter_label: &str) -> Option<String> {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::UI::Controls::Dialogs::{
        GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_HIDEREADONLY, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST,
        OPENFILENAMEW,
    };

    let executables = OsString::from("可执行文件\0*.exe;*.bat;*.cmd\0所有文件\0*.*\0\0")
        .encode_wide()
        .collect::<Vec<u16>>();
    let label = OsString::from(filter_label)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<u16>>();
    // The buffer doubles as the multi-selection output; one file is enough.
    let mut file_buffer = vec![0u16; 32768];
    let mut dialog: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    dialog.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    dialog.lpstrFilter = executables.as_ptr();
    dialog.lpstrFile = file_buffer.as_mut_ptr();
    dialog.nMaxFile = file_buffer.len() as u32;
    dialog.lpstrTitle = label.as_ptr();
    dialog.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_HIDEREADONLY | OFN_NOCHANGEDIR;
    let picked = unsafe { GetOpenFileNameW(&mut dialog) };
    if picked == 0 {
        return None;
    }
    let end = file_buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(file_buffer.len());
    let path = OsString::from_wide(&file_buffer[..end])
        .to_string_lossy()
        .into_owned();
    if path.trim().is_empty() {
        None
    } else {
        Some(path)
    }
}

#[cfg(not(windows))]
fn pick_linux() -> Option<String> {
    for (program, args) in [
        (
            "zenity",
            vec![
                "--file-selection".to_string(),
                "--file-filter=补充包 | *.zip *.tar.gz *.tgz *.tar".to_string(),
            ],
        ),
        (
            "kdialog",
            vec!["--getopenfilename".to_string(), ". *.zip".to_string()],
        ),
    ] {
        if let Ok(output) = std::process::Command::new(program).args(&args).output() {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    return Some(path);
                }
            }
        }
    }
    None
}

#[cfg(not(windows))]
fn pick_linux_skill() -> Option<String> {
    for (program, args) in [
        (
            "zenity",
            vec![
                "--file-selection".to_string(),
                "--file-filter=技能压缩包 | *.zip *.skill *.rar *.7z *.tar *.tgz *.tar.gz *.tbz2 *.tar.bz2 *.txz *.tar.xz".to_string(),
            ],
        ),
        (
            "kdialog",
            vec![
                "--getopenfilename".to_string(),
                ". *.zip *.skill *.rar *.7z *.tar *.tgz *.tar.gz *.tbz2 *.tar.bz2 *.txz *.tar.xz".to_string(),
            ],
        ),
    ] {
        if let Ok(output) = std::process::Command::new(program).args(&args).output() {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    return Some(path);
                }
            }
        }
    }
    None
}

#[cfg(not(windows))]
fn pick_linux_companion() -> Option<String> {
    for (program, args) in [
        (
            "zenity",
            vec![
                "--file-selection".to_string(),
                "--file-filter=形象包 | *.zip".to_string(),
            ],
        ),
        (
            "kdialog",
            vec!["--getopenfilename".to_string(), ". *.zip".to_string()],
        ),
    ] {
        if let Ok(output) = std::process::Command::new(program).args(&args).output() {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    return Some(path);
                }
            }
        }
    }
    None
}

#[cfg(not(windows))]
fn pick_linux_executable(tool: &str, filter_label: &str) -> Option<String> {
    for (program, args) in [
        (
            "zenity",
            vec![
                "--file-selection".to_string(),
                format!("--file-filter={filter_label} | *.{tool}"),
            ],
        ),
        (
            "kdialog",
            vec!["--getopenfilename".to_string(), format!(". *.{tool}")],
        ),
    ] {
        if let Ok(output) = std::process::Command::new(program).args(&args).output() {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    return Some(path);
                }
            }
        }
    }
    None
}

/// §8.2 attachment picker: multi-select files for the composer. Returns the
/// selected absolute paths; an empty vector means the dialog was cancelled.
pub fn pick_attachments() -> Result<Vec<String>, String> {
    #[cfg(windows)]
    {
        Ok(pick_windows_attachments())
    }
    #[cfg(not(windows))]
    {
        Ok(pick_linux_attachments())
    }
}

/// Reads one picked file into the composer's attachment shape: a `data:` URL
/// plus the detected content type. Files above the cap are skipped by the
/// caller so one huge pick cannot balloon the message.
pub fn read_attachment(path: &str) -> Option<crate::ChatAttachment> {
    use base64::Engine;
    const MAX_ATTACHMENT_BYTES: u64 = 8 * 1024 * 1024;
    let path_buf = std::path::PathBuf::from(path);
    let name = path_buf
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string());
    let metadata = std::fs::metadata(&path_buf).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_ATTACHMENT_BYTES {
        return None;
    }
    let bytes = std::fs::read(&path_buf).ok()?;
    let mime = attachment_mime(&path_buf);
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Some(crate::ChatAttachment {
        name: name.into(),
        mime_type: mime.into(),
        data_url: format!("data:{mime};base64,{encoded}").into(),
    })
}

/// Content type from the extension. The runtime forwards this verbatim, so a
/// wrong guess would surface as a provider error; unknown types fall back to
/// `application/octet-stream` rather than pretending to be text.
fn attachment_mime(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .map(|value| value.to_string_lossy().to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        Some("svg") => "image/svg+xml",
        Some("pdf") => "application/pdf",
        Some("json") => "application/json",
        Some("txt") | Some("log") | Some("md") | Some("csv") => "text/plain",
        _ => "application/octet-stream",
    }
}

#[cfg(windows)]
fn pick_windows_attachments() -> Vec<String> {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::UI::Controls::Dialogs::{
        GetOpenFileNameW, OFN_ALLOWMULTISELECT, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_HIDEREADONLY,
        OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPENFILENAMEW,
    };

    // An Explorer-style multi-select returns "directory\0name1\0name2\0\0";
    // a single selection returns the full path in one field.
    let filter = OsString::from(
        "图片\0*.png;*.jpg;*.jpeg;*.gif;*.webp;*.bmp\0文本\0*.txt;*.log;*.md;*.json;*.csv\0所有文件\0*.*\0\0",
    )
    .encode_wide()
    .collect::<Vec<u16>>();
    let mut file_buffer = vec![0u16; 65536];
    let mut dialog: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    dialog.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    dialog.lpstrFilter = filter.as_ptr();
    dialog.lpstrFile = file_buffer.as_mut_ptr();
    dialog.nMaxFile = file_buffer.len() as u32;
    dialog.Flags = OFN_FILEMUSTEXIST
        | OFN_PATHMUSTEXIST
        | OFN_HIDEREADONLY
        | OFN_NOCHANGEDIR
        | OFN_ALLOWMULTISELECT
        | OFN_EXPLORER;
    let picked = unsafe { GetOpenFileNameW(&mut dialog) };
    if picked == 0 {
        return Vec::new();
    }
    // The buffer holds NUL-separated segments ending in a double NUL; the first
    // segment is the directory when more than one follows.
    let mut segments: Vec<String> = Vec::new();
    let mut start = 0usize;
    for (index, unit) in file_buffer.iter().enumerate() {
        if *unit != 0 {
            continue;
        }
        if index == start {
            break;
        }
        segments.push(
            OsString::from_wide(&file_buffer[start..index])
                .to_string_lossy()
                .into_owned(),
        );
        start = index + 1;
    }
    match segments.len() {
        0 => Vec::new(),
        1 => vec![segments.remove(0)],
        _ => {
            let directory = segments.remove(0);
            segments
                .into_iter()
                .map(|name| {
                    std::path::Path::new(&directory)
                        .join(name)
                        .to_string_lossy()
                        .into_owned()
                })
                .collect()
        }
    }
}

#[cfg(not(windows))]
fn pick_linux_attachments() -> Vec<String> {
    // zenity is the only picker available without extra crates; a missing
    // binary simply yields no selection.
    let output = std::process::Command::new("zenity")
        .args(["--file-selection", "--multiple", "--separator=\n"])
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

/// Save-as dialog for exports; the caller writes the file itself.
pub fn save_file(title: &str, default_name: &str) -> Option<String> {
    #[cfg(windows)]
    {
        save_windows_file_dialog(title, default_name)
    }
    #[cfg(not(windows))]
    {
        save_linux_file(title, default_name)
    }
}

#[cfg(windows)]
fn save_windows_file_dialog(title: &str, default_name: &str) -> Option<String> {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::UI::Controls::Dialogs::{
        GetSaveFileNameW, OFN_HIDEREADONLY, OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT,
        OFN_PATHMUSTEXIST, OPENFILENAMEW,
    };

    let label = OsString::from(title)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<u16>>();
    let seed = OsString::from(default_name)
        .encode_wide()
        .take(32767)
        .collect::<Vec<u16>>();
    let mut file_buffer = vec![0u16; 32768];
    file_buffer[..seed.len()].copy_from_slice(&seed);
    let mut dialog: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    dialog.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    dialog.lpstrFile = file_buffer.as_mut_ptr();
    dialog.nMaxFile = file_buffer.len() as u32;
    dialog.lpstrTitle = label.as_ptr();
    dialog.Flags = OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST | OFN_HIDEREADONLY | OFN_NOCHANGEDIR;
    let picked = unsafe { GetSaveFileNameW(&mut dialog) };
    if picked == 0 {
        return None;
    }
    let end = file_buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(file_buffer.len());
    let path = OsString::from_wide(&file_buffer[..end])
        .to_string_lossy()
        .into_owned();
    if path.trim().is_empty() {
        None
    } else {
        Some(path)
    }
}

#[cfg(not(windows))]
fn save_linux_file(title: &str, default_name: &str) -> Option<String> {
    for (program, args) in [
        (
            "zenity",
            vec![
                "--file-selection".to_string(),
                "--save".to_string(),
                "--confirm-overwrite".to_string(),
                format!("--title={title}"),
                format!("--filename={default_name}"),
            ],
        ),
        (
            "kdialog",
            vec!["--getsavefilename".to_string(), format!(". {default_name}")],
        ),
    ] {
        if let Ok(output) = std::process::Command::new(program).args(&args).output() {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    return Some(path);
                }
            }
        }
    }
    None
}

/// Pick an existing directory for workspace binding; `None` means the user
/// cancelled or no native picker is available on this platform.
pub fn pick_directory(title: &str) -> Option<String> {
    #[cfg(windows)]
    {
        pick_windows_directory(title)
    }
    #[cfg(not(windows))]
    {
        pick_linux_directory(title)
    }
}

#[cfg(windows)]
fn pick_windows_directory(title: &str) -> Option<String> {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys_dialog::Win32::System::Com::CoTaskMemFree;
    use windows_sys_dialog::Win32::UI::Shell::{
        SHBrowseForFolderW, SHGetPathFromIDListW, BIF_NEWDIALOGSTYLE, BIF_RETURNONLYFSDIRS,
        BROWSEINFOW,
    };

    let mut display = [0u16; 260];
    let mut title_wide: Vec<u16> = OsString::from(title).encode_wide().collect();
    title_wide.push(0);
    let mut dialog: BROWSEINFOW = unsafe { std::mem::zeroed() };
    dialog.hwndOwner = std::ptr::null_mut();
    dialog.pszDisplayName = display.as_mut_ptr();
    dialog.lpszTitle = title_wide.as_ptr();
    dialog.ulFlags = BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE;
    let pidl = unsafe { SHBrowseForFolderW(&dialog) };
    if pidl.is_null() {
        return None;
    }
    let mut path = [0u16; 260];
    let ok = unsafe { SHGetPathFromIDListW(pidl, path.as_mut_ptr()) };
    unsafe { CoTaskMemFree(pidl.cast()) };
    if ok == 0 {
        return None;
    }
    let end = path
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(path.len());
    let picked = OsString::from_wide(&path[..end])
        .to_string_lossy()
        .into_owned();
    if picked.trim().is_empty() {
        None
    } else {
        Some(picked)
    }
}

#[cfg(not(windows))]
fn pick_linux_directory(title: &str) -> Option<String> {
    for (program, args) in [
        (
            "zenity",
            vec![
                "--file-selection".to_string(),
                "--directory".to_string(),
                format!("--title={}", title),
            ],
        ),
        (
            "kdialog",
            vec![
                "--getexistingdirectory".to_string(),
                ".".to_string(),
                format!("--title={}", title),
            ],
        ),
    ] {
        if let Ok(output) = std::process::Command::new(program).args(&args).output() {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    return Some(path);
                }
            }
        }
    }
    None
}
