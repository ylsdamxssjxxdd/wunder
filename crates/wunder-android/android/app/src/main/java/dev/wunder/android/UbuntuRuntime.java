package dev.wunder.android;

import android.content.Context;
import android.content.res.AssetFileDescriptor;
import android.content.res.AssetManager;
import android.os.Build;
import android.os.StatFs;
import android.util.Log;

import java.io.BufferedReader;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.concurrent.TimeUnit;

/**
 * Bundled proot Ubuntu runtime for 安卓版.
 *
 * <p>The rootfs and the proot/busybox binaries ship inside the APK. The first
 * install copies them into the app's private directory, extracts the rootfs
 * with the static busybox, picks the proot flags that this kernel accepts and
 * publishes a {@code wunder-shell} wrapper. The wrapper accepts the same
 * {@code -lc <command>} contract as a POSIX shell, so the engine only has to
 * point {@code WUNDER_SHELL_BIN} at it.
 *
 * <p>All heavy work runs on a private background thread; the engine polls the
 * snapshot methods instead of blocking its own threads on the extraction.
 */
public final class UbuntuRuntime {
    private static final String TAG = "WunderUbuntu";
    private static final String ROOTFS_ARCHIVE = "rootfs.tar.xz";
    private static final String ROOTFS_ASSET = "ubuntu/" + ROOTFS_ARCHIVE;
    private static final String INSTALL_MARKER = ".wunder_installed_ok";
    /// Offline tool pack shipped next to the rootfs. It is extracted into the
    /// same tree, so python/git/ffmpeg need no network or package manager.
    private static final String PACK_ASSET = "ubuntu/packs/common.tar.xz";
    private static final String PACK_MARKER = ".wunder_pack_common";
    /// v6 adds the common runtime CLI set (curl/wget/zip/unzip, procps,
    /// psmisc, lsof, bc, bzip2, time, python alias, iproute2) on top of the
    /// v5 fonts; v7 adds scipy and the video-skill fonts; v8 adds the
    /// document/office fonts; v9 trims video fonts to Latin/mono; v10 adds
    /// back the Noto Serif/Sans SC variable fonts for 知识短片. Bumping the
    /// marker re-extracts the pack on devices with an older version.
    private static final String PACK_VERSION = "10";
    /// Browser overlay: Playwright plus an offline Chromium build. Release
    /// builds bundle it so browser-backed skills run out of the box; dev
    /// builds may omit it and skip the download/extraction cost.
    private static final String BROWSER_PACK_ASSET = "ubuntu/packs/browser.tar.xz";
    private static final String BROWSER_PACK_MARKER = ".wunder_pack_browser";
    /// v3 is the first browser pack bundled with release APKs; older markers
    /// re-extract it so devices pick up the browser runtime.
    private static final String BROWSER_PACK_VERSION = "3";
    private static final String WHEELS_DIR = "opt/wunder/wheels";
    /// Command name -> native library packaged in jniLibs. The binaries have to
    /// stay in the package-managed library directory: Android rejects execve on
    /// app-private files. Test builds add the Termux proot runtime and its
    /// shared libraries under the same scheme.
    private static final String[][] TOOL_LINKS = {
            {"proot", "liboperit_proot.so"},
            {"loader", "liboperit_loader.so"},
            {"busybox", "libbusybox.so"},
            {"libtalloc.so.2", "libwunder_talloc.so"},
            {"libandroid-shmem.so", "libwunder_shmem.so"},
    };

    private static final Object LOCK = new Object();
    private static Context app;
    private static volatile String state = "unknown";
    private static volatile int progress = 0;
    private static volatile String stage = "未安装";
    private static volatile String version = "";
    private static volatile String lastError = "";
    private static volatile boolean installing = false;

    private UbuntuRuntime() {}

    /** Bind the application context once per process and detect an existing install. */
    public static void bind(Context context) {
        app = context.getApplicationContext();
        if (!"installing".equals(state)) {
            detect();
        }
    }

    /** One-line snapshot consumed by the Rust side: state / progress / stage / version / error. */
    public static String snapshot() {
        return state
                + "\u001f" + progress
                + "\u001f" + stage
                + "\u001f" + version
                + "\u001f" + lastError;
    }

    public static String shellPath() {
        return app == null ? "" : wrapperFile().getAbsolutePath();
    }

    /// Host path of the proot python wrapper, or empty until the runtime is
    /// installed. The browser bridge is spawned by the app process, which has
    /// no host interpreter, so it launches this script through /system/bin/sh.
    public static String pythonPath() {
        if (app == null || !isInstalled()) {
            return "";
        }
        return pythonWrapperFile().getAbsolutePath();
    }

    /// True when the browser overlay is installed. `WUNDER_BROWSER_BIN` is
    /// only published in that case.
    public static boolean browserInstalled() {
        if (app == null || !isInstalled()) {
            return false;
        }
        try {
            return BROWSER_PACK_VERSION.equals(
                    readText(new File(rootfsDir(), BROWSER_PACK_MARKER)).trim());
        } catch (IOException error) {
            return false;
        }
    }

    /// Guest path of the bundled Chromium. proot binds the rootfs at `/`, so
    /// this is also the path the bridge sees after entering the wrapper.
    public static String browserExecutablePath() {
        return "/opt/browser/chrome-linux/chrome";
    }

    /** Start a background install. Safe to call repeatedly; only one install runs. */
    public static void install() {
        if (app == null) {
            return;
        }
        synchronized (LOCK) {
            if (installing) {
                return;
            }
            if (isInstalled() && packsInstalled() && browserPackInstalled()) {
                detect();
                return;
            }
            installing = true;
            state = "installing";
            progress = 0;
            lastError = "";
            stage = isInstalled() ? "准备安装常用工具…" : "准备安装 Ubuntu…";
        }
        Thread worker = new Thread(UbuntuRuntime::installBlocking, "wunder-ubuntu-install");
        worker.setDaemon(true);
        worker.start();
    }

