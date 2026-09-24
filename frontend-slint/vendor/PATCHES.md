# Local Win32 and text-stack patches

The `-1.18` source copies are pinned to Slint 1.18.0 (declared in
`frontend-slint/Cargo.toml`) and are used only through Cargo's
`[patch.crates-io]` override.

## Slint 1.18.0 port notes

Ported from the 1.17.1 patch set with these upstream changes:

- The text stack moved from fontique/parley 0.10 to 0.11 (`unstable-fontique-011`
  replaces `unstable-fontique-010`; `slint::fontique_011` replaces
  `slint::fontique_010` in `src/system_fonts.rs` and
  `src/app/help_window/layout.rs`). fontique 0.11.1 was audited for the
  quadratic `load_fonts_from_paths` family-table merge: **not fixed upstream**
  (the batch map is still shared across the scan and re-merged per font), so
  `vendor/fontique-0.11` carries the same per-font batch-map fix. Both
  fontique 0.11 and parley 0.11 keep the `default = ["std"]` change so the
  `system` discovery feature (DirectWrite/WinRT on Windows) stays off; parley
  0.11 also keeps the `#[allow(deprecated)]` on the bidi `mask` helper.
- `i-slint-backend-winit` 1.18 moved the window-event match from
  `event_loop.rs` into `winitwindowadapter.rs::handle_window_event`. The
  Win7 event patches were re-applied there: resize-direction reset on
  maximize/fullscreen transitions, `WindowEvent::Moved` forcing a full
  software frame, the focus-gain forced complete frame, and `handle_resize`
  gating the left-press consumption. The maximized-state guard on
  `request_inner_size` and the hide-window `occluded(true)` are in the same
  file.
- `renderer/sw.rs`: 1.18 upstream now presents every disjoint damage
  rectangle through `present_with_damage` (formerly our per-rect present
  patch). The local remainder is the deferred-error handling (transient GDI
  failures set `force_full_repaint` and retry instead of terminating the
  event loop) plus the frame diagnostics recording.
- `i-slint-core` 1.18 ships `RetainedLineBreaking` in
  `textlayout/sharedparley/`: line breaking and its derived metrics are
  cached in the paragraph cache and reused while width/alignment/overflow are
  unchanged. This supersedes the local `sharedparley/line_cache.rs`
  scroll-text patch, which was dropped (both scroll-text patch archives from
  1.17 have no 1.18 equivalent). The `DirtyRegion::MAX_COUNT` 3→16 raise and
  the partial-render physical-pixel margin are unchanged and still local;
  the margin hunk needed `scale_factor().get()` because 1.18 returns a typed
  `euclid::Scale`.
- `i-slint-renderer-software` 1.18 added subpixel-bin offsets to
  `render_vector_glyph` and a struct `GlyphCacheKey`; the exact-size bitmap
  strike preference (never stretch a nearby strike, skip zero-width SimSun
  whitespace bitmaps) and `hint(true)` were re-applied on top of that shape.
  `sharedparley::draw_text_input` now takes the text layout cache instead of
  the password-character callback; both call sites pass
  `self.text_layout_cache`.
- The compiler's `software-renderer` feature was renamed to
  `renderer-software`; the `gb2312_table` module gate follows the new name.
  Upstream rewrote `try_extract_literal_from_element` with the newer binding
  API, so the local copy of that helper was dropped; the non-positive
  font-size filter around it is unchanged.
- `const-field-offset` must be 0.2.1: the 1.18 generator emits
  `sp::compose_field_offsets`, which only exists in const-field-offset 0.2.1.
  A lockfile that still pins 0.2.0 fails every `slint::slint!` expansion —
  run `cargo update -p const-field-offset` after upgrading.
- 1.18 deprecates the `viewport-*` Flickable/ListView properties in favour of
  `content-*`. The workbench UI has been renamed to `content-*` throughout.

The remaining 1.17 patch intents (gb2312 tiered embedding, bitmap-strike
embedding preference, ASCII subpixel packing, 1bpp packed glyphs + LRU,
Win32 GDI ASCII path, softbuffer GDI failure handling, winit 0.30.2 Win32
caption/erase/minmax overlays) applied to 1.18 with at most line offsets and
are unchanged in behavior.

## Current runtime system-font rendering

