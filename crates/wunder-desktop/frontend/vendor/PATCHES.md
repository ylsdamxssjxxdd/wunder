# Local Win32 and text-stack patches

> Shared vendor tree: rcho and Wunder keep all nine Slint crate directories and
> `patches/slint-1.19.1/` byte-identical. Use this queue and
> `patches/slint-1.19.1/README.md` as the single source of truth; the app-side
> notes below are split by project because their UI entry points differ.

The `-1.19` source copies are pinned to Slint 1.19.1 (declared in
`frontend-slint/Cargo.toml` in rcho and
`crates/wunder-desktop/frontend/Cargo.toml` in Wunder) and are used only
through Cargo's `[patch.crates-io]` override. They are rebased from the previous
1.18 fork rather than carried over file-for-file: the upstream 1.19 tree is the
base, and the shared behaviors are grafted back.

The replayable form is a generated patch queue. `upstreams.json` pins the
pristine crates.io versions and SHA256 values, and
`patches/slint-1.19.1/series.tsv` lists one patch per changed file (62
patches across the nine crates at the time of writing). The vendored trees
remain the build source of truth; the queue is verified to
reproduce them byte-for-byte (ignoring Cargo's `.cargo-ok` and
`Cargo.toml.orig` metadata). The same vendor tree and queue are shared
byte-for-byte with Wunder; see `patches/slint-1.19.1/README.md`.

```powershell
python scripts/slint_vendor_patches.py verify
python scripts/slint_vendor_patches.py apply --upstream-root <new-src> --out target/slint-vendor-<ver>
python scripts/slint_vendor_patches.py capture --upstream-root <new-src> --vendor <resolved> --patch-dir frontend-slint/vendor/patches/slint-<ver>
```

## What is local in each crate

### `i-slint-common` 1.19.1

- `bitmapfont.rs` adds `data_packing: u8` to `BitmapGlyph`
  (0 = 8-bit alpha/SDF, 1 = 1-bit monochrome, 2 = RGB subpixel) while
  keeping 1.19's generic `Storage` model.
- `sharedfontique.rs` disables system-font discovery (`system_fonts: false`).
  The Win7 target must not pull DirectWrite/WinRT bindings in through font
  enumeration; the shell registers its bundled faces explicitly.

### `parley` 0.12 and `fontique` 0.12

Both are path-patched only to keep feature resolution aligned with the Slint
pins: `default = ["std"]`, system discovery off, `complex-scripts` and
`bytemuck` retained. fontique 0.12 already deduplicates family tables upstream,
so the old quadratic-scan workaround is gone.

### `i-slint-compiler` 1.19.1

- `passes/embed_glyphs.rs` is the 1.18 port rebased on the 1.19 pass: SimSun
  bitmap strikes are preferred (`Source::Bitmap(StrikeWith::ExactSize)`),
  outlines use `hint(true)`, printable ASCII uses `Format::Subpixel`, and
  fallback faces from the project are merged into the embedded font.
- `passes/gb2312_table.rs` and the `mod gb2312_table;` declaration supply the
  GB2312 + UI-literal coverage set used for non-strike sizes.
- `passes/embed_glyphs.rs` and `generator/rust.rs` emit the new
  `data_packing` field for embedded glyphs. `i-slint-common`'s
  `bitmapfont.rs` only declares the field; `passes/bitmapfont.rs` itself has
  no local diff in 1.19.1.
- `widgets/fluent/scrollview.slint` keeps the 1.19 `ScrollViewInterface` but
  drops the two arrow buttons and uses the 2 px offset / whole-track opacity
  treatment.
- `widgets/fluent/menu.slint` and `widgets/common/menu-base.slint` size the
  popup menu like rcho's own card instead of a full-size Fluent surface.
- `widgets/common/lineedit-base.slint` and `textedit-base.slint` localize the
  standard text-edit context menus to Chinese (撤销/重做/剪切/复制/粘贴/全选).
  These are the only two std widgets whose menu labels are visible without a
  translation catalog; project-owned `TextInput` uses `ui/text-field.slint`.
  The queue entries are `0006-common-lineedit-base.patch` and
  `0008-common-textedit-base.patch`.
