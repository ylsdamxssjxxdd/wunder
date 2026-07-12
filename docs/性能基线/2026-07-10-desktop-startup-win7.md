# 2026-07-10 desktop Win7 startup performance comparison

## Goal

- Startup path: Electron shell plus the local desktop bridge on low-memory 32-bit Windows 7 deployments.
- Risks: a blank first window during bridge boot, and unconditional file copying during every restart.

## Environment

- OS: Windows development host; this is a local bridge measurement, not a physical Windows 7 acceptance run.
- Runtime: current `wunder-desktop-bridge` release binary, SQLite, a persistent local data directory, and the built frontend assets.
- Data: packaged built-in skill source containing 193 files (about 1.87 MB); no existing chat or agent task records.
- Sampling: three separate process starts per variant. The median is used.
- Scope isolation: the repository had unrelated uncommitted changes outside the desktop startup files; the measured bridge data directories were under `temp_dir/`.

## Method

1. Launch `target/release/wunder-desktop-bridge.exe` against a fixed temporary data directory and an HTTP port.
2. Poll `/wunder/desktop/bootstrap` every 50 ms until it responds successfully.
3. Stop the process, then repeat three times without deleting that data directory.
4. Before: the previous release binary, whose packaged skill copies were refreshed on every process start.
5. After: the rebuilt release binary with `WUNDER_DESKTOP_PACKAGED=1`, matching the environment applied by the packaged Electron shell.

## Results

| Metric | Before | After | Change | Result |
| --- | ---: | ---: | ---: | --- |
| Local bridge ready, median | 1,697.0 ms | 97.6 ms | -1,599.4 ms (-94.2%) | Improved |
| Local bridge ready, first run | 1,448.3 ms | 714.1 ms | -734.2 ms (-50.7%) | Improved |
| Reused skill sync, bridge log | about 1,180 ms | 1.2 ms | about -1,179 ms | Improved |
| Electron loading-shell default delay | 1,200 ms | 0 ms | -1,200 ms | Improved by code path |

Raw timing files used for this comparison remain available locally:

- `temp_dir/desktop-startup-baseline/persistent-before/results.json`
- `temp_dir/desktop-startup-after/results.json`

## Bottleneck Detail

| Stage | Before | After (reused start) | Notes |
| --- | ---: | ---: | --- |
| Workspace built-in skill synchronization | 565.8-599.3 ms | 0.6-0.7 ms | Matching packaged copy is reused. |
| User-tool built-in skill synchronization | 454.0-613.8 ms | 0.6 ms | Matching packaged copy is reused. |
| Bridge total from startup log | 1,284.1-1,316.1 ms | 44.7-49.7 ms | Excludes the polling interval. |

## Implementation and Safety

- The Electron loading page is requested immediately rather than after a fixed delay, so slow local initialization never leaves a blank native window.
- A copied built-in skill tree is reused only for a packaged runtime when its manifest exists, every manifest directory still exists, and its source/version stamp matches the current package.
- First launch, package upgrade, missing or damaged copy, development mode, and `WUNDER_FORCE_SKILL_SYNC=1` continue to execute a full synchronization.

## Conclusion

- Verdict: passed for the local bridge startup path; no measured regression.
- The tested steady restart path is below 100 ms for bridge readiness. The loading shell also removes the prior fixed 1.2-second blank-window delay.
- A physical 32-bit Windows 7 device still needs release-package acceptance to measure Electron process launch, legacy WebGL fallback, antivirus scanning, and first paint. This report does not claim those device-level timings.

## Local Electron End-to-End Measurement

### Environment and method

- Host: local 64-bit Windows 11 development machine, current release bridge binary, built frontend assets, and the development Electron executable. This is intentionally not represented as a Win7 or 32-bit acceptance result.
- Invocation: `npm run benchmark:startup --workspace wunder-desktop-electron -- -Runs 5 -DataRoot temp_dir/desktop-electron-startup-benchmark-local -ResetData`.
- Isolation: Electron `userData`, SQLite data, and workspace were redirected to the benchmark data directory. Existing desktop data was not read or modified.
- Readiness definition: the main chat view completed its critical bootstrap (`messenger-bootstrap-finish`), meaning the main surface has mounted and its required initial data work has settled. Deferred realtime work is excluded.

### Results

| Metric | First isolated launch | Reused restart median (4 runs) | All-run median (5 runs) |
| --- | ---: | ---: | ---: |
| Loading window visible | 211.9 ms | 211.2 ms | 211.9 ms |
| Local bridge ready | 689.1 ms | 224.4 ms | 226.7 ms |
| Main document loaded | 821.2 ms | 367.8 ms | 376.7 ms |
| Vue application mounted | 840.9 ms | 390.6 ms | 400.3 ms |
| Main chat critical bootstrap complete | 1,434.7 ms | 897.8 ms | 939.7 ms |

### Remaining hotspots

- On reused restarts, the renderer needs about 23 ms after document load to mount the root application, about 92 ms to load and construct the chat view, and about 409 ms for its critical initial data bootstrap.
- The first isolated launch adds about 465 ms of bridge setup compared with the reused median; this is expected first-run local database/resource initialization, rather than the previously eliminated repeated skill-copy work.
- The front-end distribution contains 502 files totaling about 31.2 MB. The core chat chunk is about 1.64 MB, and optional panels include chunks above 5 MB; these are the next likely candidates if slower hardware still misses the target.

### Conclusion

- On this host, the normal restart path satisfies the stated seconds-open goal: visible feedback in about 0.21 seconds and the usable chat surface in under 0.9 seconds at the median.
- This confirms there is no fixed Electron wait remaining in the measured path. It does not establish an absolute performance limit: a packaged Electron 22 Win7 ia32 run remains necessary before making an equivalent claim for the deployment target.
