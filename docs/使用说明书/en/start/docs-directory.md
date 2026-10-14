---
title: Documentation Directory
summary: A guide to the repo's documentation ecosystem: how the overview and design docs, technical manual, API reference, user manual, performance standards and scoring, changelog, and lessons-learned divide the work, and how to read them.
read_when:
  - You want to know which doc layer answers your question
  - You are about to write or update a document and need the conventions
source_docs:
  - docs/使用说明书/site.json
  - docs/性能标准.md
  - docs/SOP.md
  - docs/功能迭代.md
updated_at: 2026-10-13
---

# Documentation Directory

wunder's documentation falls into seven categories, each answering a different kind of question. When writing docs, don't mix them: **design docs cover boundaries and rationale**, the **technical manual covers engineering execution** (change points, run method, verification points), the **API reference covers interface contracts**, the **user manual covers user and admin operations**, **performance standards and scoring cover conventions and quantification**, the **changelog covers the flow of changes**, and **lessons-learned covers recurring problems**.

## What the seven categories are for

| Category | Location | Audience | When to look |
| --- | --- | --- | --- |
| **Overview design** | `docs/总体设计.md`, `docs/wunder系统简介.md` | All readers | To grasp the product positioning and the division between forms in one line |
| **Design docs** | `docs/设计文档/` (01–13 plus topical pieces on context compression / real-time sync / tools / the memory system / the goal-state system) | Architecture reviews, trade-off decisions | To learn **why it's designed this way** and where the boundaries are |
| **Technical manual** | `docs/技术说明书/` (chapters 01–13 + README) | Engineering executors | To learn **change points, run method, verification points** |
| **API reference** | `docs/API文档.md` | Interface consumers | Concrete HTTP / WebSocket endpoint contracts |
| **User manual** | `docs/使用说明书/zh-CN/`, `docs/使用说明书/en/` | Users and admins | Interface maps and steps for concrete operations |
| **Performance standards and scoring** | `docs/性能标准.md`, `docs/性能评分体系.md`, `docs/性能基线/` | Operations and benchmark testers | Before/after comparison, pass/fail judgment, per-form independent scoring |
| **Changelog / lessons-learned** | `docs/功能迭代.md`, `docs/经验教训.md` | Project maintainers | The change flow each round; recurring pitfalls |

## The user manual (this site)

`docs/使用说明书/site.json` drives the navigation for both the Chinese and English sites. **The Chinese site is the authoritative version; the English site follows it.**

### Start

| Page | Content |
| --- | --- |
| [Quick Start](/docs/en/start/quickstart/) | Get your first task running in 4 steps |
| [Pupa intro](/docs/en/start/desktop/) | Individual users download and go |
| [Hull deployment](/docs/en/start/server/) | Multi-user sharing, unified management |
| [Helm usage](/docs/en/start/cli/) | Terminal-driven, automation |
| [First-run troubleshooting](/docs/en/start/troubleshooting-first-run/) | Locate install / launch / access failures by form |
| [Manual overview](/docs/en/start/hubs/) | Navigation home |
| [Documentation directory](/docs/en/start/docs-directory/) | This page |

### Surfaces

| Page | Content |
| --- | --- |
| [Surfaces overview](/docs/en/surfaces/index/) | The four forms' division and capability matrix |
| [Pupa interface](/docs/en/surfaces/desktop-ui/) | The main window's two pages + settings' 12 categories |
| [Beehive interface](/docs/en/surfaces/frontend/) | The single-page app in the browser |
| [Bridge interface](/docs/en/surfaces/web-admin/) | The admin governance backend |

### Other groups (this page lists only the site's main tabs, not everything)

- Core concepts → `/docs/en/concepts/`
- Tools → `/docs/en/tools/`
- Reference → `/docs/en/reference/`
- Help → `/docs/en/help/`

## Design docs (`docs/设计文档/`)

**They answer "why it's designed this way" and don't cover how to change things.** Numbers 01–13 form the backbone:

