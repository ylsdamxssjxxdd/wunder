---
title: Android Runtime
nav_title: Runtime
summary: What actually ships inside the APK: the bundled Ubuntu rootfs and its tool list, the two browser engines, the permission it needs, and what proot cannot do.
updated_at: 2026-11-14
---

# Android Runtime

Android packs the Desktop engine together with an Ubuntu environment into the APK: after installing you can run commands, read and write files, and call tools on the phone with no root and no download of a runtime.

## Parts

| Part | Notes |
| --- | --- |
| OS requirement | Android 8.0 (API 26) and up, arm64-v8a; the x86_64 slice is for emulator checks only |
| UI | Single-column phone workbench, reusing the Desktop Slint UI code |
| Command environment | A bundled Ubuntu arm64 rootfs run by proot inside the app's private directory, without requesting root |
| Data | Entirely in the app's private directory; uninstalling clears it |

## Bundled tools

The rootfs ships the usual tools, ready for the agent:

- **Development**: python, git, ripgrep, 7zip, static ffmpeg, binutils, patchelf, upx, sqlite3
- **System**: curl, wget, zip/unzip, procps, psmisc, lsof, bc, bzip2, time, iproute2, rsync, tmux
- **Fonts**: fontconfig plus DejaVu, Liberation, Noto Color Emoji and WenQuanYi Micro Hei, covering Latin, monospace, Chinese and emoji
- **Phone image and recovery toolbox**: adb, simg2img, erofs-utils, debugfs, squashfs-tools, mtools/dosfstools/exfatprogs/ntfs-3g, testdisk/photorec, ddrescue, binwalk, foremost, nmap

These tools work at file level only: proot has no block devices, no FUSE and no raw sockets, so `fastboot`, `mount`, `tcpdump`, USB and Bluetooth control are unavailable.

## Two browser engines

| Case | What runs | Requirement |
| --- | --- | --- |
| Ordinary web work | The system WebView, hosted in a floating window | Grant "display over other apps" on first use |
| Frame-by-frame rendering skills (e.g. knowledge shorts) | Playwright 1.40 with Chromium 1091, installed at `/opt/browser/chrome-linux` | Shipped inside the release APK, works offline |

Development builds may omit the browser pack; the WebView path still works, but rendering skills report a missing runtime.

## Boundaries

- No ports, no background service, and no inbound access.
- Tool semantics match Desktop, but system-level abilities (tray, global hotkeys, desktop control) do not exist on a phone.
- Terminal key bindings follow phone input; see [Android Usage](/docs/en/forms/android-usage/).
