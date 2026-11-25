# Editions & Project Files (1.19+)

A project file records the language **edition** and shared compiler paths.
An edition selects language *behavior*, not the Slint version you build with:
you can run 1.19.1 while keeping 1.18 semantics.

## Choosing an edition

- New project: `"1.19"`.
- Existing project being upgraded: `"1.18"` until you have checked the 1.19
  behavior changes below. Raise it deliberately, in its own change.
- The oldest edition is `1.18` (behavior of 1.18 and earlier); an edition newer
  than the compiler is an error.
- Without an edition the compiler applies **none** of the edition-gated
  behavior changes and prints an info. The `slint!` macro warns too; put
  `#[edition = "1.19"]` at the top of its body when you want 1.19 semantics.

## `slint-project.json`

Name it `slint-project.json`, in the project directory. JSON with `//` and
`/* */` comments:

```json
{
  "edition": "1.19",
  "main": "ui/main.slint",
  "include-paths": ["ui/shared"],
  "library-paths": { "widgets": "ui/widgets" },
  "experimental-features": ["closures"]
}
```

- `edition` is required when the file is used. `main` is required when you pass
  the file or its directory to the compiler.
- Paths resolve against the project-file directory.
- `library-paths` may point at a library's project file; that library keeps its
  own edition and paths.
- `experimental-features` lists exact names. The old catch-all value `"1"` and
  unknown names are errors in 1.19.
- `SLINT_ENABLE_EXPERIMENTAL_FEATURES` still works for env-driven builds, but
  also takes a comma-separated list of names.

## Build wiring

```rust
// build.rs
fn main() {
    slint_build::compile(".").unwrap();   // directory containing slint-project.json
}
```

With a custom config, an explicit edition wins over the project file:

```rust
let config = slint_build::CompilerConfiguration::new().with_edition((1, 19));
slint_build::compile_with_config("ui/main.slint", config).unwrap();
```

- C++: `slint_target_sources(app .)`, or
  `slint_target_sources(app ui/main.slint EDITION 1.19)`.
- Node.js: `slint.loadFile(".")`, Python: `slint.load_file(".")`.
- CLI: `slint-compiler --edition 1.19 …`, `slint-viewer --edition 1.19 …`.
- Editor / viewer project root: `slint-viewer --project-root . ui/main.slint`.
- API/CLI options win over the project file; include paths combine, with
  command-line paths searched first.

## 1.19 behavior changes

Only active with edition `1.19` (or newer):

| Change | What to check / fix |
|---|---|
| `if` / `for` elements now count toward the parent's size, including components that inherit a layout | A parent can grow; wrapped text can add a line. Give the parent explicit `width`/`height` where it must stay fixed. A child reading `parent.width` can become a binding loop — use `100%`, or size the parent. |
| A `width`/`height`/`x`/`y` set only by some states keeps its **default** when no state applies | Code that relied on `0px` must set that value outside the states. A parent that follows a state-set size can now form a binding loop. |
| Nested binding loops inside a longer loop are reported too | Fix the inner loop as well; a loop through the window layout is no longer only a warning. |

These are language behavior, not new syntax. You can use every 1.19 feature
with an older edition; only behavior changes are gated.

## 1.19.1 patch notes worth knowing

- `TextInput` / `TextEdit`: tabbing into a multi-line field no longer selects
  all text.
- `ListView`: fixed empty content after setting `content-y` past the end, and
  long callback chains with mixed row heights / smooth `ensure-row-visible()`.
- `SystemTrayIcon`: hidden icons stay hidden at startup; Explorer restart adds
  the icon back on Windows.
- Software renderer: fixed a rounded-`Clip` panic when it is far outside the
  view or taller than 32767 physical pixels.
