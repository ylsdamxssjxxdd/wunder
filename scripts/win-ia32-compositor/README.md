# 32 位 Remotion 渲染器

Remotion 官方只为 Windows 提供 `@remotion/compositor-win32-x64-msvc`，没有 ia32 版本，所以 32 位运行时里的 Remotion 原本无法出片。这里用 Remotion 自己的 `packages/compositor` 源码交叉编译出 ia32 版本。

## 工具链与 wunder 主程序保持一致

| 组件 | 取值 | 说明 |
| --- | --- | --- |
| Rust host | `nightly-2026-03-14-x86_64` | 64 位 host，和 `builders/build-win7-*.ps1` 同一规定（规避 32 位 LLVM 地址空间限制） |
| Rust target | `i686-win7-windows-gnu` | Rust 1.78+ 内置的 Win7 target，tier 3 |
| std | `-Z build-std=std,panic_abort` + rust-src | tier 3 没有预编译 std，从源码编 |
| 编译器 | `i686-w64-mingw32` GCC（msvcrt） | UCRT 版在 Win7 上缺 `ucrtbase.dll` |
| FFmpeg | 7.1（共享库）+ 静态 x264 | compositor 链接 `avcodec-61` / `avformat-61` / `avutil-59` |
| Remotion | 4.0.538 | 必须与 `opt/remotion` 完全同版本 |

用 `i686-win7-windows-gnu` 而不是降级到 Rust 1.77，好处是不必迁就旧语法、也不必改 `Cargo.lock` 版本；std 本身就是 Win7 语义，产物只依赖 `msvcrt.dll`，`objdump` 检查没有任何 Win10 时代的导入。

## 要打的补丁

1. `rust-ffmpeg-sys/build.rs`：`get_zip_name()` 只映射官方目标，加一条 win7 target。
2. `bindings/i686-win7-windows-gnu.rs`：官方不自带，用 bindgen + 原 `build.rs` 的 `Callbacks` 生成；bindgen 求不出 `AV_CH_LAYOUT_*`（枚举移位），用 C 端求值补常量；`AVRational` 补 `PartialEq`。
3. `Cargo.lock`：Remotion 锁的 `proc-macro2 1.0.56` 用了已被移除的 `proc_macro_span_shrink`，抬一次版本。
4. `rust/memory.rs`：见下节，32 位帧缓存上限。

`rust-ffmpeg-sys` 的源码已 vendor 在 `vendor/rust-ffmpeg-sys/`（73 KB）。上游仓库有 3.5 GB，且逐文件拉 GitHub API 会撞 60 次/小时的限流，预置后构建不再依赖 GitHub。

## 32 位的真正瓶颈：x264 线程数

compositor 自身内存稳定在 410–430MB，与分辨率和时长无关。真正吃地址空间的是 `ffmpeg.exe` 里的 libx264——它按 CPU 数分配线程，每线程持有全分辨率帧缓冲：

| 分辨率 | threads | 峰值私有内存 | 结果 |
| --- | --- | --- | --- |
| 720p | auto | 532 MB | 通过 |
| 1080p | auto | 1470 MB | 通过 |
| 1080p | 8 | 718 MB | 通过（慢 1.8×） |
| 1440p | auto | 2998 MB | 通过但危险 |
| 4K | auto（本机 32 核→48 线程） | 3716 MB | **编码器 OOM，上层报 EPIPE** |
| 4K | 8 | 2596 MB | 通过 |
| 4K | 6 + veryfast | 1740 MB | 通过 |

所以注入脚本会给 `@remotion/renderer/dist/prespawn-ffmpeg.js` 打一个线程上限：≥4K 用 6 线程，≥1440p 用 8 线程，1080p 及以下不动。实测 4K 60 秒（1800 帧）峰值 2289 MB，稳定出片。

同时 `rust/memory.rs` 里把 32 位进程的「可用内存」钳到 1.5GB——原逻辑读的是宿主内存，在 32 位进程里会算出 20GB 的帧缓存上限。

## 用法

```sh
# 单一产物：config/runtime/win-ia32/compositor-win32-ia32
scripts/win-ia32-compositor/build.sh
```

CI 里由 `.github/workflows/desktop-supplements.yml` 的 `build-compositor` job 构建并缓存（按 `scripts/win-ia32-compositor/**` 哈希），产物作为 artifact 传给 win7 补充包 job；打包脚本用 `-RemotionCompositorPath` 接收，再由 `packaging/windows/scripts/remotion-ia32-compositor.ps1` 做注入与校验（PE 必须是 i386 且 LARGE_ADDRESS_AWARE，且不得有 Win10 导入）。

## 验收记录

