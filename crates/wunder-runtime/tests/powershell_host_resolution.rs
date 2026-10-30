//! Verifies that the command tool launches the PowerShell host it probed.
//!
//! The bundled portable PowerShell 7 is named `pwsh.exe`, so a bare
//! `powershell.exe` lookup can never reach it: the runtime has to resolve
//! the executable by absolute path, start it, and read the version back from
//! that same binary. This test points the runtime at a real PowerShell and
//! checks that the launcher, the reported version and the display name all
//! describe one interpreter.
//!
//! It also pins down the import path: a supplement import publishes a new
//! `WUNDER_POWERSHELL_EXE` while the process is already running, so the
//! cached probe has to be droppable. Without that the portable PowerShell
//! would only take effect after a restart.
//!
//! Run with the path to a PowerShell executable:
//!   WUNDER_TEST_PWSH="C:\...\pwsh.exe" cargo test -p wunder-runtime --test powershell_host_resolution

#![cfg(windows)]

use std::path::{Path, PathBuf};

use wunder_server::command_utils::{
    self, ShellKind, ShellLaunch, build_shell_command_with_shell, powershell_display_name,
    powershell_major_version,
};

fn candidate() -> Option<PathBuf> {
    if let Ok(value) = std::env::var("WUNDER_TEST_PWSH") {
        let path = PathBuf::from(value);
        if path.is_file() {
            return Some(path);
        }
    }
    // Fall back to the supplement staged under the repository, which is where
    // the portable build lands after a supplement import.
    let staged = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../supplement-build/stage/package-root/opt/powershell/pwsh.exe");
    if staged.is_file() {
        return Some(staged);
    }
    None
}

/// One test on purpose: the override variable and the probed-host cache are
/// process-global, so splitting the scenarios into parallel tests would let
/// them race over the same state.
#[test]
fn runtime_launches_the_probed_powershell_host() {
    let Some(exe) = candidate() else {
        eprintln!("skipping: no PowerShell to probe (set WUNDER_TEST_PWSH)");
        return;
    };

    // --- Baseline: nothing published, so the runtime falls back to the OS
    // --- install (or reports that it has none).
    std::env::remove_var(command_utils::POWERSHELL_EXE_ENV);
    command_utils::reset_powershell_host();
    let baseline = command_utils::powershell_host();

    // --- The supplement import publishes the bundled executable and clears
    // --- the cache; the next lookup has to adopt it.
    std::env::set_var(command_utils::POWERSHELL_EXE_ENV, &exe);
    command_utils::reset_powershell_host();
    let adopted = command_utils::powershell_host().expect("the override must be adopted");

    assert_eq!(
        adopted.exe.to_string_lossy().to_ascii_lowercase(),
        exe.to_string_lossy().to_ascii_lowercase(),
        "the newly published override was not adopted"
    );
    assert!(
        adopted.bundled,
        "an override-sourced host must be marked as bundled"
    );
    if let Some(baseline) = baseline {
        assert_ne!(
            baseline.exe.to_string_lossy().to_ascii_lowercase(),
            exe.to_string_lossy().to_ascii_lowercase(),
            "the override has to change the resolved host"
        );
    }

    // --- The launcher must be the probed binary, not a PATH lookup of
    // --- powershell.exe.
    let shell = ShellLaunch {
        kind: ShellKind::PowerShell,
        bash_bin: None,
        path_dirs: Vec::new(),
    };
    let mut command = build_shell_command_with_shell(
        "Write-Output ('host=' + $PSVersionTable.PSVersion)",
        Path::new("."),
        &shell,
    );
    let program = PathBuf::from(command.as_std().get_program());
    let output = command.as_std_mut().output().expect("spawn shell");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "shell failed: status={:?} stderr={stderr}",
        output.status
    );
    assert!(stdout.contains("host="), "unexpected stdout: {stdout:?}");
    assert!(stderr.trim().is_empty(), "stderr was not clean: {stderr}");

    assert_eq!(
        program.to_string_lossy().to_ascii_lowercase(),
        exe.to_string_lossy().to_ascii_lowercase(),
        "launcher diverged from the probed PowerShell host"
    );

    let major = powershell_major_version().expect("version must be detected");
    assert!(
        major >= 5,
        "expected a modern PowerShell, got major {major} from {exe:?}"
    );
    assert_eq!(
        powershell_display_name(),
        exe.file_name().unwrap().to_string_lossy().to_string(),
        "display name must name the executable that actually runs"
    );
    assert!(
        stdout.contains(&format!("{major}")),
        "stdout should report major {major}: {stdout:?}"
    );
}