    private static void detect() {
        if (app == null) {
            return;
        }
        try {
            if (isInstalled()) {
                if (version.isEmpty()) {
                    version = readVersion(rootfsDir());
                }
                // An app update installs it under a new /data/app path, so the
                // symlinks into nativeLibraryDir must be rebuilt on every
                // process start, not just on the first install.
                linkTools(new File(app.getFilesDir(), "usr/bin"));
                writeWrapper();
                if (!packsInstalled() || !browserPackInstalled()) {
                    // The rootfs survived an app update but a bundled pack was
                    // added or replaced; extract it before reporting ready.
                    state = "missing";
                    progress = 0;
                    stage = !packsInstalled() ? "待安装常用工具" : "待安装浏览器组件";
                    lastError = "";
                    return;
                }
                state = "installed";
                progress = 100;
                stage = "已安装 " + (version.isEmpty() ? "Ubuntu" : version);
                lastError = "";
                return;
            }
        } catch (Throwable error) {
            Log.w(TAG, "detect failed", error);
        }
        if (!"installing".equals(state)) {
            if (stagedRootfs().isFile()) {
                state = "missing";
                stage = "已检测到导入的 Ubuntu 根文件系统，可开始安装";
            } else if (!deviceCanRunBundledRuntime()) {
                state = "unsupported";
                stage = "内置 Ubuntu 仅支持 arm64 设备（当前 " + primaryAbi() + "）";
            } else {
                state = "missing";
                stage = "未安装";
            }
        }
    }

    private static void installBlocking() {
        File tmp = null;
        try {
            if (app == null) {
                throw new IOException("应用上下文不可用");
            }
            File files = app.getFilesDir();
            File rootfs = rootfsDir();
            if (isInstalled()) {
                // A previous version installed the base rootfs before an
                // offline pack existed. Only the missing pack extraction is left.
                setStage("准备安装补充包…", 4);
                boolean hasBrowser = hasAsset(BROWSER_PACK_ASSET);
                int toolsEnd = hasBrowser ? 88 : 98;
                installPacks(files, rootfs, 4, toolsEnd);
                installBrowserPack(files, rootfs, toolsEnd, 100);
                writeWrapper();
                if (version.isEmpty()) {
                    version = readVersion(rootfs);
                }
                progress = 100;
                stage = "已安装 " + (version.isEmpty() ? "Ubuntu" : version)
                        + (packsInstalled() ? " + 常用工具" : "")
                        + (browserPackInstalled() ? " + 浏览器" : "");
                state = "installed";
                lastError = "";
                return;
            }
            File stageDir = new File(files, "ubuntu-stage");
            File stagedArchive = new File(stageDir, ROOTFS_ARCHIVE);
            boolean staged = stagedArchive.isFile();
            if (!staged && !deviceCanRunBundledRuntime()) {
                throw new IOException("内置 Ubuntu 仅支持 arm64 设备（当前 " + primaryAbi() + "）");
            }
            long free = new StatFs(files.getAbsolutePath()).getAvailableBytes();
            long need = hasAsset(PACK_ASSET) ? 2000L * 1024 * 1024 : 400L * 1024 * 1024;
            if (hasAsset(BROWSER_PACK_ASSET)) {
                need += Math.max(assetLength(BROWSER_PACK_ASSET) * 3, 300L * 1024 * 1024);
            }
            if (free < need) {
                throw new IOException("存储空间不足，安装 Ubuntu 至少需要约 "
                        + (need / (1024 * 1024)) + " MB（当前可用 "
                        + (free / (1024 * 1024)) + " MB）");
            }

            setStage("准备运行组件…", 2);
            File bin = new File(files, "usr/bin");
            mkdirs(bin);
            mkdirs(new File(files, "tmp"));
            linkTools(bin);

            File archive = new File(files, "ubuntu-rootfs.tar.xz");
            long expected = staged ? stagedArchive.length() : assetLength(ROOTFS_ASSET);
            if (!archive.isFile() || (expected > 0 && archive.length() != expected)) {
                setStage("复制 Ubuntu 根文件系统…", 4);
                if (staged) {
                    copyFileWithProgress(stagedArchive, archive, expected);
                } else {
                    copyAssetWithProgress(app.getAssets(), ROOTFS_ASSET, archive, expected);
                }
            }

            File parent = rootfs.getParentFile();
            mkdirs(parent);
            tmp = new File(parent, "ubuntu.install.tmp");
            deleteRecursive(tmp);
            mkdirs(tmp);
            setStage("正在解压 Ubuntu 根文件系统…", 35);
            extract(bin, archive, tmp, files);
            flattenSingleRoot(tmp);

            setStage("配置 Ubuntu 环境…", 90);
            writeResolvConf(tmp);

            deleteRecursive(rootfs);
            if (!tmp.renameTo(rootfs)) {
                throw new IOException("无法落地根文件系统目录");
            }
            tmp = null;

            File flags = new File(files, "usr/proot.flags");
            setStage("验证 proot 启动…", 94);
            String prootFlags = chooseProotFlags(bin, rootfs, files);
            // The marker is the installed-state source of truth: write it only
            // after proot actually started, so a failed probe cannot leave a
            // broken tree that every later boot reports as ready.
            writeText(new File(rootfs, INSTALL_MARKER), "wunder ubuntu\n");
            writeText(flags, prootFlags + "\n");
            writeWrapper();
            // The compressed rootfs is only needed for the first install.
            // Keeping it would double the on-disk footprint forever.
            //noinspection ResultOfMethodCallIgnored
            archive.delete();

            version = readVersion(rootfs);
            progress = 85;
            stage = "Ubuntu 基础环境已就绪，正在安装常用工具…";
            boolean hasBrowser = hasAsset(BROWSER_PACK_ASSET);
            int toolsEnd = hasBrowser ? 94 : 99;
            installPacks(files, rootfs, 85, toolsEnd);
            if (hasBrowser) {
                stage = "正在安装浏览器组件…";
            }
            installBrowserPack(files, rootfs, toolsEnd, 100);
            writeWrapper();
            progress = 100;
            stage = "已安装 " + (version.isEmpty() ? "Ubuntu" : version)
                    + (packsInstalled() ? " + 常用工具" : "")
                    + (browserPackInstalled() ? " + 浏览器" : "");
            state = "installed";
            lastError = "";
            Log.i(TAG, "ubuntu runtime installed: " + version + " flags=" + prootFlags);
        } catch (Throwable error) {
            lastError = message(error);
            state = "error";
            progress = 0;
            stage = "安装失败：" + lastError;
            Log.e(TAG, "ubuntu install failed", error);
        } finally {
            if (tmp != null) {
                deleteRecursive(tmp);
            }
            synchronized (LOCK) {
                installing = false;
            }
        }
    }

