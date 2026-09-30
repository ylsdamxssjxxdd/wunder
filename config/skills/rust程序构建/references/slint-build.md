# Slint build integration

Desktop projects use `frontend-slint/`. Read the project's Slint version from `Cargo.toml`/`Cargo.lock` before choosing APIs; the bundled guidance targets Slint 1.17+.

Before compiling Rust, run `slint-viewer --check <ui-file>`. For visual changes run `slint-viewer --screenshot <ui-file>` or inspect with the Slint MCP server. Prefer this fast preview over building the whole desktop app for screenshots; `--auto-reload` is useful during iteration.

Rust integration normally uses `slint-build::compile()` in `build.rs` and `slint::include_modules!()`. Keep UI work on the UI thread and move network/heavy parsing to background tasks. If a check fails, consult the Slint language/layout, gotchas, interop, and debugging references from the installed Slint skill.