The workbench loads installed font files through Fontique and uses `VectorFont`.
Its Swash scaler enables outline hinting and prefers an exact-size monochrome
bitmap strike before falling back to the outline. This preserves small SimSun
CJK pixels without stretching a nearby strike or embedding font bytes. The
bitmap dimensions are checked first: SimSun whitespace can have zero-width
strikes, which panic in Swash 0.2's row decoder. Empty bitmaps use the outline
fallback while text layout retains the whitespace advance. The
existing per-font/size/glyph/variation cache and glyph baseline conversion are
unchanged. Rendering remains grayscale; the historical PixelFont/GDI ClearType
patches below do not apply to this runtime path.

`fonts/vectorfont/tests.rs` checks installed Windows fonts for exact bitmap
coverage, baseline, cache reuse, hinted fallback and empty whitespace strikes.
`bundled_subset_matches_master_rendering` additionally renders the bundled
`res/fonts/simsun-subset.ttf` (produced by `scripts/subset_simsun.py`) against
the full `simsun.ttf` master glyph-for-glyph at both bitmap-strike and outline
sizes, guarding the custom EBDT/EBLC rebuild against silent corruption. (The
sample intentionally excludes U+2212/U+2194/U+25BE: the master SimSun cmap
itself lacks them, so they fall back to `.notdef` in both fonts.) The
`font_preview` example
renders this project's actual software renderer without starting app services;
an independently installed slint-viewer does not include these Rust patches.

## Embedded-font and platform patches

The behaviors below were written for the original 1.17.1 patch series and now
live in the `-1.18` directories (see the 1.18.0 port notes above for the
deltas).

- `i-slint-compiler` ignores non-positive literal font sizes while
  collecting software-renderer bitmap sizes. Slint's widget interfaces use
  `font-size: 0px` as an inheritance sentinel; rasterizing it as a real size
  embeds huge unscaled glyph bitmaps. It also deduplicates the same default
  font face across exported top-level components; every component still
  registers the one shared bitmap resource with its renderer.
- The compiler embeds the first registered project font as one bitmap font
  resource. rcho uses its bundled SimSun face for digits, English, and CJK;
  runtime `TextInput` values therefore stay visible without probing system
  fonts or duplicating a fallback atlas.
- Pre-rendered glyph embedding prefers the font's embedded monochrome bitmaps
  (`Source::Bitmap(StrikeWith::ExactSize)` before `Source::Outline`) and turns
  on swash hinting for the outline fallback. SimSun ships hand-tuned EBDT
  strikes at 12–18 ppem — the same pixel glyphs GDI/DirectWrite render — so
  the desktop build keeps SDF off; bitmap strikes cover the common UI sizes
  and hinted outlines cover the rest.
- Printable ASCII (English, digits, punctuation, and space) is the deliberate
  exception: SimSun does not supply it through those small EBDT strikes, so
  the compiler renders its hinted outline as `swash::zeno::Format::Subpixel`
  and stores the RGB coverages as `BitmapGlyph::data_packing == 2` (four bytes
  per pixel, RGB plus a reserved byte). The software renderer identifies that
  text-only payload and blends its channels independently; ordinary images
  cannot enter this path. This supplies the ClearType-equivalent step absent
  from upstream Slint's software renderer, rather than attempting to disguise
  the problem by swapping Western fonts.
- On Win7, the renderer registers only the bundled SimSun face with
  `AddFontMemResourceEx`, then uses a screen-compatible memory DC,
  `CreateDIBSection`, and `ExtTextOutW` with `CLEARTYPE_NATURAL_QUALITY` to
  obtain native RGB coverages for ASCII. The DIB is immediately copied into
  the renderer-owned cache; no system font enumeration or runtime font file
  lookup is performed. If registration, GDI rasterization, or ClearType
  detection fails, the pre-embedded Slint glyph remains the fallback.
- Embedded glyph payloads are size-tiered and bit-packed to bound the binary:
  strike sizes (12/13/14/15/16/18 px) keep the full cmap, other sizes embed
  GB2312 + UI literals + ASCII only (`passes/gb2312_table.rs`, generated by
  `scripts/gen_gb2312_table.py`; out-of-coverage glyphs keep their advance but
  stay blank instead of becoming tofu). Purely monochrome glyph bitmaps are
  stored as 1-bit rows (`BitmapGlyph::data_packing == 1`, MSB first). The
  software renderer expands only the individual packed glyph that is painted
  (`glyph_alpha_map` in `fonts.rs`), with a per-UI-thread LRU cache capped at
  1,024 glyphs or 1 MiB. Font registration stays a plain push, so opening a
  window (a fresh process) never pays a whole-font expansion. The C++ generator does
  not know about `data_packing`; do not use it for packed fonts.