    private static String chooseProotFlags(File bin, File rootfs, File files) throws IOException {
        List<String> base = new ArrayList<>(Arrays.asList(
                binProot().getAbsolutePath(),
                "-0",
                "-r", rootfs.getAbsolutePath(),
                "-w", "/",
                "/usr/bin/env", "-i",
                "HOME=/root",
                "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
                "/bin/bash", "--noprofile", "--norc", "-c",
                "grep -m1 '^PRETTY_NAME=' /etc/os-release"
        ));
        // apt/dpkg hard-link files across directories; --link2symlink makes
        // those calls work on Android's SELinux policy. Prefer it when the
        // kernel accepts it and fall back to plain proot otherwise.
        List<String> linked = new ArrayList<>();
        linked.add(binProot().getAbsolutePath());
        linked.add("-0");
        linked.add("-r");
        linked.add(rootfs.getAbsolutePath());
        linked.add("--link2symlink");
        linked.add("-w");
        linked.add("/");
        linked.add("/usr/bin/env");
        linked.add("-i");
        linked.add("HOME=/root");
        linked.add("PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin");
        linked.add("/bin/bash");
        linked.add("--noprofile");
        linked.add("--norc");
        linked.add("-c");
        linked.add("grep -m1 '^PRETTY_NAME=' /etc/os-release");
        RunResult link = runProot(linked, files, 120);
        if (link.exit == 0) {
            return "--link2symlink";
        }

        RunResult plain = runProot(base, files, 120);
        if (plain.exit == 0) {
            return "";
        }
        throw new IOException("proot 启动失败：" + tail(plain.output.isEmpty() ? link.output : plain.output, 300));
    }

    private static RunResult runProot(List<String> command, File files, long timeoutSeconds) {
        ProcessBuilder builder = new ProcessBuilder(command);
        builder.redirectErrorStream(true);
        builder.environment().put("PROOT_LOADER", new File(files, "usr/bin/loader").getAbsolutePath());
        builder.environment().put("PROOT_TMP_DIR", new File(files, "tmp").getAbsolutePath());
        File testLibs = new File(files, "usr/bin/libtalloc.so.2");
        if (testLibs.isFile()) {
            builder.environment().put("LD_LIBRARY_PATH", testLibs.getParentFile().getAbsolutePath());
        }
        StringBuilder output = new StringBuilder();
        Process process = null;
        try {
            process = builder.start();
            final Process running = process;
            Thread drain = new Thread(() -> {
                try (BufferedReader reader = new BufferedReader(
                        new InputStreamReader(running.getInputStream(), StandardCharsets.UTF_8))) {
                    String line;
                    while ((line = reader.readLine()) != null) {
                        synchronized (output) {
                            if (output.length() < 8192) {
                                output.append(line).append('\n');
                            }
                        }
                    }
                } catch (IOException ignored) {
                    // The process may be killed by the timeout; the exit code
                    // and whatever was captured already are enough to report.
                }
            }, "wunder-proot-output");
            drain.setDaemon(true);
            drain.start();
            if (!process.waitFor(timeoutSeconds, TimeUnit.SECONDS)) {
                process.destroyForcibly();
                process.waitFor(3, TimeUnit.SECONDS);
                synchronized (output) {
                    output.append("proot 启动超时");
                }
            }
            synchronized (output) {
                return new RunResult(process.exitValue(), output.toString());
            }
        } catch (Exception error) {
            synchronized (output) {
                output.append(message(error));
            }
            return new RunResult(-1, output.toString());
        } finally {
            if (process != null) {
                process.destroy();
            }
        }
    }

    private static void extract(File bin, File archive, File target, File files) throws IOException {
        // Android SELinux denies link() on app-private files, and a distro
        // rootfs contains hard links (perl, dpkg). Running the extractor
        // through proot with --link2symlink turns those calls into symlinks
        // instead of failing halfway through.
        List<String> command = new ArrayList<>(Arrays.asList(
                new File(bin, "proot").getAbsolutePath(),
                "-0", "-r", "/", "--link2symlink", "-w", "/",
                new File(bin, "busybox").getAbsolutePath(),
                "tar", "xvf", archive.getAbsolutePath(),
                "-C", target.getAbsolutePath()));
        ProcessBuilder builder = new ProcessBuilder(command);
        builder.environment().put("PROOT_LOADER", new File(bin, "loader").getAbsolutePath());
        builder.environment().put("PROOT_TMP_DIR", new File(files, "tmp").getAbsolutePath());
        if (new File(bin, "libtalloc.so.2").isFile()) {
            builder.environment().put("LD_LIBRARY_PATH", bin.getAbsolutePath());
        }
        builder.redirectErrorStream(true);
        Process process = builder.start();
        int count = 0;
        StringBuilder tail = new StringBuilder();
        try (BufferedReader reader = new BufferedReader(
                new InputStreamReader(process.getInputStream(), StandardCharsets.UTF_8))) {
            String line;
            while ((line = reader.readLine()) != null) {
                count += 1;
                if (line.startsWith("tar:")) {
                    tail.append(line).append('\n');
                    if (tail.length() > 600) {
                        tail.delete(0, tail.length() - 600);
                    }
                }
                if ((count & 127) == 0) {
                    int pct = Math.min(88, 35 + count / 160);
                    setStage("正在解压 Ubuntu 根文件系统…（" + count + " 项）", pct);
                }
            }
        }
        int exit;
        try {
            exit = process.waitFor();
        } catch (InterruptedException error) {
            Thread.currentThread().interrupt();
            throw new IOException("解压被中断");
        }
        if (exit != 0) {
            String detail = tail.length() == 0 ? "" : "：" + tail.toString().trim();
            throw new IOException("解压失败（退出码 " + exit + "）" + detail);
        }
        if (count == 0) {
            throw new IOException("根文件系统压缩包为空");
        }
    }

