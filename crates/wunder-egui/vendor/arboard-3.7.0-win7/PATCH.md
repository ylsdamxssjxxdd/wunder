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
