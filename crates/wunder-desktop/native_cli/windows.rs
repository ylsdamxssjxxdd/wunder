use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Threading::{
    CreateProcessW, CREATE_NEW_CONSOLE, CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION,
    STARTUPINFOW,
};

fn wide(value: &OsStr) -> std::io::Result<Vec<u16>> {
    let mut value = value.encode_wide().collect::<Vec<_>>();
    if value.contains(&0) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "process argument contains NUL",
        ));
    }
    value.push(0);
    Ok(value)
}

fn argument(value: &OsStr, output: &mut Vec<u16>) -> std::io::Result<()> {
    let value = wide(value)?;
    output.push(u16::from(b'"'));
    let mut backslashes = 0;
    for character in &value[..value.len() - 1] {
        if *character == u16::from(b'\\') {
            backslashes += 1;
            continue;
        }
        let count = if *character == u16::from(b'"') {
            backslashes * 2 + 1
        } else {
            backslashes
        };
        output.extend(std::iter::repeat_n(u16::from(b'\\'), count));
        output.push(*character);
        backslashes = 0;
    }
    output.extend(std::iter::repeat_n(u16::from(b'\\'), backslashes * 2));
    output.push(u16::from(b'"'));
    Ok(())
}

pub(super) fn spawn(
    executable: &Path,
    arguments: &[OsString],
    workdir: &Path,
    home: &Path,
    path: &OsStr,
) -> std::io::Result<()> {
    let application = wide(executable.as_os_str())?;
    let workdir = wide(workdir.as_os_str())?;
    let mut command_line = Vec::new();
    argument(executable.as_os_str(), &mut command_line)?;
    for value in arguments {
        command_line.push(u16::from(b' '));
        argument(value, &mut command_line)?;
    }
    command_line.push(0);
    let mut entries = std::env::vars_os()
        .filter(|(key, _)| {
            let key = key.to_string_lossy();
            !key.eq_ignore_ascii_case("PATH") && !key.eq_ignore_ascii_case("WUNDER_HOME")
        })
        .collect::<Vec<_>>();
    entries.extend([
        (OsString::from("WUNDER_HOME"), home.as_os_str().to_owned()),
        (OsString::from("PATH"), path.to_owned()),
    ]);
    entries.sort_by_cached_key(|(key, _)| key.to_string_lossy().to_uppercase());
    let mut environment = Vec::new();
    for (key, value) in entries {
        let mut entry = key;
        entry.push("=");
        entry.push(value);
        environment.extend(wide(&entry)?);
    }
    environment.push(0);
    let startup = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    let mut process: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    let result = unsafe {
        CreateProcessW(
            application.as_ptr(),
            command_line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            CREATE_NEW_CONSOLE | CREATE_UNICODE_ENVIRONMENT,
            environment.as_ptr().cast(),
            workdir.as_ptr(),
            &startup,
            &mut process,
        )
    };
    if result == 0 {
        return Err(std::io::Error::last_os_error());
    }
    unsafe {
        CloseHandle(process.hThread);
        CloseHandle(process.hProcess);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_arguments_escape_quotes_trailing_slashes_and_empty_values() {
        for (value, expected) in [
            ("", "\"\""),
            ("sample & value", "\"sample & value\""),
            ("sample\\", "\"sample\\\\\""),
            ("sample\"value", "\"sample\\\"value\""),
        ] {
            let mut output = Vec::new();
            argument(OsStr::new(value), &mut output).unwrap();
            assert_eq!(String::from_utf16(&output).unwrap(), expected);
        }
        assert!(argument(OsStr::new("sample\0value"), &mut Vec::new()).is_err());
    }
}