    /**
     * Extract the bundled offline tool pack over the installed rootfs. The
     * pack is a plain overlay (python/git/rg/ffmpeg/7zip and their libraries),
     * so the guest PATH needs no extra entries and no package manager runs.
     */
    private static void installPacks(File files, File rootfs, int basePct, int spanPct)
            throws IOException {
        if (packsInstalled()) {
            return;
        }
        if (!hasAsset(PACK_ASSET)) {
            // Dev builds without the pack must not re-enter the installer on
            // every launch; the marker is invalidated as soon as a later build
            // ships the asset again.
            writeText(new File(rootfs, PACK_MARKER), "absent\n");
            Log.w(TAG, "offline tool pack missing; runtime starts without common tools");
            return;
        }
        long expected = assetLength(PACK_ASSET);
        long free = new StatFs(files.getAbsolutePath()).getAvailableBytes();
        if (expected > 0 && free < expected * 7 + 256L * 1024 * 1024) {
            throw new IOException("存储空间不足，安装常用工具至少需要约 "
                    + ((expected * 7 + 256L * 1024 * 1024) / (1024 * 1024)) + " MB");
        }
        File archives = new File(files, "ubuntu-packs");
        mkdirs(archives);
        File archive = new File(archives, "common.tar.xz");
        int span = spanPct - basePct;
        int copyEnd = basePct + Math.max(2, span / 6);
        int extractBase = basePct + Math.max(3, span / 5);
        int extractEnd = basePct + Math.max(span * 2 / 3, extractBase - basePct);
        try {
            setStage("写入常用工具包…", basePct);
            copyAsset(app.getAssets(), PACK_ASSET, archive, expected,
                    basePct, copyEnd, "写入常用工具包…");
            setStage("正在安装常用工具…", extractBase);
            extractPack(new File(files, "usr/bin"), archive, rootfs, files,
                    extractBase, extractEnd, "常用工具");
            installPythonWheels(files, rootfs, extractEnd, Math.max(extractEnd, spanPct - 1));
            writeText(new File(rootfs, PACK_MARKER), PACK_VERSION + "\n");
            setStage("常用工具与 Python 库已就绪", spanPct);
            Log.i(TAG, "offline packs installed: python/git/rg/ffmpeg/7zip + fonts + runtime CLI tools + python libraries");
        } finally {
            //noinspection ResultOfMethodCallIgnored
            archive.delete();
            deleteRecursive(archives);
        }
    }

    /**
     * Install the bundled wheelhouse without a network. The pack carries the
     * wheels and a pinned requirements file; pip only reads local files, so the
     * first launch stays fully offline.
     */
    private static void installPythonWheels(File files, File rootfs, int basePct, int spanPct)
            throws IOException {
        File wheels = new File(rootfs, WHEELS_DIR);
        File requirements = new File(wheels, "requirements-android.txt");
        if (!requirements.isFile()) {
            Log.w(TAG, "python wheelhouse missing; skipping library install");
            return;
        }
        setStage("正在安装 Python 库…", basePct);
        List<String> command = new ArrayList<>();
        command.add(binProot().getAbsolutePath());
        command.add("-0");
        command.add("-r");
        command.add(rootfs.getAbsolutePath());
        File flagsFile = new File(files, "usr/proot.flags");
        String flags = flagsFile.isFile() ? readText(flagsFile).trim() : "";
        if (!flags.isEmpty()) {
            command.add(flags);
        }
        command.add("-w");
        command.add("/");
        command.add("/usr/bin/env");
        command.add("-i");
        command.add("HOME=/root");
        command.add("PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin");
        command.add("/bin/bash");
        command.add("-lc");
        // apt-installed distributions carry no RECORD file, so any upgrade of
        // one of them fails at the uninstall step ("Cannot uninstall ...,
        // RECORD file not found"). --ignore-installed skips uninstalls and
        // installs the wheelhouse into /usr/local, which shadows /usr/lib.
        command.add("python3 -m pip install --break-system-packages --ignore-installed "
                + "--no-cache-dir --disable-pip-version-check --no-index "
                + "--no-compile --no-deps --progress-bar off "
                + "--find-links /opt/wunder/wheels "
                + "-r /opt/wunder/wheels/requirements-android.txt");
        ProcessBuilder builder = new ProcessBuilder(command);
        builder.environment().put("PROOT_LOADER", new File(files, "usr/bin/loader").getAbsolutePath());
        builder.environment().put("PROOT_TMP_DIR", new File(files, "tmp").getAbsolutePath());
        File libtalloc = new File(files, "usr/bin/libtalloc.so.2");
        if (libtalloc.isFile()) {
            builder.environment().put("LD_LIBRARY_PATH", libtalloc.getParentFile().getAbsolutePath());
        }
        builder.redirectErrorStream(true);
        Process process = builder.start();
        // pip can be silent for long stretches while it unpacks the large
        // binary wheels under proot. Keep the stage text and progress alive so
        // the first-run screen does not look frozen at 93%.
        final long startedAt = System.nanoTime();
        final java.util.concurrent.atomic.AtomicBoolean finished =
                new java.util.concurrent.atomic.AtomicBoolean(false);
        Thread heartbeat = new Thread(() -> {
            while (!finished.get()) {
                try {
                    Thread.sleep(2000);
                } catch (InterruptedException error) {
                    return;
                }
                if (finished.get()) {
                    return;
                }
                long seconds = (System.nanoTime() - startedAt) / 1_000_000_000L;
                setStage("正在安装 Python 库…（已 " + seconds + " 秒）",
                        Math.min(spanPct, basePct + (int) (seconds / 10)));
            }
        }, "wunder-python-install-heartbeat");
        heartbeat.setDaemon(true);
        heartbeat.start();
        int count = 0;
        StringBuilder tail = new StringBuilder();
        try {
            try (BufferedReader reader = new BufferedReader(
                    new InputStreamReader(process.getInputStream(), StandardCharsets.UTF_8))) {
                String line;
                while ((line = reader.readLine()) != null) {
                    count += 1;
                    if (line.startsWith("ERROR") || line.contains("Successfully installed")) {
                        tail.append(line).append('\n');
                        if (tail.length() > 800) {
                            tail.delete(0, tail.length() - 800);
                        }
                    }
                    if ((count & 15) == 0) {
                        setStage("正在安装 Python 库…（" + count + " 行）",
                                Math.min(spanPct, basePct + count / 20));
                    }
                }
            }
            int exit;
            try {
                if (!process.waitFor(1800, TimeUnit.SECONDS)) {
                    process.destroyForcibly();
                    throw new IOException("安装 Python 库超时");
                }
                exit = process.exitValue();
            } catch (InterruptedException error) {
                Thread.currentThread().interrupt();
                throw new IOException("安装 Python 库被中断");
            }
            if (exit != 0) {
                String detail = tail.length() == 0 ? "" : "：" + tail.toString().trim();
                throw new IOException("安装 Python 库失败（退出码 " + exit + "）" + detail);
            }
        } finally {
            finished.set(true);
            heartbeat.interrupt();
        }
        Log.i(TAG, "python wheelhouse installed from local wheels");
    }

