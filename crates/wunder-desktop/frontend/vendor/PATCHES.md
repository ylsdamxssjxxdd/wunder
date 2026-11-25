# Local Win32 and text-stack patches

The `-1.19` source copies are pinned to Slint 1.19.1 (declared in
`crates/wunder-desktop/frontend/Cargo.toml`) and are used only through Cargo's
`[patch.crates-io]` override. They were rebased from the 1.18 fork rather than
carried over file-for-file: the upstream 1.19 tree is the base, and only the
behaviors the workbench actually depends on were grafted back.

## What is local in each crate

### `i-slint-common` 1.19.1

- `bitmapfont.rs` adds `data_packing: u8` to `BitmapGlyph` (0 = 8-bit
  alpha/SDF, 1 = 1-bit monochrome, 2 = RGB subpixel) while keeping 1.19's
  generic `Storage` model.
- `sharedfontique.rs` disables system-font discovery (`system_fonts: false`).
  The Win7 target must not pull DirectWrite/WinRT bindings in through font
  enumeration; the shell registers its bundled faces explicitly.

### `parley` 0.12 and `fontique` 0.12

Both are path-patched only to keep feature resolution aligned with the Slint
pins: `default = ["std"]`, `system` discovery off, `complex-scripts` and
`bytemuck` retained. fontique 0.12 already deduplicates family tables upstream,
so the 1.11 quadratic-scan workaround is gone.

### `i-slint-compiler` 1.19.1

- `passes/embed_glyphs.rs` is the 1.18 port rebased on the 1.19 pass: SimSun
  bitmap strikes are preferred (`Source::Bitmap(StrikeWith::ExactSize)`),
  outlines use `hint(true)`, printable ASCII uses `Format::Subpixel`, and
  fallback faces from the project are merged into the embedded font.
- `passes/gb2312_table.rs` and the `mod gb2312_table;` declaration supply the
  GB2312 + UI-literal coverage set used for non-strike sizes.
- `passes/bitmapfont.rs` and `generator/rust.rs` emit the new `data_packing`
  field for embedded glyphs.
- `widgets/fluent/scrollview.slint` keeps the 1.19 `ScrollViewInterface` but
  drops the two arrow buttons and uses the 2 px offset / whole-track opacity
  treatment.
- `widgets/fluent/menu.slint` and `widgets/common/menu-base.slint` size the
  popup menu like the workbench's own card (`ui/text_menu.slint`) instead of a
  full-size Fluent surface: 148 px minimum width, 28 px rows, 7 px separators,
  13 px label on a white `#FFFFFF` card with a `#D7DBE0` hairline, 10 px radius
  and a 12 px/3 px shadow (colours mirror `ui/design_tokens.slint`). The
  shortcut text now follows the item's `font-size`/`font-weight` so shrinking
  the label does not leave a full-size accelerator at the right edge.
- `builtin_elements.rs` declares the fork-only API the UI already uses:
  `line-height-factor` on `StyledText`, and the
  `context-menu(x, y, selected)` callback. Stock 1.19 only ships the former on
  `Text`, and has no right-click callback at all.

### `i-slint-core` 1.19.1

- `text_selection.rs` (new, window-level) owns cross-block selection state,
  hit-testing, selection geometry, copy/select-all, drag auto-scroll and
  right-click dispatch. This is deliberately not 1.19's per-item
  `textlayout/sharedparley/selection.rs`: the markdown surface selects across
  many `StyledText` items, which the item-local implementation cannot express.
- `items/text.rs` grafts `line-height-factor` and `context_menu` onto
  `StyledTextItem`, reports press/drag/release to the selection layer, paints
  the caret cursor, and reads `text_selection::covered()` in `render()` so a
  selection change invalidates the item. The stock 1.19 item-local selection
  remnants (`SelectionUnit`/`MouseSelection`, `selection_mouse_event`,
  `selectable_text_cursor`, `selection_by_unit` and the `mouse_selection`
  field) are dropped: the window-level layer owns click counting and unit
  expansion, so nothing called them.
