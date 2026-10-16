---
title: Compatibility
summary: Offline build chains, software rendering, and terminal adaptation for the Pupa and the Helm on Windows 7 32-bit and Ubuntu 18.04.
updated_at: 2026-10-16
---

# Compatibility

## Supported systems

| System | Approach |
| --- | --- |
| Windows 7, 32-bit | A dedicated `i686-win7-windows-gnu` Tier 3 target plus a build-std offline toolchain |
| Ubuntu 18.04 x64 / arm64 | An offline build chain and sysroot |

## Runtime constraints

- **The interface does not depend on a GPU**: Slint uses software rendering.
- **Building does not depend on network access**: both the Windows 7 32-bit and the Ubuntu 18.04 chains are offline toolchains, and the Ubuntu side carries a sysroot as well.
- **Command sessions adapt to old terminals**: command sessions are carried over winpty, and output is decoded as GBK.
- **Startup checks**: a recovery scan and self-check run at startup.

Both chains work offline, the interface needs no GPU, and command sessions adapt to legacy terminals — Windows 7 32-bit and Ubuntu 18.04 (x64 / arm64) can run the Pupa and the Helm directly.

## Limits

- Build scripts still carry dead Tauri links, pointing at directories that no longer exist. That is an engineering gap, not a capability.
- win7 interlink nodes do not declare `tool.exec`, so the capability surface on legacy devices is narrower.