| Number | Topic |
| --- | --- |
| 01 | System overall design |
| 02 | Agent core design |
| 03 | Real-time projection system design |
| 05 | Tools, skills, and MCP system design |
| 06 | Memory, knowledge, and workspace system design |
| 07 | Integration, gateway, and channel system design |
| 08 | Data storage and state-model design |
| 09 | User-side frontend design |
| 10 | Admin-side and operations backend design |
| 11 | Desktop-CLI local form design |
| 12 | Security, permissions, and isolation design |
| 13 | Observability, testing, and acceptance design |

Topical sub-pages: context compression design, real-time sync design, tool design, memory system design, goal-state system design, and the apply-patch tool refactor plan; there's also a Beehive high-volume thread-log rendering optimization plan (at the root of `docs/`, the plan rewritten and the implementation still to be done).

## Technical manual (`docs/技术说明书/`)

**They answer "change points, run method, verification points" and don't cover boundaries and rationale.** Chapters 01–13:

| Number | Topic |
| --- | --- |
| 01 | System overview and terminology |
| 02 | Repository structure and module responsibilities |
| 03 | Runtime and execution path |
| 04 | Real-time projection and communication path |
| 05 | Tools, skills, MCP, and external capabilities |
| 06 | Data storage, workspaces, and configuration |
| 07 | Interfaces, gateway, and channel integration |
| 08 | Deployment, startup, and run modes |
| 09 | Development, extension, and change flow |
| 10 | Operations, troubleshooting, and acceptance |
| 11 | Tool-result trimming and the observability path |
| 12 | The persona system |
| 13 | The goal-state system |

Extra: Docker acceptance for throughput tests.

## API reference (`docs/API文档.md`)

A single file, from the interface consumer's perspective. All HTTP endpoints use the `/wunder/*` prefix (**not `/api/*`**); error responses write back `x-trace-id` and `x-error-code` response headers. WebSocket channels include `chat/ws`, `ws`, `gateway/ws`, `interlink/ws`, `interlink/remote_ws`, `user_world/ws`, and `desktop/lan/ws` (event names are scattered rather than centrally enumerated).

## Performance standards and scoring (`docs/`)

| Document | Content |
| --- | --- |
| `docs/性能标准.md` | Same-environment, same-entry before/after comparison + standard-entry table |
| `docs/性能评分体系.md` | Three forms scored **independently 0–100 and weighted; not summable across forms** |
| `docs/性能基线/` | Historical baseline snapshots; the latest include `2026-10-13-server-capacity.md` and `2026-10-13-thread-view-v3.md` |

**Conventions**: performance isn't an adjective. Any list, log, or panel must be able to state its paging / limit / bounded semantics; performance reviews use the fixed user `performance_admin`, the work root `.wunder_perf`, and a 5s command timeout.

## Changelog and lessons-learned

- `docs/功能迭代.md`: each round's changes go into a designated category block. The types are: added / changed / fixed / performance / docs / refactor / security / engineering / testing / removed / deprecated.
- `docs/经验教训.md`: **for recurring problems, you must write an entry after fixing one**, so the same mistake isn't repeated.

## Project SOP

`docs/SOP.md`: a 6-step flow (worktree → quality-gate → peer review → merge-gate → PR → merge and cleanup); Biome `pnpm check`, `pnpm lint`; files warn at 200 lines and hard-cap at 350.

## Suggested reading paths

**New users**: user manual → Quick Start → Pupa intro; when something goes wrong, first-run troubleshooting.
**Developers getting started**: technical manual 01–03 → 02 repository structure and modules → 03 runtime and execution path; then design docs 01–02 to fill in the "why."
**Adding an interface**: technical manual 07 → API reference → design doc 07.
**Performance issues**: performance standards → performance scoring → performance baselines; for specific paging constants, see technical manual 03–04.
**Operations and acceptance**: technical manual 10 → `docs/SOP.md`; `docs/功能迭代.md` for historical decisions; `docs/经验教训.md` to avoid repeating pitfalls.

## Further reading

- [Manual overview](/docs/en/start/hubs/)
- [wunder home page](/docs/en/index/)
- [API index](/docs/en/reference/api-index/)
- [Configuration reference](/docs/en/reference/config/)