- `items/text.rs` also restores the 1.18 Android IME guards lost in the rebase:
  no `InputMethodRequest::Update` while an IME composition is open (both the
  per-frame `render()` push and `update_ime()`), and
  `items::commit_focused_preedit()` so the Android shell can collect a pending
  composition before it replaces the draft programmatically (voice
  transcription).
- `textlayout/sharedparley.rs` passes the item's live selection into
  `draw_text` and paints the selection background with 1.19's
  `selection_geometry`; `shaping.rs` uses 1.19's `raw_text_ranges` instead of
  the fork's paragraph-range workaround.
- `cursor.rs`, `window.rs`, `item_rendering.rs` and `partial_renderer.rs` keep
  the window-level drag routing, cursor bookkeeping and physical-pixel damage
  margin needed by the selection layer.
- `items/flickable.rs` keeps the 1.18 touch behaviour: a touch press is
  forwarded immediately instead of waiting out `FORWARD_DELAY`, the gesture is
  claimed by its dominant axis, and a selection drag keeps ownership of the
  pointer instead of panning.
- `model/repeater.rs` keeps exact `anchor_y` prefix sums from measured row
  heights, including insert/remove shifts and the content-height estimate for
  unmeasured rows below the window.
- `DirtyRegion::MAX_COUNT` stays raised to 16.
- `Window::set_native_popup_menus_enabled()` exposes the `WindowInner` switch
  that `show_native_popup_menu()` already reads, so a window can keep context
  menus on Slint's in-window popup. The workbench turns native menus off at
  startup: `TrackPopupMenu` is a modal loop on the UI thread, and while it runs
  no frame is produced and no timer fires, so a streaming transcript freezes
  behind the menu until it is dismissed.

### `i-slint-renderer-software` 1.19.1

- `fonts/pixelfont.rs` expands packing 1 (1 bpp, MSB first) to a single-channel
  `GlyphAlphaMap`, and packing 2 to `[r, g, b, 0]` per pixel marked
  `subpixel`. `RenderableGlyph` carries the flag, and the scene/shared-buffer
  path keeps the three channels instead of folding them to one alpha.
- `draw_functions.rs` blends the three coverages per target channel
  (`TargetPixel::blend_subpixel`); formats that cannot do so average them.
- The two `assert!(region.len() == 3)` checks in `api.rs` are relaxed to
  `>= 3` because core's `DirtyRegion::MAX_COUNT` is 16. Everything else is
  stock 1.19.
- `scene.rs`/`draw_functions.rs` carry the image-sampling precision fix
  inherited from the rcho fork. `SceneTextureExtra` keeps
  `precise_dx`/`precise_dy`/`precise_off_x`/`precise_off_y` alongside the
  stock fixed-point fields, and `draw_texture_line` converts them to
  `Fixed<i32, 16>` for ordinary images. Signed-distance-field glyphs still
  take the 8-bit ratio / 4-bit offset path (the `fract()` bug that 1.18
  needed is already fixed upstream in 1.19). Without this hunk the 1/256
  ratio truncation is multiplied by a large scroll offset and the bitmap
  drifts several source pixels away from vector overlays at fractional DPI.
  Verification: zoom a large `Image` (e.g. 1.75× DPI scale plus thousands of
  pixels of scroll offset) and compare the drawn bitmap against the expected
  source-cell centroid; the drift is tens of physical pixels before the
  patch and under 2 px after it.

### `i-slint-backend-winit` 1.19.1

Stock 1.19 plus the workbench modules and Win32 redraw fixes:

- `first_frame.rs` reports the first successfully presented software frame;
  the software renderer has no `set_rendering_notifier`, so startup services
  wait on this notification plus 10 ms.
- `software_diagnostics.rs` records frame timings and windowing events behind
  `RCHO_SLINT_DIAGNOSTICS`; `renderer/sw.rs` calls `frame_start()` before the
  prepare phase and `record()` on every present attempt with the measured
  prepare/render/present times, damage pixels, popup flag and failure flag, then
  `first_frame::notify_presented()` after a successful `present_with_damage`.
