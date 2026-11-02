# arboard 3.7.0 Win7 patch

Source: `arboard` 3.7.0, upstream commit `5f5c8637911ba219edee619105a19b4f74e66a9f`.

`src/platform/windows.rs` normally imports `PathCchStripPrefix` from
`api-ms-win-core-path-l1-1-0.dll`. That API starts at Windows 8, so a static
import prevents a Win7 program from starting. The patch keeps the surrounding
clipboard and `CF_HDROP` behavior, but implements the two verbatim DOS forms
returned by `GetFinalPathNameByHandleW` directly in UTF-16:

- `\\?\C:\...` becomes `C:\...`;
- `\\?\UNC\server\share\...` becomes `\\server\share\...`.

Remove this override when upstream supports a Win7-compatible implementation.

## Clipboard open retry window

Upstream opens the clipboard with 5 attempts spaced 5ms apart (~25ms total). A
clipboard monitor briefly holding the clipboard open defeats that window; the
failure surfaces as `Error::ClipboardOccupied` and callers (the Slint and egui
shells) treat it as "no clipboard content", so paste silently does nothing
while the system edit controls — which retry much longer — still succeed. The
patch widens the window to 25 attempts spaced 20ms (~0.5s), matching the
persistence class of Chromium/Firefox, and logs once when the clipboard stays
occupied after all attempts.
