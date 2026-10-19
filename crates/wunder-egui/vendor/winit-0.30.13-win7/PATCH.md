# winit 0.30.13 Win7 IME patch

Some legacy Windows IMEs can emit a translated `WM_CHAR` after the same text
was delivered as `WM_IME_COMPOSITION/GCS_RESULTSTR`. The patched Windows event
loop records each IME commit and consumes only a matching subsequent character
sequence. A new physical key press clears stale state, so ordinary Unicode
keyboard layouts remain unchanged. This keeps the IME commit as the single
authoritative text event for both the egui and Slint frontends.
