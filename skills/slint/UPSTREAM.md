# Upstream & local deltas

This skill is maintained in-repo. The generic upstream copy ships inside the
`slint` crate at `skills/slint/` (for 1.19.1:
`$CARGO_HOME/registry/src/*/slint-1.19.1/skills/slint/`), laid out as
`SKILL.md` + `guidance/`.

## Why this is not copied wholesale

The packaged 1.19.1 skill is intentionally release-agnostic: it still targets
`≥ 1.17` and says "latest", and it has no `slint-project.json` / editions
section, no 1.19 `if`/`for` sizing notes, and no `ContextMenuArea` lowering
note. Copying it over this tree would remove the 1.19 behavior guidance and the
host-specific MCP workflow.

## Local deltas to preserve when syncing

- Flat `setup.md` / `reference/*.md` layout instead of upstream `guidance/`.
- Doubao MCP Apps flow in `SKILL.md` (`preview_slint`,
  `get_preview_screenshot`) and the project's stricter "render, then review"
  workflow.
- Expanded MCP server instructions in `reference/debugging-and-mcp.md`.
- 1.19 additions: `reference/editions-and-project-file.md`, edition/state
  behavior, project-file build wiring, and the Win32/software-renderer caveats.

## Sync procedure

1. Locate the crate copy for the target Slint version in `$CARGO_HOME`.
2. Diff it against this directory; port only new upstream guidance wording.
3. Re-apply the deltas above; never copy `guidance/` blindly over the flat tree.
4. Re-check relative links and run at least one `slint-viewer --check` /
   screenshot flow before declaring the skill updated.