同一台机器（Windows，32 核，32 位 node 16.20.2 + 32 位 compositor + 32 位 ffmpeg 7.1）：

| 用例 | 帧数 | 耗时 | ffmpeg 峰值 | 结果 |
| --- | --- | --- | --- | --- |
| 1080p / 10s | 300 | 32 s | — | h264 MP4 |
| 1080p / 60s | 1800 | 104 s | — | h264 MP4 |
| 1080p / 3min | 5400 | 243 s | — | h264 MP4 |
| 1080p / 10min | 18000 | 649 s | 1450 MB | h264 MP4 |
| 1440p / 60s | 1800 | 113 s | 1154 MB | h264 MP4 |
| 4K / 10s | 300 | 64 s | 2289 MB | h264 MP4 |
| 4K / 60s | 1800 | 249 s | 2289 MB | h264 MP4 |
| 1080p / 20s（含 Chrome 109 ia32 截图链路） | 600 | 30 s | — | 3.6 MB h264 MP4 |

## 浏览器：Chrome 109（ia32）

补充包自带 `opt/browser/chrome-win/chrome.exe`（109.0.5414.120，最后一个支持 Win7 的版本），Remotion 用 `--browser-executable` 指过去即可；`--headless=old`、`--headless=new`、有头模式三种都能出画。

踩过两个坑，都写进注入脚本了：

1. **`--force-gpu-mem-available-mb=4096`**。`@remotion/renderer/dist/open-browser.js` 无条件传这个参数，32 位进程总共只有 4GB 地址空间，合成器分配不到光栅缓冲，于是**每一帧都截成"只有 body 背景色、内容全空"的图**。页面 DOM、样式、console 全部正常，日志里没有任何报错，只能从产物体积看出来：1920×1080 的空帧是 36 KB 纯色 PNG，正常帧是 1.6–1.7 MB。注入脚本在 ia32 下把它改成不传（`process.arch === 'ia32' ? null : ...`）。
2. **`@esbuild/win32-ia32`**。在 x64 构建机上 `npm install` 不会装 ia32 平台包，`opt/remotion/node_modules/@esbuild/` 会是个空目录，于是 bundle 阶段直接报 `You installed esbuild for another platform`。打包脚本会显式校验并补装。

同一帧前后对比（Chrome 109 ia32，1920×1080）：

| 配置 | 产物 | 内容 |
| --- | --- | --- |
| 带 `--force-gpu-mem-available-mb=4096` | 36 KB | 全画面 `#111`，文字/渐变/布局全部缺失 |
| 不传该参数 | 1.67 MB | 正常出画（含中文） |

## 没有 GPU 的机器

Chrome 109 检测不到可用 GPU 时会自己回退到 Skia 软件光栅。Remotion 截的是 DOM 光栅化结果，纯 2D 合成本来就不依赖 GPU，所以**出片质量和有 GPU 时完全一致**——同一工程两版 mp4 的 md5 相同（`77d1ac0e738205cb8f473884acb7caf5`），代价只有时间：

| 渲染方式 | 600 帧 1080p / 20 s | 产物 |
| --- | --- | --- |
| 默认（GPU 可用） | 29.5 s | md5 `77d1ac0e…` |
| `--disable-gpu`（强制软件，等价于无 GPU 主机） | 60 s | 同上，字节一致 |
| `--gl=swangle`（强制软件 WebGL） | 120 帧就要 56 s（×4.3），全片约 4.5 min | 画面正常，但纯属浪费 |

- **不要为了"保险"加 `--gl=swangle`**：页面不用 WebGL，它只是把合成路径换成更慢的 ANGLE/SwiftShader，白掉 4 倍速度。
- 老机器估算：本机单并发约 0.2–0.25 s/帧（现代 CPU），600 帧 ≈ 2 min/并发；双核 Win7 老机按 CPU 差距放大，20 秒 1080p 落在 5–10 分钟属正常。
- 内存：仍在 32 位 4 GB 地址空间内，1080p 无压力；4K 受 x264 线程数限制（已打补丁）。
- 只有在老显卡驱动导致 GPU 进程反复崩溃时，才建议显式加 `--disable-gpu` 兜底。

## 注意

- 32 位进程 4 GB 地址空间（LARGE_ADDRESS_AWARE 已开）；8K 或超长 4K 没有验证过，建议分段渲染后拼接。
- Win7 依赖 Chrome 109（最后一个支持 Win7 的版本），已在补充包里；Remotion 4 与它的兼容性已实测。
- 升级 Remotion 必须重编：compositor 与 `@remotion/renderer` 版本强绑定。
- 32 位 Windows 主机需要 32 位 `node.exe`；包里默认的通用运行时是 x64，只能跑在 64 位 Windows 上。
