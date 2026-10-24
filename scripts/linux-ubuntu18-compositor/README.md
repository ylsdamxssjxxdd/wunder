# Ubuntu 18.04 Remotion compositor

Remotion 4.0.538 publishes an x86_64 Linux compositor that requires glibc
2.35, while the Linux desktop release baseline is Ubuntu 18.04 (glibc 2.27).
This directory rebuilds the compositor, FFmpeg 7.1 and x264 in the project's
existing `wunder-rust-builder-kylin-x86:ubuntu18` image.

```sh
bash scripts/linux-ubuntu18-compositor/build.sh
```

On Windows with Git Bash, set `WUNDER_DOCKER_BIN` to the Docker Desktop
executable if the shell resolves a WSL Docker client instead:

```sh
WUNDER_DOCKER_BIN='/c/Program Files/Docker/Docker/resources/bin/docker.exe' \
  bash scripts/linux-ubuntu18-compositor/build.sh
```

The result is written to
`config/runtime/linux-x64/compositor-linux-x64-gnu/`. It replaces the native
files in `@remotion/compositor-linux-x64-gnu` when the Linux amd64 supplement
is assembled. `package.json`, `index.js` and `index.mjs` remain from npm so
Remotion's package resolution is unchanged.

The build uses Remotion 4.0.538's `packages/compositor` source, FFmpeg 7.1,
x264, the vendored rust-ffmpeg crates and the pinned `mp4-rust` revision.
Cargo Git access is avoided: source fetched from codeload is patched locally.
`build.sh` verifies that the compositor does not require a glibc newer than
Ubuntu 18.04's glibc 2.27.

The output includes the compositor and its FFmpeg shared-library closure.
The renderer adds this directory to `LD_LIBRARY_PATH` so FFmpeg can start on
Ubuntu 18.04. The browser used for rendering must also be compatible with the
target host; pass a compatible browser with Remotion's `--browser-executable`
option when required (Ubuntu 18.04 ships Chromium 112).