- `builtin_elements.rs` declares the fork-only API the UI already uses:
  `line-height-factor` on `StyledText`, and the
  `context-menu(x, y, selected)` callback. Stock 1.19 only ships the former on
  `Text`, and has no right-click callback at all.

### `i-slint-core` 1.19.1

- `text_selection.rs` (new, window-level) owns cross-block selection state,
  hit-testing, selection geometry, copy/select-all, drag auto-scroll and
  right-click dispatch, instead of 1.19's per-item selection implementation.
- `items/text.rs` grafts `line-height-factor` and `context_menu` onto
  `StyledTextItem`, reports press/drag/release to the selection layer, paints
  the caret cursor, and reads `text_selection::covered()` in `render()` so a
  selection change invalidates the item.
- `items/text.rs` also restores the 1.18 Android IME guards lost in the rebase:
  no `InputMethodRequest::Update` while an IME composition is open, and
  `items::commit_focused_preedit()` so the shell can collect a pending
  composition before it replaces the draft programmatically.
- `textlayout/sharedparley.rs` passes the item's live selection into
  `draw_text` and paints the selection background with 1.19's
  `selection_geometry`; `shaping.rs` uses 1.19's `raw_text_ranges`.
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
  menus on Slint's in-window popup. rcho turns native menus off at startup:
  `TrackPopupMenu` is a modal loop on the UI thread, and while it runs no frame
  is produced and no timer fires.

### `i-slint-renderer-software` 1.19.1

- `fonts/pixelfont.rs` expands packing 1 (1 bpp, MSB first) to a single-channel
  `GlyphAlphaMap`, and packing 2 to `[r, g, b, 0]` per pixel marked
  `subpixel`. `RenderableGlyph` carries the flag, and the scene/shared-buffer
  path keeps the three channels instead of folding them to one alpha.
- `draw_functions.rs` blends the three coverages per target channel
  (`TargetPixel::blend_subpixel`); formats that cannot do so average them.
- The two `assert!(region.len() == 3)` checks in `api.rs` are relaxed to
  `>= 3` because core's `DirtyRegion::MAX_COUNT` is 16.
- `fonts/vectorfont.rs` restores the 1.18 glyph-quality path: an exact-size
  EBDT strike (`skrifa::bitmap::BitmapStrikes::with_format(.., BitmapFormat::Ebdt)`,
  `ppem == pixel_size`) is preferred over the outline, and the scaler turns on
  `hint(true)` for everything else. Without it, 100 % DPI text loses SimSun's
  hand-tuned 13/14 px bitmaps and Win7 falls back to unhinted outlines.
  Colour and zero-width strikes keep the upstream source list.
