# Shared Slint 1.19.1 patch queue

This queue is the canonical per-file patch set for the vendored Slint 1.19.1
tree. It is shared byte-for-byte by rcho and Wunder: the nine vendored crate
directories and this queue are identical in both projects.

- 62 patches across nine crates (`i-slint-common`, `i-slint-compiler`,
  `i-slint-core`, `i-slint-renderer-software`, `i-slint-backend-winit`,
  `fontique`, `parley`, `softbuffer`, `winit`).
- Generated against pristine crates.io 1.19.1 / 0.12.0 / 0.4.8 / 0.30.2
  sources. `series.tsv` is the authoritative order and file list.
- The rcho copy can be checked from the repository root with
  `python scripts/slint_vendor_patches.py verify`; the Wunder copy uses the
  same script under `crates/wunder-desktop/frontend/scripts/`.

Do not edit the vendored crates by hand without regenerating this queue. The
workflow is: edit the vendor file, then run the project's
`slint_vendor_patches.py export` against a pristine upstream tree, and keep the
resulting `series.tsv` and per-file patches together.

Application code is intentionally outside this queue. Touch-keyboard startup,
the project `TextFieldMenu`, window maximize polling and other app-side
integrations live in the application source trees and are documented in each
project's own notes.
