---
title: Compatibility
summary: The Compatibility core lets the Honeycomb (desktop) and the Helm (cli) run in older operating-system environments under explicit constraints: Windows 7 32-bit and Ubuntu 18.04 each have an offline build chain, and the interface uses software rendering that does not depend on a GPU.
updated_at: 2026-10-13
---

# Compatibility

The Compatibility core lets the Honeycomb (desktop) and the Helm (cli) run in older operating-system environments, and states explicitly where that runtime environment is constrained.

## Which older systems are supported

| System | Approach |
| --- | --- |
| Windows 7, 32-bit | A dedicated `i686-win7-windows-gnu` Tier 3 target plus a build-std offline toolchain |
| Ubuntu 18.04 x64 / arm64 | An offline build chain and sysroot |

## Why it runs

- **The interface does not depend on a GPU**: Slint uses software rendering.
- **Building does not depend on network access**: both the Windows 7 32-bit and the Ubuntu 18.04 chains are offline toolchains, and the Ubuntu side carries a sysroot as well.
- **Command sessions adapt to old terminals**: command sessions are carried over winpty, and output is decoded as GBK.
- **Startup recovery scan and self-check**: recovery scanning and self-checking run at startup.

## What it means for users

Windows 7 32-bit and Ubuntu 18.04 (x64 / arm64) are not excluded runtime environments; the Honeycomb and the Helm can be installed on old machines and run there. The interface renders on machines without a GPU. Command sessions are carried and decoded in old Windows terminals.

## Boundaries and current limits

- Build scripts still carry dead Tauri links — pointing at directories that no longer exist. That is an engineering gap and says nothing about capability.
- win7 interlink nodes do not declare `tool.exec`, so the capability surface of old devices is narrower.