- `lib.rs` rejects text whose item rectangle no longer intersects the current
  clip before font matching and shaping, so a ScrollView full of off-screen
  rows does not lay out every hidden paragraph. When no bitmap font is
  registered (rcho's configuration), it also hands the text straight to
  `sharedparley::draw_text`, skipping the construction of a `VectorFont` that
  would only be inspected and dropped.
- Because no embedded bitmap font exists, `RenderableGlyph::subpixel` is always
  false and `draw_functions.rs` never takes the LCD-channel path; the 1.18
  `system_subpixel_order()` detection is therefore not needed in 1.19.
- `scene.rs`/`draw_functions.rs` carry the v0.2.1 image-sampling precision
  fix. `SceneTextureExtra` keeps `precise_dx`/`precise_dy`/`precise_off_x`/
  `precise_off_y` alongside the stock fixed-point fields, and
  `draw_texture_line` converts them to `Fixed<i32, 16>` for ordinary images.
  Signed-distance-field glyphs still take the 8-bit ratio / 4-bit offset
  path (the `fract()` bug that 1.18 needed is already fixed upstream in
  1.19). Without this hunk the 1/256 ratio truncation is multiplied by the
  large scroll offset, and the bitmap drifts several source pixels away from
  the vector overlays at fractional DPI. Regression:
  `image_sampling_stays_on_source_cells_after_large_scroll_clip` and
  `selection_and_peak_follow_raster_at_fractional_dpi_and_zoom` in
  `src/app/radar_ui_zoom_tests.rs` (both `#[ignore]`, run with `--ignored`).

The queue contains eight changed files in this crate and covers every
working-tree change; `verify` reproduces it byte-for-byte. `series.tsv` is
authoritative, and any change here must be re-captured before the next
version bump.
### `i-slint-backend-winit` 1.19.1

Stock 1.19 plus the Win32 redraw and diagnostics fixes:

- `first_frame.rs` reports the first successfully presented software frame;
  the software renderer has no `set_rendering_notifier`, so startup services
  wait on this notification plus a short fallback delay.
- `software_diagnostics.rs` records frame timings and windowing events behind
  `RCHO_SLINT_DIAGNOSTICS`; `renderer/sw.rs` calls `frame_start()` before the
  prepare phase and `record()` on every present attempt, then
  `first_frame::notify_presented()` after a successful present.
- External Explorer file drags are translated from winit's `HoveredFile` /
  `DroppedFile` / `HoveredFileCancelled` events into
  `WindowInner::process_drag_event`: the hover payload is remembered, cursor
  moves re-position its `DropEvent`, and the first `DroppedFile` consumes the
  complete path set. Without this, `DropArea` never receives OS drags on
  Windows.
- Win32 event handling forces a complete frame when activation repaints the
  non-client frame, after `TrackPopupMenu` returns, on DPI changes, and when
  the window is moved. Resize requests redraw directly instead of through the
  frame throttle, and minimized windows skip the forced DIB render.
  Focus gain presents the frame in the same message turn (so the custom border
  never flashes transparent), skipped when another frame was presented less
  than 250 ms ago; focus loss arms a full repaint and queues one.
- `window_attributes()` keeps Win32 top-levels opaque with
  `with_transparent(!cfg!(target_os = "windows"))`. Slint 1.19 changed the
  default to a transparent window; the native GDI 3D child writes RGB with
  `StretchDIBits`, so DWM treats its rectangle as a hole and the desktop shows
  through the training world and the workbench 3D viewport.
- `frame_throttle.rs` keeps an absolute deadline grid instead of a
  "schedule after each redraw" model: one `single_shot` timer per period,
  `frame_period` computed in nanoseconds (`1e12 / mHz`, so 60 Hz is 16.667 ms
  rather than 16 ms), and monitor refresh rates outside 20–500 Hz ignored
  (winit's 0/1 sentinels fall back to 60 Hz). A redraw that arrives while
  hidden or minimized cancels the pending request and clears the grid.
- `drag_resize_window.rs` returns `false` without starting a gesture when the
  window is maximized/fullscreen, decorated by the system, or not resizable;
  the adapter then lets the press through to normal item dispatch. Without
  this, pressing an edge on a maximized Win7 window could start a bogus
  restore-and-resize.
- `renderer/sw.rs` treats `surface.resize`/`buffer_mut`/`present` failures as
  transient: log, retry the deferred resize at most twice, keep the frame
  request armed and return success so the loop survives a lost DIB. A surface
  size change (or `occluded`, `suspend`) arms a full repaint, because the
  cached damage describes the previous geometry; only a successful present
  notifies `first_frame`.
- `renderer/sw/popup_repaint.rs` (new) copies 1.18's popup policy: compare
  the active `ChildWindow` rectangles each frame, and when only the popup
  moved, turn its screen rectangle into dirty regions inflated by
  `1/scale_factor` instead of repainting the whole window.
  `RCHO_SLINT_POPUP_FULL_REPAINT=1` forces the full path for diagnostics.
- `lib.rs` additionally exports `software_diagnostics`/`first_frame` when the
  software renderer is enabled (1.18 module wiring).

The queue contains eight changed files in this crate and covers every
working-tree change, including `drag_resize_window.rs`, `frame_throttle.rs`,
and the new `renderer/sw/popup_repaint.rs`; `verify` reproduces the tree
byte-for-byte. Before the next capture, inspect these diffs and drop anything
that is no longer required rather than carrying it forward blindly.

### `winit` 0.30.2

- `platform_impl/windows/event_loop.rs` tracks the right-button gesture.
  `WM_RBUTTONDOWN` sets `right_button_pressed`; `WM_EXITMENULOOP` synthesizes
  the `Released` event when `TrackPopupMenu` consumed the real release, and
  arms a full repaint after the modal menu loop unwinds. A second
  `WM_EXITMENULOOP` is inert, and a real `WM_RBUTTONUP` is never duplicated.
- `platform_impl/windows/keyboard.rs` turns a standalone `WM_CHAR` with no
  keydown into a synthetic `KeyboardInput` event (including UTF-16 surrogate
  pairs), so IMEs and accessibility tools that send text directly still reach
  the focused editor.
- `event_loop.rs`/`keyboard.rs` keep the IME commit-echo deduplicator:
  `begin_keydown` expires an unmatched echo, only the exact committed text is
  suppressed, and full-width punctuation delivered as direct `WM_CHAR` is
  preserved.
- `platform/windows.rs` exposes `with_no_activate`, and
  `platform_impl/windows/window.rs` stores the right-button state used by the
  event loop.
- Linux X11 XIM callbacks use the `c_char`-portable signature for aarch64;
  this is required for the ARM64 Linux build.

### `softbuffer` 0.4.8

- `src/backends/win32.rs` turns `CreateCompatibleDC` / `CreateDIBSection` /
  `BitBlt` failures into `SoftBufferError` instead of `assert!`/ignored return.
  In particular, a failed `BitBlt` must not validate `WM_PAINT`, otherwise a
  hidden and re-shown Win7 window never retries its white client area. The
  older `patches/softbuffer-0.4.8.patch` archive is historical only.

## App-side integration notes (rcho)

These live outside `vendor/` but break silently on a Slint upgrade:

- `ui/business-record-page.slint` must keep its row content as a sibling of
  `ContextMenuArea`, not a child. In 1.19 `ContextMenuArea` is a non-visual
  `Empty` that only carries the `Menu`; putting a row `Rectangle` inside it
  makes `parent.width` zero and the row disappears.
- The vendored `SpinBoxBase`/`ComboBoxBase` import the standard-widget
  interfaces and use `@shadowable`, so `SLINT_ENABLE_EXPERIMENTAL_FEATURES`
  must be `interfaces,shadowable`. This is set both in `build.rs` and in
  `.cargo/config.toml` so `slint!` examples pick it up too.
- The `patches/softbuffer-0.4.8.patch` and `patches/winit-0.30.2.patch`
  archives are kept as historical records. They are not part of the replay
  pipeline and must not be applied on top of the per-file queue; use
  `patches/slint-1.19.1/` for upgrades.
- `src/main.rs::run_workbench` polls `native::window::maximize_slint_window`
  after `show()`; on Win32 it calls `ShowWindow(SW_MAXIMIZE)` once the HWND is
  visible, because Winit applies the `with_maximized` attribute asynchronously
  and Slint's first preferred-size request can restore the window.
  The backend's `!winit_window.is_maximized()` guard in `set_window_size`
  (part of `0008-winitwindowadapter.patch`) is required together with that
  poll; keep both when porting the queue.
- `ui/text-field.slint` is the single home for the project-owned text-input
  context menu. It must set an explicit `width`/`height` because
  `ContextMenuArea` is a non-visual `Empty`, and `@children` must precede
  `Menu` or the menu-lowering pass panics. Wrap any new bare `TextInput` with
  `TextFieldMenu`; std `LineEdit`/`TextEdit` already carry their own
  `ContextMenuArea`, and only need the compiler label patch.
- `src/app/touch_keyboard.rs` installs one `on_winit_window_event` filter per
  top-level window and calls the shell's `ITipInvocation::Toggle` only when the
  activation came from `WindowEvent::Touch` and Slint subsequently focused a
  text input. New top-level windows with editable fields must call
  `super::touch_keyboard::install(window.as_weak())` before `show()`. This
  depends on the vendored winit `RegisterTouchWindow` block
  (`SM_DIGITIZER & NID_READY`); the IMM `set_ime_allowed` hook alone does not
  raise TabTip on Win32 desktop windows.

## App-side integration notes (Wunder)

- `crates/wunder-desktop/frontend/ui/composer.slint` is a layout child; its
  dynamic card height is exposed as `card-height` and applied as
  `min-height: self.card-height` at the two usage sites (`ui/main.slint` and
  `crates/wunder-android/ui/mobile.slint`). A plain `height:` binding is
  ignored inside a layout.
- Wunder's `build.rs` must list experimental features explicitly. 1.19
  rejects the old catch-all value `1`; the workbench needs `closures` for the
  animated `index-of` path.
- Wunder's `ui/text_field.slint` / `ui/components.slint` own the in-window
  `TextFieldMenu`; the same rules apply as rcho's `ui/text-field.slint`:
  explicit width/height on the non-visual `ContextMenuArea`, and `@children`
  before `Menu`.
- `crates/wunder-desktop/frontend/src/touch_keyboard.rs` installs the touch
  filter per top-level window; its `TextFieldActivation` global reports the
  exact field focus. New top-level windows with editable fields must install
  the filter before `show()`.
- Update `[patch.crates-io]` paths and `slint-build` in both
  `crates/wunder-desktop/frontend/Cargo.toml` and
  `crates/wunder-android/Cargo.toml`; the latter also shares the vendored
  Slint tree.

## Upgrading Slint again

1. Unpack the new crates.io sources for the five Slint crates plus
   fontique/parley, softbuffer, and winit. Keep the pristine tree separate from
   the working vendor tree.
2. Confirm the current queue still reproduces the current vendor tree. In rcho
   run `python scripts/slint_vendor_patches.py verify`; in Wunder run the same
   script from `crates/wunder-desktop/frontend`. If it fails, fix the queue
   before changing versions; do not use a failing baseline for a rebase.
3. Replay the queue onto the new sources with
   `python scripts/slint_vendor_patches.py apply --upstream-root <new-src> --out target/slint-vendor-<ver>`.
   `git apply --reject` leaves `.rej` files for real conflicts; inspect every
   one. Do not copy whole files from the old tree, and do not use
   `patch --merge=diff3` (it anchors unrelated hunks into the wrong impl).
4. Resolve conflicts in the staging tree. Then freeze the result as the next
   queue with
   `python scripts/slint_vendor_patches.py capture --upstream-root <new-src> --vendor <staging> --patch-dir <vendor>/patches/slint-<ver>`.
   Copy the resulting queue and vendor crates to the other project, then run
   `verify` in both before committing.
5. Update `upstreams.json` with the new versions, vendor directory names, and
   SHA256 values; update `[patch.crates-io]` paths and `slint-build` in
   `frontend-slint/Cargo.toml` (rcho), both Wunder Cargo manifests, and
   `crates/wunder-android/Cargo.toml`; regenerate `Cargo.lock`; refresh the
   offline `cargo-vendor-slint` snapshot; then build with
   `build.bat -t windows -a win7` in rcho and `build.bat -t desktop -a win7-x86`
   in Wunder.
6. Run the fractional-DPI image-sampling regressions. In rcho they are
   `image_sampling_stays_on_source_cells_after_large_scroll_clip` and
   `selection_and_peak_follow_raster_at_fractional_dpi_and_zoom` in
   `src/app/radar_ui_zoom_tests.rs` (both `#[ignore]`; run with `--ignored`).
   The `scene.rs`/`draw_functions.rs` hunk is the one that silently regresses:
   it looks correct in a static 1:1 screenshot and only shows up after
   fractional-DPI zoom plus a large scroll offset.
7. The Win7 PE import gate must stay green: no `combase.dll`, no
   `api-ms-win-*`/`ext-ms-win-*`, no `SystemParametersInfoForDpi`,
   `CreateWaitableTimerEx` or `GetDpiForWindow` strong imports.

`RCHO_SLINT_DIAGNOSTICS=1` enables the frame/window event JSONL stream.
