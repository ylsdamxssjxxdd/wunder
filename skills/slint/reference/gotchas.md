# Gotchas & Common Compile Errors

Compiler messages are quoted verbatim — match the error you see against the
bold text.

- **`Invalid unit 'em'`** — Slint has no `em`; the message lists the valid
  units. For font-size-relative spacing use `rem`, not a hand-converted `px`
  (rewriting `0.04em` → `0.4px` silently breaks scaling).
- **`Cannot convert float to length. Use an unit, or multiply by 1px to
  convert explicitly`** (likewise `int to length`, `float to angle`, …) —
  unit types and plain numbers are distinct. Convert with `value * 1px` /
  `len / 1px`, `* 1deg` / `/ 1deg` for angles (trigonometric functions take
  and return `angle`).
- **`Unknown unqualified identifier 'hsl'`** — there is `hsv` but no `hsl`.
  Colors: hex literals, `rgb()`, `rgba()` (alpha `0.0..1.0`), `hsv()`, and
  `oklch(l, c, h)` (`l` is `0..1`; a `%` chroma maps `100%` → `0.4`). Helpers
  like `.mix()`, `.with-alpha()`, `.brighter()` and the full signatures are on
  the docs' colors & brushes page.
- **`padding only has effect on layout elements`** (warning; likewise
  `spacing`) — see [language-and-layout.md](language-and-layout.md) for the
  wrap-in-a-layout fix.
- **Literal `${name}` or `{name}` shows up in the UI** (no diagnostic) —
  string interpolation is backslash-brace: `"Count: \{root.count}"`.
- **Math**: bare names (`floor(x)`, `max(a, b)`), methods (`x.floor()`,
  `x.clamp(lo, hi)`), and the `Math` namespace all work. But `/` never does
  integer division — the result is a float, and assigning it to an `int`
  silently truncates toward zero; use `.floor()`/`.round()`/`.ceil()` to pick
  the rounding you mean.
- **Enum values** are `EnumName.value`, lowercase for builtins
  (`PointerEventKind.down`, `ColorScheme.dark`); special keys are CamelCase in
  the `Key` namespace (`Key.Escape`, `Key.UpArrow`).
- **Transforms** (`transform-rotation`, `transform-scale`, `transform-origin`)
  apply visually to the element and its descendants; the layout box stays the
  same.
- **Dashed paths** `(1.19+)`: `stroke-dash-array: [6px, 3px];` and
  `stroke-dash-offset: 2px;` on `Path`; lengths are logical pixels.
- **Gradients**: `@linear-gradient(angle, color stop%, …)`,
  `@radial-gradient(circle [radius] [at x y], color stop%, …)` (radius and
  center are `(1.17+)`; `ellipse` shape is `(1.19+)`), and
  `@conic-gradient([from angle] [at x y], color deg, …)`.
- **Animations** are declared on *properties*, inside the element whose
  property changes: `animate width { duration: 200ms; }`.
- **Only bind a property when overriding the default.** An unbound `visible`,
  `opacity`, `x`, … stays compiler-constant; inside a layout an explicit
  `x: 0` even overrides the computed position.
- **`if`/`for` now affect parent size under edition 1.19.** A parent can grow,
  and a child reading `parent.width` can become a binding loop
  (`The binding for the property … is part of a binding loop`). Give the
  parent an explicit size, or use `width: 100%` instead of `parent.width`.
- **A size/position set only by states keeps its default in 1.19.** Code that
  expected `0px` while no state applied must set that value outside the
  states. See [editions-and-project-file.md](editions-and-project-file.md).
- **`ContextMenuArea` is not a sizing parent in 1.19.** It lowers to a
  non-visual `Empty`; a `ScrollView` or `TextInput` inside it can get
  `parent.width == 0`. Keep `Menu` in it and put the visual children as
  siblings. See [events-and-overlays.md](events-and-overlays.md).
- **Experimental features must be named.** `SLINT_ENABLE_EXPERIMENTAL_FEATURES`
  and `experimental-features` take a list (`closures`, `interfaces`, …); the
  old catch-all `1` and unknown names are errors.

For "element won't fill / is centered" and "padding does nothing", see
[language-and-layout.md](language-and-layout.md).