- Runtime bitmap-size matching picks the closest embedded size and prefers
  the larger one on near ties: downscaling a bigger bitmap stays crisp while
  upstream's always-snap-down upscales smaller strikes and visibly softens
  text at 150-200% display scaling. `SLINT_FONT_SIZES=18,21,24` in build.rs
  embeds the sizes those scale factors actually request.
- The (currently unused) SDF path generates its single scalable atlas at a
  minimum of 24 px. Slint's upstream 16 px floor visibly thins and breaks
  dense SimSun strokes when compact 10–13 px Chinese UI labels are sampled by
  the software renderer. The renderer's upstream contour threshold remains
  unchanged to avoid making adjacent strokes stick together.
- `softbuffer` 0.4.8 keeps its MIT and Apache-2.0 license files. Its Win32 GDI
  resize path returns an error for `CreateCompatibleDC` / `CreateDIBSection`
  failures and preserves the last valid bitmap; it no longer asserts. The
  present path also propagates `BitBlt` failures and leaves WM_PAINT invalid,
  so a transient failed copy is retried instead of validating a white frame.
- `i-slint-backend-winit` 1.18.0 retains its upstream licensing information. Its
  software renderer treats those transient surface errors as a deferred frame
  rather than returning an event-loop terminating error. Retry requests are
  capped at two until the next successful frame to avoid a permanent redraw
  loop under sustained memory pressure. Embedded popup stack/geometry changes
  retain a complete software frame. Steady popups explicitly dirty their whole
  rectangle plus a physical-pixel rounding margin, so partial Win7/GDI repaints
  cannot leak underlying toolbar icons through the popup while hover avoids
  repainting the full workbench. `renderer/sw/popup_repaint.rs` owns this policy;
  `RCHO_SLINT_POPUP_FULL_REPAINT=1` retains the old full-frame mode for same-exe
  A/B diagnostics. A hidden/re-shown or
  moved Win32 window marks its next software frame as a new buffer and queues
  a redraw: Win7 GDI may discard pixels without updating softbuffer's buffer
  age, otherwise reused auxiliary windows can reopen white or retain clipped
  regions after being dragged back onto a display.
- Its frame throttle rejects implausible monitor refresh rates before picking
  the redraw-timer interval. `EnumDisplaySettingsExW` documents
  `dmDisplayFrequency` 0 or 1 as the "hardware default" sentinel, which Win7
  basic/mirror/remote display drivers actually return; winit forwards it raw,
  so an unfiltered `1_000_000 / 1000` made every throttled redraw wait one
  full second (and 0 would divide by zero). Rates outside 20–500 Hz fall back
  to 60 Hz. Minimized Win32 windows skip software rendering entirely instead
  of painting a forced full frame into an invisible DIB; the occluded flag
  stays armed so the restore frame is still complete. Focus gain no longer
  presents its forced full frame when another frame was presented within the
  last 250 ms — restoring from minimize previously painted two full frames
  back-to-back (the queued WM_PAINT frame plus the focus-turn frame). The
  bookkeeping `Moved` event to the off-screen minimize position no longer
  forces a full repaint while the window is iconic.
- `software_diagnostics.rs` also records timestamped windowing events when
  `RCHO_SLINT_DIAGNOSTICS` is set: `resized` (with the Slint layout-dispatch
  duration in `ms`), `moved`, `focused` (b = skipped-sync-draw),
  `occluded`, `state` (a = minimized, b = maximized) and
  `draw_skip_minimized`. The rcho caption handlers add `showwindow` events
  measuring how long the synchronous Win32 `ShowWindow(SW_MINIMIZE/
  SW_MAXIMIZE/SW_RESTORE)` call blocked the event loop — on classic-theme Win7
  the system zoom-rectangle animation runs inside that call, so a large `ms`
  there isolates system animation from render-path cost. Events drain through
  `take_events()` and land as `{"kind":"event", ...}` JSONL lines.
- The Win32 software present path submits each disjoint damage rectangle to
  softbuffer instead of one bounding-box BitBlt. Navigation frames dirty the
  radar canvas, file list and panels at once; the bounding box spanned the
  whole window and copied megabytes of unchanged pixels every frame.
- `first_frame.rs` reports the first successfully presented software frame;
  `Window::set_rendering_notifier` is unsupported by the software renderer, so
  the workbench cannot learn it through the public API. Startup services
  (recent-directory restore, decode and map warm-up) wait for this
  notification plus 10 ms instead of racing the first frame.