    private static void extractPack(File bin, File archive, File rootfs, File files,
                                    int basePct, int spanPct, String label) throws IOException {
        List<String> command = new ArrayList<>(Arrays.asList(
                new File(bin, "proot").getAbsolutePath(),
                "-0", "-r", "/", "--link2symlink", "-w", "/",
                new File(bin, "busybox").getAbsolutePath(),
                "tar", "xvf", archive.getAbsolutePath(),
                "-C", rootfs.getAbsolutePath()));
        ProcessBuilder builder = new ProcessBuilder(command);
        builder.environment().put("PROOT_LOADER", new File(bin, "loader").getAbsolutePath());
        builder.environment().put("PROOT_TMP_DIR", new File(files, "tmp").getAbsolutePath());
        if (new File(bin, "libtalloc.so.2").isFile()) {
            builder.environment().put("LD_LIBRARY_PATH", bin.getAbsolutePath());
        }
        builder.redirectErrorStream(true);
        Process process = builder.start();
        int count = 0;
        StringBuilder tail = new StringBuilder();
        try (BufferedReader reader = new BufferedReader(
                new InputStreamReader(process.getInputStream(), StandardCharsets.UTF_8))) {
            String line;
            while ((line = reader.readLine()) != null) {
                count += 1;
                if (line.startsWith("tar:")) {
                    tail.append(line).append('\n');
                    if (tail.length() > 600) {
                        tail.delete(0, tail.length() - 600);
                    }
                }
                if ((count & 255) == 0) {
                    int pct = Math.min(spanPct, basePct + count / 400);
                    setStage("正在安装" + label + "…（" + count + " 项）", pct);
                }
            }
        }
        int exit;
        try {
            exit = process.waitFor();
        } catch (InterruptedException error) {
            Thread.currentThread().interrupt();
            throw new IOException("安装" + label + "被中断");
        }
        if (exit != 0) {
            String detail = tail.length() == 0 ? "" : "：" + tail.toString().trim();
            throw new IOException("安装" + label + "失败（退出码 " + exit + "）" + detail);
        }
        if (count == 0) {
            throw new IOException(label + "包为空");
        }
    }

    /**
     * Browser overlay: Playwright wheels and the offline Chromium tree live
     * under `/opt/browser` (and the Playwright driver under
     * /usr/local/lib/python3.12/dist-packages). It is a plain tar overlay just
     * like the common pack, so no package manager runs on the device. Release
     * APKs bundle it for browser-backed skills such as 知识短片.
     */
    private static void installBrowserPack(File files, File rootfs, int basePct, int spanPct)
            throws IOException {
        if (browserPackInstalled()) {
            setStage("浏览器组件已就绪", spanPct);
            return;
        }
        if (!hasAsset(BROWSER_PACK_ASSET) || !hasAsset(PACK_ASSET)) {
            writeText(new File(rootfs, BROWSER_PACK_MARKER), "absent\n");
            Log.w(TAG, "browser pack or its common base missing; browser tools and browser-backed skills stay unavailable");
            return;
        }
        long expected = assetLength(BROWSER_PACK_ASSET);
        long free = new StatFs(files.getAbsolutePath()).getAvailableBytes();
        if (expected > 0 && free < expected * 4 + 128L * 1024 * 1024) {
            throw new IOException("存储空间不足，安装浏览器组件至少需要约 "
                    + ((expected * 4 + 128L * 1024 * 1024) / (1024 * 1024)) + " MB");
        }
        File archives = new File(files, "ubuntu-packs");
        mkdirs(archives);
        File archive = new File(archives, "browser.tar.xz");
        int copyEnd = basePct + Math.max(2, (spanPct - basePct) / 5);
        int extractBase = basePct + Math.max(3, (spanPct - basePct) / 4);
        int extractEnd = Math.max(extractBase, spanPct - 1);
        try {
            setStage("写入浏览器组件包…", basePct);
            copyAsset(app.getAssets(), BROWSER_PACK_ASSET, archive, expected,
                    basePct, copyEnd, "写入浏览器组件包…");
            setStage("正在安装浏览器组件…", extractBase);
            extractPack(new File(files, "usr/bin"), archive, rootfs, files,
                    extractBase, extractEnd, "浏览器组件");
            writeText(new File(rootfs, BROWSER_PACK_MARKER), BROWSER_PACK_VERSION + "\n");
            setStage("浏览器组件已就绪", spanPct);
            Log.i(TAG, "offline browser pack installed");
        } finally {
            //noinspection ResultOfMethodCallIgnored
            archive.delete();
        }
    }