- External Explorer file drags are translated from winit's `HoveredFile` /
  `DroppedFile` / `HoveredFileCancelled` events into
  `WindowInner::process_drag_event`: the hover payload is remembered, cursor
  moves re-position its `DropEvent`, and the first `DroppedFile` consumes the
  complete path set. Without this, `DropArea` never receives OS drags on
  Windows.
- Win32 event handling forces a complete frame when activation repaints the
  non-client frame, after `TrackPopupMenu` returns, on DPI changes, and when
  the window is moved (GDI can discard surface bands). Resize requests redraw
  directly instead of through the frame throttle, and minimized windows skip
  the forced DIB render.

## Behavior intentionally dropped in the 1.19 rebase

These 1.18 fork changes were not carried over; re-evaluate before restoring
them one by one:

- The Win32 GDI ClearType ASCII rasterizer in the software renderer. The
  embedded ASCII sub-pixel payload now keeps its RGB channels, but the runtime
  installed-font GDI fallback is still absent; the app never registered that
  face, so this was already dead code in 1.18.
- The `frame_capture` module. It had no caller: the workbench screenshot path
  captures the desktop with GDI and keeps its own frozen frame, so an
  event-loop surface copy would duplicate that work.
- The Win7 IME deferral and standalone `WM_CHAR` fixes in the winit fork are
  still present in `vendor/winit-0.30.2`, but the 1.19 backend has not yet been
  regression-tested with them on a real Win7 machine.

The `patches/softbuffer-0.4.8.patch` and `patches/winit-0.30.2.patch`
archives remain valid; `softbuffer` and `winit` versions are unchanged by the
1.19 upgrade.

## App-side integration notes

These live outside `vendor/` but break silently on a Slint upgrade:

- `ui/textedit-base.slint` must keep `ScrollView` and `TextInput` as siblings of
  `ContextMenuArea`, not children. In 1.19 `ContextMenuArea` is a non-visual
  `Empty` that only carries the `Menu`; putting the scroll view inside it makes
  `parent.width` zero and the editor unhittable.
- `ui/composer.slint` is a layout child. Its dynamic card height is exposed as
  `card-height` and applied as `min-height: self.card-height` at the two usage
  sites (`ui/main.slint`, `crates/wunder-android/ui/mobile.slint`); a plain
  `height:` binding is ignored inside a layout.
- `build.rs` must list experimental features explicitly. 1.19 rejects the old
  catch-all value `1`; the workbench needs `closures` for the animated
  `index-of` path.

## Upgrading Slint again

1. Unpack the new crates.io sources for the five crates plus fontique/parley.
2. Diff the current `-1.19` tree against that pristine source and replay only
   the hunks listed above; do not copy whole files, and do not use
   `patch --merge=diff3` (it anchors unrelated hunks into the wrong impl).
   The `scene.rs`/`draw_functions.rs` sampling hunk is the one that silently
   regresses: it looks correct at 1:1 and only shows up after fractional-DPI
   zoom plus a large scroll offset, so a clean 1:1 screenshot does not prove
   it survived the replay.
3. `git apply` silently skips files in this repository and still exits 0; use
   GNU `patch` and inspect every `.rej`.
4. Update `[patch.crates-io]` paths and `slint-build` in both
   `crates/wunder-desktop/frontend/Cargo.toml` and
   `crates/wunder-android/Cargo.toml`, regenerate `Cargo.lock`, refresh the
   offline `cargo-vendor-slint` snapshot, then build with
   `build.bat -t desktop -a win7-x86`.
5. The Win7 PE import gate must stay green: no `combase.dll`, no
   `api-ms-win-*`/`ext-ms-win-*`, no `SystemParametersInfoForDpi`,
   `CreateWaitableTimerEx` or `GetDpiForWindow` strong imports.

`RCHO_SLINT_DIAGNOSTICS=1` enables the frame/window event JSONL stream.