- `frame_capture.rs` implements one-shot frame capture for the workbench
  screenshot feature: a request arms the capture for one window id, and the
  software renderer copies the complete frame out of the softbuffer surface
  just before `present_with_damage` (which consumes the buffer). The copy
  happens only while armed, so steady-state rendering pays one atomic load;
  the workbench polls `take()` from a Slint timer after forcing a redraw.
- On Windows its software renderer queries the Win7-era User32
  `SystemParametersInfoW` settings `SPI_GETFONTSMOOTHING`,
  `SPI_GETFONTSMOOTHINGTYPE`, and `SPI_GETFONTSMOOTHINGORIENTATION`. It uses
  RGB/BGR LCD channel order only when the user has ClearType enabled; disabled
  ClearType, a failed settings query, or a transposed (rotated) render safely
  averages the RGB coverage back to normal grayscale anti-aliasing. This adds
  no DirectWrite, WinRT, or Windows 8+ dependency.
- Its frameless edge-resize handler explicitly ignores maximized windows and
  resets any cached resize cursor before dispatching pointer input. Winit still
  advertises a maximized borderless HWND as resizable; forwarding that gesture
  into Win7's native resize loop can otherwise strand later move/resize input.
- Its Winit adapter is registered before the first visibility/layout pass, so
  the synchronous Win32 `WM_SIZE` emitted by the initial `SetWindowPos` reaches
  Slint. This removes the first-open bottom gap and makes auxiliary dialogs
  center correctly on their first display.
- Frameless auxiliary windows are centered inside the Winit adapter after
  Slint has resolved their preferred size but before their first native show.
  The auxiliary rectangle uses that just-requested physical preferred size:
  Win32 applies `request_inner_size` through `SWP_ASYNCWINDOWPOS`, so
  `GetWindowRect` would still report the creation size at this point. The
  active (or maximized workbench) reference HWND is measured with
  `GetWindowRect`, so Win7's DPI virtualization cannot mix monitor work-area
  and window coordinate spaces. This avoids the first-open offset that
  application-level timers could only correct on a later show.
- Its frameless-window setup keeps Winit's maximize button disabled. A custom
  Slint title bar owns all caption actions; re-enabling the native maximize
  box during a resize-property update can expose a latent Win7 caption button
  after the first minimize or maximize transition.
- Its Windows top-level surfaces opt out of Winit's transparent/layered DWM
  path. Every rcho window paints an opaque root, whereas transparent Win32
  activation can briefly expose erased client-edge pixels before the queued
  software redraw. On focus gain, the adapter therefore forces and presents a
  complete frame in the focus-message turn; this keeps the custom border
  opaque on Win7 and removes the corresponding edge flash on newer Windows.
- `winit` 0.30.2 retains its upstream licensing information. Its Win32
  undecorated-window styles remove `WS_CAPTION`, `WS_SYSMENU`, and native
  caption boxes while retaining `WS_SIZEBOX`; Winit's default concealed
  caption is susceptible to appearing after a Win7 frame refresh and stealing
  the first click from the custom title bar.
- Frameless Winit windows now acknowledge `WM_NCACTIVATE` themselves after
  updating their active state and return handled for `WM_ERASEBKGND`. This
  prevents User32 from briefly drawing or erasing a native non-client edge
  around the retained `WS_SIZEBOX` before Slint presents the custom border;
  normally decorated windows retain upstream default processing.
- For undecorated windows, Winit now constrains `WM_GETMINMAXINFO` maximized
  bounds to the monitor work area (`rcWork`) rather than the full monitor. The
  HWND, Slint surface, and taskbar edge therefore share one geometry from the
  first maximize frame, preventing the Win7 taskbar white-strip flash.
- `i-slint-common` 1.18.0, `i-slint-core` 1.18.0, and
  `i-slint-renderer-software` 1.18.0 retain their upstream licensing
  information. Their feature defaults no longer enable system-font discovery
  or the shared Fontique/Parley text path. The workbench embeds SimSun, so the
  Win7 target has no need to query DirectWrite and its WinRT error bindings.
- `i-slint-core` raises `DirtyRegion::MAX_COUNT` from 3 to 16 (with the
  matching `PHYSICAL_REGION_MAX_SIZE` in the software renderer). A workbench
  frame change (radar image + file-list rows + status bar + side panels)
  dirties far more than three disjoint rectangles; the three-slot merge
  fallback unioned them into a whole-window bounding box, so every file
  switch repainted the entire surface (~4 Mpx at 175% scale).