    private static boolean browserPackInstalled() {
        if (app == null) {
            return false;
        }
        File marker = new File(rootfsDir(), BROWSER_PACK_MARKER);
        if (!marker.isFile()) {
            return false;
        }
        try {
            String text = readText(marker).trim();
            if (BROWSER_PACK_VERSION.equals(text)) {
                return true;
            }
            return "absent".equals(text)
                    && (!hasAsset(BROWSER_PACK_ASSET) || !hasAsset(PACK_ASSET));
        } catch (IOException error) {
            return false;
        }
    }

    private static boolean packsInstalled() {
        if (app == null) {
            return false;
        }
        File marker = new File(rootfsDir(), PACK_MARKER);
        if (!marker.isFile()) {
            return false;
        }
        try {
            String text = readText(new File(rootfsDir(), PACK_MARKER)).trim();
            if (PACK_VERSION.equals(text)) {
                return true;
            }
            return "absent".equals(text) && !hasAsset(PACK_ASSET);
        } catch (IOException error) {
            return false;
        }
    }

    private static boolean hasAsset(String name) {
        if (app == null) {
            return false;
        }
        try (InputStream ignored = app.getAssets().open(name)) {
            return true;
        } catch (IOException error) {
            return false;
        }
    }

    private static void copyAsset(AssetManager assets, String name, File dest, long total,
                                  int basePct, int spanPct, String label) throws IOException {
        try (InputStream input = assets.open(name);
             OutputStream output = new FileOutputStream(dest)) {
            byte[] buffer = new byte[64 * 1024];
            long copied = 0;
            int step = 0;
            int read;
            while ((read = input.read(buffer)) != -1) {
                output.write(buffer, 0, read);
                copied += read;
                if ((step++ & 7) == 0) {
                    int pct = total > 0
                            ? basePct + (int) ((spanPct - basePct) * copied / total)
                            : basePct;
                    setStage(label + " " + (copied / (1024 * 1024)) + " MB", pct);
                }
            }
            output.flush();
        }
    }

    /** Archives that wrap everything in one top directory are common; expose its children. */
    private static void flattenSingleRoot(File dir) throws IOException {
        File[] entries = dir.listFiles();
        if (entries == null || entries.length != 1 || !entries[0].isDirectory()) {
            return;
        }
        File only = entries[0];
        File[] children = only.listFiles();
        if (children == null || children.length == 0) {
            return;
        }
        for (File child : children) {
            File dest = new File(dir, child.getName());
            if (dest.exists()) {
                throw new IOException("根文件系统目录结构异常：" + dest.getName());
            }
            if (!child.renameTo(dest)) {
                throw new IOException("无法整理根文件系统目录");
            }
        }
        //noinspection ResultOfMethodCallIgnored
        only.delete();
    }

    /// Android denies reads of several host /proc entries (version, stat,
    /// loadavg, uptime) even after they are bind-mounted into proot. Provide
    /// small stable values instead so probing tools do not fail outright.
    private static void writeFakeSysdata(File rootfs) throws IOException {
        File proc = new File(rootfs, "proc");
        mkdirs(proc);
        writeTextIfAbsent(new File(proc, ".version"),
                "Linux version 6.6.0-proot (wunder@android) #1 SMP PREEMPT\n");
        writeTextIfAbsent(new File(proc, ".stat"),
                "cpu  1234 0 567 8901 0 0 0 0 0 0\n"
                        + "cpu0 1234 0 567 8901 0 0 0 0 0 0\n"
                        + "intr 0\nctxt 0\nbtime 1700000000\nprocesses 1\n"
                        + "procs_running 1\nprocs_blocked 0\nsoftirq 0 0 0 0 0 0 0 0 0 0 0\n");
        writeTextIfAbsent(new File(proc, ".loadavg"), "0.00 0.00 0.00 1/100 1\n");
        writeTextIfAbsent(new File(proc, ".uptime"), "100.00 100.00\n");

        // Emulator and some mobile networks advertise IPv6 without a working
        // route; apt otherwise waits on connect timeouts for every mirror.
        File aptConf = new File(rootfs, "etc/apt/apt.conf.d");
        mkdirs(aptConf);
        writeTextIfAbsent(new File(aptConf, "99wunder-android"),
                "Acquire::ForceIPv4 \"true\";\n"
                        + "Acquire::Retries \"2\";\n"
                        + "Acquire::http::Timeout \"25\";\n"
                        + "Acquire::https::Timeout \"25\";\n");
    }

    private static void writeTextIfAbsent(File file, String text) throws IOException {
        if (!file.isFile()) {
            writeText(file, text);
        }
    }

    private static void writeResolvConf(File rootfs) throws IOException {
        File etc = new File(rootfs, "etc");
        mkdirs(etc);
        writeText(new File(etc, "resolv.conf"),
                "nameserver 223.5.5.5\n"
                        + "nameserver 119.29.29.29\n"
                        + "nameserver 8.8.8.8\n"
                        + "nameserver 1.1.1.1\n");
    }

