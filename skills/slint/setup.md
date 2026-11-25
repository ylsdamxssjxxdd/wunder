# Project Setup

Start new projects from the official templates — dependencies and build wiring
set up the way the docs recommend:

- Rust: <https://github.com/slint-ui/slint-rust-template>
- C++ (CMake + `FetchContent`): <https://github.com/slint-ui/slint-cpp-template>
- Node.js: <https://github.com/slint-ui/slint-nodejs-template>
- Python: <https://github.com/slint-ui/slint-python-template>

## Project file (1.19+)

Give the project a `slint-project.json` next to the sources, and choose the
language edition deliberately:

```json
{
  "edition": "1.19",
  "main": "ui/main.slint",
  "include-paths": ["ui/shared"]
}
```

- `"1.19"` opts into the 1.19 behavior changes; `"1.18"` keeps the older
  behavior while still building with 1.19. A missing edition applies none of
  the behavior changes.
- Build APIs accept the project file or its directory; the directory form is
  the normal setup. See
  [editions-and-project-file.md](reference/editions-and-project-file.md) for
  settings, precedence, and the behavior-change checklist.

Rust:

```rust
// build.rs
fn main() {
    slint_build::compile(".").unwrap();
}
```

`CompilerConfiguration::new().with_edition((1, 19))` overrides the project
file. The `slint!` macro does not read a project file; put
`#[edition = "1.19"]` at the top of its body to silence the warning and opt in.

C++: `slint_target_sources(app .)`. With an inline source list, add
`EDITION 1.19`. `slint_target_sources` still generates a header named after the
main `.slint` file.

Node.js: `slint.loadFile(".")`. `slint-project.json` in the source file's
directory is picked up automatically when importing a `.slint` file or
generating TypeScript declarations. Node.js 22.18+ is required by 1.19.

Python: `slint.load_file(".")`. `slint.loader` exposes the loaded project as
`slint_project`.

## Language wiring

Rust compiles `.slint` at build time with `slint-build::compile()` in `build.rs`
and `slint::include_modules!()` in main. C++ uses `slint_target_sources()` and
links `Slint::Slint`. Node.js (`slint-ui` package) and Python (`slint` wheel)
load `.slint` files at runtime.

Python note: the `slint` wheel's `requires-python` tracks recent CPython
releases. If `uv add` / `pip install` picks an older Slint than expected, bump
the project's `requires-python` (and `.python-version` for uv) to match the
latest wheel on PyPI before pinning a Slint version.

Preview a `.slint` file without host code: `slint-viewer ui/main.slint`
(hot-reloads on save; install via [tools-install.md](tools-install.md)). Pass
`--project-root .` when the viewer should use the project file.

See [interop.md](reference/interop.md) for connecting business logic.
