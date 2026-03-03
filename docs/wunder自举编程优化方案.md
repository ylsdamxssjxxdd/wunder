# wunder bootstrapping engineering optimization plan

## Goal
Build a Codex-like developer experience in `wunder-desktop`, then use Wunder itself to develop and evolve Wunder safely and continuously.

## Gap vs codex-main
- Prompting: Wunder had only `tool_call` and `function_call`; codex style needs mixed structured + freeform guidance.
- File editing: old line-edit JSON tool (`edit_file`) is weaker than patch-first workflows.
- Tool protocol robustness: freeform payload parsing and retry guidance were limited.
- End-to-end consistency: backend/frontend/docs/config had to be aligned for stable developer usage.

## Landed in this round
1. Added `freeform_call` mode (alongside `tool_call` / `function_call`).
2. Replaced file editing capability with `apply_patch` / `application patch` end-to-end.
3. Added freeform prompt protocol blocks in both zh/en templates.
4. Added parser support for `<input>...</input>` tool payloads.
5. Added Responses `custom_tool_call` extraction and backfill support.
6. Synced desktop/web mode selectors, tool labels, evaluation cases, and docs.

## P0/P1/P2 roadmap
### P0 (done)
- Three tool-call modes wired through API, prompting, orchestrator, and UI.
- `apply_patch` available as the coding edit primitive.
- Baseline checks and targeted tests pass.

### P1 (1-2 weeks)
- Improve patch chunk matching heuristics and conflict diagnostics.
- Add stricter rollback guarantees and richer retry hints.
- Add test coverage for move/rename conflicts, multi-file rollback, EOF markers.

### P2 (2-4 weeks)
- Upgrade tool spec model to explicit `Function + Freeform` typed specs.
- Add stronger diff-review/approval UX for desktop/cli.
- Ship a dedicated "Wunder self-hosted coding" prompt/profile preset.

## Acceptance metrics
- One-shot multi-file patch success rate >= 85%.
- First retry recovery rate after patch failure >= 70%.
- Unexplainable tool errors < 5%.
- No path escape writes, no partial writes on failed patch transactions.

## Operating policy
- Use `freeform_call` for strong models with grammar-following ability.
- Keep `function_call` as stable fallback for weaker models.
- Continuously feed failed patch samples back into evaluation cases and regression tests.