- The renderer additionally carries the subpixel order from its Winit backend
  into the embedded bitmap text scene and implements per-channel blending for
  RGB8, RGB565, premultiplied RGBA, and the Win32 softbuffer target. Text alpha
  is multiplied into each coverage before compositing, so translucent labels
  remain correct and a zero-coverage channel never darkens the background.
- `fontique` 0.11 and `parley` 0.11 are vendored only to keep the feature
  resolution consistent with the Slint patches above. They retain their
  upstream license files and must stay version-aligned with the pinned Slint
  release (0.11 with 1.18.0).
- `fontique` 0.11 `CommonData::load_fonts_from_paths` now uses a fresh
  per-font batch map. Upstream reuses one map across the whole directory
  scan while `register_font_impl` merges that entire map into the family
  table on every font, so each scanned face re-appends every previously
  scanned face of its family — family font lists grew quadratically (~29k
  entries across 121 families on a stock Windows install) and the settings
  font scan took seconds. The fix keeps the merge linear.
- `i-slint-core` partial-render item filtering includes a physical-pixel
  margin around floating-point item bounds. Software damage is rounded out
  before clearing; adjacent toolbar borders must remain in the draw list
  when those rounded edge pixels overlap them. Actual writes still use the
  renderer's damage clip. This fixes a 261-pixel border gap when keyboard
  navigation changes the combo text at 175% scale with a stable popup.

The patches
are intentionally narrow for the Win7 software-rendering baseline.
When any upstream dependency is updated, re-evaluate and reapply only the
equivalent behavior rather than copying these files blindly. Run
`../../builders/build-win7-slint.ps1` afterwards; its PE import gate must continue to pass.

## Patch archives and upgrading Slint

`vendor/patches/<crate>-<version>.patch` is a unified diff of each vendored
crate against its pristine crates.io source, so local changes can be replayed
onto a newer Slint release instead of being rediscovered by hand.

Regenerate after touching vendored code (from `frontend-slint/vendor`; the
pristine copy may come from the cargo registry or a downloaded `.crate`):

```sh
git diff --no-index --ignore-cr-at-eol <pristine>/i-slint-compiler-1.18.0 i-slint-compiler-1.18 > patches/i-slint-compiler-1.18.0.patch
# same for i-slint-core / i-slint-renderer-software / i-slint-common /
# i-slint-backend-winit / fontique / parley
```

Note: `git apply` is unreliable for replaying these archives inside this
repository (it silently prints "Skipped patch" for every file and exits 0).
Use GNU `patch -p<N>` instead — count the directory components in the diff
headers for N — and inspect every `.rej` file afterwards. For the 1.18
archives (headers `a/<pristine path>/... b/i-slint-<crate>-1.18/...`) run
`patch -p2` from inside a pristine source extraction; the `b/`-side names
resolve for both modified and new files.

The Win7 backend overlays are archived alongside those patches as
`patches/softbuffer-0.4.8.patch` and
`patches/i-slint-backend-winit-1.18.0.patch`. The pinned Winit overlays are
archived as `patches/winit-0.30.2.patch`; regenerate the matching archive
whenever a vendored source changes.

To upgrade Slint: unpack the new crates.io sources, apply the patch with
GNU `patch` (see the note above about `git apply`), resolve the rejects by
reading the intent documented above, then update the `[patch.crates-io]` pins
and rebuild.
Most hunks are self-contained (new files like `passes/gb2312_table.rs` and
`pack_mono_1bpp` never conflict); the expected touch points are
`passes/embed_glyphs.rs` and `fonts.rs::register_bitmap_font`.

## Mandatory Win7 import audit

Windows 7 does not ship `combase.dll`. A startup error naming that DLL means a
Cargo feature or patched dependency has reintroduced a Windows 8+/WinRT
dependency; it is **not** a redistributable-runtime problem. Never copy,
bundle, or ask an operator to install `combase.dll`. Remove the feature or
dependency that imports it instead.

The final distribution executable, not merely a debug build or a previous
release, must pass `../../builders/build-win7-slint.ps1`. In addition to `combase.dll`, the
gate rejects `api-ms-win-*`, `ext-ms-win-*`, `SystemParametersInfoForDpi`,
`CreateWaitableTimerEx`, and `GetDpiForWindow` strong imports. Before changing
the text stack, inspect `cargo tree -e features -i fontique`: Fontique/Parley
`default` or `system` discovery can pull the `windows` WinRT bindings back in.