    private static void writeWrapper() throws IOException {
        if (app == null || !isInstalled()) {
            return;
        }
        File files = app.getFilesDir();
        File wrapper = wrapperFile();
        File pythonWrapper = pythonWrapperFile();
        File prootFlags = new File(files, "usr/proot.flags");
        String flags = "";
        if (prootFlags.isFile()) {
            flags = readText(prootFlags).trim();
        }
        writeFakeSysdata(rootfsDir());
        String header = "#!/system/bin/sh\n"
                + "BIN=\"" + new File(files, "usr/bin").getAbsolutePath() + "\"\n"
                + "ROOTFS=\"" + rootfsDir().getAbsolutePath() + "\"\n"
                + "FILES=\"" + files.getAbsolutePath() + "\"\n"
                + "TMP=\"" + new File(files, "tmp").getAbsolutePath() + "\"\n"
                + "FLAGS=\"" + flags + "\"\n"
                + "mkdir -p \"$TMP\" 2>/dev/null\n"
                + "# Android has no /dev/shm; give the guest a private directory so\n"
                + "# Python multiprocessing/shared_memory can create POSIX semaphores.\n"
                + "mkdir -p \"$TMP/shm\" 2>/dev/null\n"
                + "chmod 1777 \"$TMP/shm\" 2>/dev/null\n"
                + "# Mirror the phone timezone; the rootfs /etc/localtime stays UTC.\n"
                + "TZ=$(/system/bin/getprop persist.sys.timezone 2>/dev/null)\n"
                + "[ -n \"$TZ\" ] || TZ=UTC\n"
                + "export PROOT_LOADER=\"$BIN/loader\"\n"
                + "export PROOT_TMP_DIR=\"$TMP\"\n"
                + "# Test builds stage proot as a dynamically linked Termux binary; its\n"
                + "# talloc/shm libraries live next to it. The shipped static build has\n"
                + "# neither file and keeps the linker environment untouched.\n"
                + "if [ -e \"$BIN/libtalloc.so.2\" ]; then export LD_LIBRARY_PATH=\"$BIN\"; fi\n"
                + "BINDS=\"\"\n"
                + "for p in /dev /proc /sys /data/local/tmp /storage/emulated /sdcard /data/data \"$FILES\"; do\n"
                + "  if [ -e \"$p\" ]; then BINDS=\"$BINDS -b $p\"; fi\n"
                + "done\n";
        String proot = "exec \"$BIN/proot\" -0 -r \"$ROOTFS\" $FLAGS $BINDS \\\n"
                + "  -b \"$TMP/shm:/dev/shm\" \\\n"
                + "  -b \"$ROOTFS/proc/.version:/proc/version\" \\\n"
                + "  -b \"$ROOTFS/proc/.stat:/proc/stat\" \\\n"
                + "  -b \"$ROOTFS/proc/.loadavg:/proc/loadavg\" \\\n"
                + "  -b \"$ROOTFS/proc/.uptime:/proc/uptime\" \\\n"
                + "  -w ";

        String shellScript = header
                + "# Generated by 安卓版: run a command inside the bundled Ubuntu rootfs.\n"
                + "CMD=\"$2\"\n"
                + "if [ \"$1\" != \"-lc\" ]; then CMD=\"$*\"; fi\n"
                + "# Command::current_dir changes the real cwd without updating the\n"
                + "# inherited PWD variable, so resolve the physical path instead.\n"
                + "HOST_WD=$(pwd -P 2>/dev/null || pwd)\n"
                + "GUEST_WD=\"/root\"\n"
                + "case \"$HOST_WD\" in\n"
                + "  \"$FILES\"*|/data/data/*|/storage/*|/sdcard*|/data/local/tmp*) GUEST_WD=\"$HOST_WD\" ;;\n"
                + "esac\n"
                + proot + "\"$GUEST_WD\" \\\n"
                + "  /usr/bin/env -i HOME=/root TERM=dumb LANG=C.UTF-8 \\\n"
                + "  TZ=\"$TZ\" \\\n"
                + "  PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \\\n"
                + "  WUNDER_UBUNTU=1 \\\n"
                + "  /bin/bash -lc \"$CMD\"\n";

        // The browser bridge is spawned by the Android host, where no python
        // exists. This wrapper is read by the system shell (execve on private
        // files is denied) and runs the rootfs interpreter; the bridge script
        // itself is reachable because `$FILES` is bound into proot.
        String pythonScript = header
                + "# Generated by 安卓版: run the bundled Ubuntu python inside proot.\n"
                + proot + "\"/root\" \\\n"
                + "  /usr/bin/env -i HOME=/root TERM=dumb LANG=C.UTF-8 \\\n"
                + "  TZ=\"$TZ\" \\\n"
                + "  PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \\\n"
                + "  WUNDER_UBUNTU=1 \\\n"
                + "  WUNDER_BROWSER_BIN=\"$WUNDER_BROWSER_BIN\" \\\n"
                + "  PLAYWRIGHT_BROWSERS_PATH=\"$PLAYWRIGHT_BROWSERS_PATH\" \\\n"
                + "  PYTHONIOENCODING=utf-8 \\\n"
                + "  /usr/bin/python3 \"$@\"\n";

        mkdirs(wrapper.getParentFile());
        writeText(wrapper, shellScript);
        wrapper.setReadable(true, false);
        wrapper.setExecutable(true, false);
        writeText(pythonWrapper, pythonScript);
        pythonWrapper.setReadable(true, false);
        pythonWrapper.setExecutable(true, false);
    }

    private static void linkTools(File bin) throws IOException {
        String nativeDir = app.getApplicationInfo().nativeLibraryDir;
        for (String[] tool : TOOL_LINKS) {
            File source = new File(nativeDir, tool[1]);
            if (!source.isFile()) {
                continue;
            }
            File link = new File(bin, tool[0]);
            try {
                Files.deleteIfExists(link.toPath());
                Files.createSymbolicLink(link.toPath(), source.toPath());
            } catch (Exception error) {
                throw new IOException("无法链接运行组件 " + tool[0], error);
            }
        }
        for (String required : new String[]{"proot", "loader", "busybox"}) {
            if (!new File(bin, required).exists()) {
                throw new IOException("安装包缺少运行组件：" + required);
            }
        }
    }

    private static boolean deviceCanRunBundledRuntime() {
        if (app == null) {
            return false;
        }
        File proot = new File(app.getApplicationInfo().nativeLibraryDir, "liboperit_proot.so");
        if (!proot.isFile()) {
            return false;
        }
        // The bundled rootfs is arm64; an emulator/test tree may stage a
        // matching rootfs for another ABI instead.
        return primaryAbi().startsWith("arm64") || stagedRootfs().isFile();
    }

    private static String primaryAbi() {
        String[] abis = Build.SUPPORTED_ABIS;
        return abis != null && abis.length > 0 ? abis[0] : "";
    }

    private static boolean isInstalled() {
        return app != null && new File(rootfsDir(), INSTALL_MARKER).isFile();
    }

    private static File stagedRootfs() {
        return new File(new File(app.getFilesDir(), "ubuntu-stage"), ROOTFS_ARCHIVE);
    }

    private static File rootfsDir() {
        return new File(app.getFilesDir(), "usr/var/lib/proot-distro/installed-rootfs/ubuntu");
    }

    private static File wrapperFile() {
        return new File(app.getFilesDir(), "bin/wunder-shell");
    }

    private static File pythonWrapperFile() {
        return new File(app.getFilesDir(), "bin/wunder-python");
    }

    private static File binProot() {
        return new File(app.getFilesDir(), "usr/bin/proot");
    }

    private static String readVersion(File rootfs) {
        try {
            String release = readText(new File(rootfs, "etc/os-release"));
            for (String line : release.split("\n")) {
                if (line.startsWith("PRETTY_NAME=")) {
                    return line.substring("PRETTY_NAME=".length()).trim().replace("\"", "");
                }
            }
        } catch (IOException ignored) {
            // Fall through to issue.net, then to a generic label.
        }
        try {
            String issue = readText(new File(rootfs, "etc/issue.net")).trim();
            if (!issue.isEmpty()) {
                return issue;
            }
        } catch (IOException ignored) {
            // The probe already proved the tree is usable; the label is cosmetic.
        }
        return "";
    }

    private static void setStage(String text, int pct) {
        int clamped = Math.max(0, Math.min(100, pct));
        stage = text;
        progress = clamped;
        Log.i(TAG, clamped + "% " + text);
    }

    private static long assetLength(String name) {
        if (app == null) {
            return 0;
        }
        try (AssetFileDescriptor fd = app.getAssets().openFd(name)) {
            return fd.getLength();
        } catch (IOException error) {
            // Compressed assets have no exposed length; the installer then
            // reports byte counts without a percentage during the copy.
            return 0;
        }
    }

    private static void copyAssetWithProgress(AssetManager assets, String name, File dest, long total)
            throws IOException {
        try (InputStream input = assets.open(name);
             OutputStream output = new FileOutputStream(dest)) {
            copyStream(input, output, total, 4, 30);
        }
    }

    private static void copyFileWithProgress(File source, File dest, long total) throws IOException {
        try (InputStream input = new FileInputStream(source);
             OutputStream output = new FileOutputStream(dest)) {
            copyStream(input, output, total, 4, 30);
        }
    }

    private static void copyStream(InputStream input, OutputStream output,
                                   long total, int basePct, int spanPct) throws IOException {
        byte[] buffer = new byte[64 * 1024];
        long copied = 0;
        int step = 0;
        int read;
        while ((read = input.read(buffer)) != -1) {
            output.write(buffer, 0, read);
            copied += read;
            if (total > 0 && (step++ & 3) == 0) {
                int pct = basePct + (int) ((spanPct - basePct) * copied / total);
                String label = spanPct >= 30 ? "复制 Ubuntu 根文件系统… " : "写入运行组件… ";
                setStage(label + (copied / (1024 * 1024)) + " MB", pct);
            }
        }
        output.flush();
    }

    private static String readText(File file) throws IOException {
        try (FileInputStream input = new FileInputStream(file)) {
            byte[] data = new byte[(int) Math.min(file.length(), 64 * 1024)];
            int offset = 0;
            int read;
            while (offset < data.length && (read = input.read(data, offset, data.length - offset)) > 0) {
                offset += read;
            }
            return new String(data, 0, offset, StandardCharsets.UTF_8);
        }
    }

    private static void writeText(File file, String text) throws IOException {
        File parent = file.getParentFile();
        if (parent != null) {
            mkdirs(parent);
        }
        try (FileOutputStream output = new FileOutputStream(file)) {
            output.write(text.getBytes(StandardCharsets.UTF_8));
        }
    }

    private static void mkdirs(File dir) throws IOException {
        if (!dir.isDirectory() && !dir.mkdirs() && !dir.isDirectory()) {
            throw new IOException("无法创建目录：" + dir.getAbsolutePath());
        }
    }

    private static void deleteRecursive(File file) {
        if (file == null || !file.exists()) {
            return;
        }
        File[] children = file.listFiles();
        if (children != null) {
            for (File child : children) {
                deleteRecursive(child);
            }
        }
        //noinspection ResultOfMethodCallIgnored
        file.delete();
    }

    private static String tail(String text, int max) {
        String trimmed = text.trim();
        if (trimmed.length() <= max) {
            return trimmed;
        }
        return trimmed.substring(trimmed.length() - max);
    }

    private static String message(Throwable error) {
        String text = error.getMessage();
        return text == null || text.isEmpty() ? error.getClass().getSimpleName() : text;
    }

    private static final class RunResult {
        final int exit;
        final String output;

        RunResult(int exit, String output) {
            this.exit = exit;
            this.output = output;
        }
    }
}
