package dev.wunder.android;

import android.content.Context;
import android.content.pm.PackageInfo;

import java.io.BufferedWriter;
import java.io.File;
import java.io.FileWriter;
import java.io.PrintWriter;
import java.text.SimpleDateFormat;
import java.util.Arrays;
import java.util.Comparator;
import java.util.Date;
import java.util.Locale;

/**
 * Last-resort crash report for the Java side of the shell.
 *
 * <p>The Android runtime kills the process on an uncaught exception before the
 * Rust panic hook (and before the log buffer is flushed to logcat in many
 * builds), so a crash otherwise leaves no file behind. This handler writes one
 * timestamped report under {@code filesDir/wunder/logs} and then delegates to
 * the previous handler untouched. Every failure inside the writer is ignored:
 * a diagnostic must never change how the process dies.
 */
public final class CrashHandler implements Thread.UncaughtExceptionHandler {
    private static final String PREFIX = "crash-java-";
    private static final int KEEP_REPORTS = 10;
    private static final Object lock = new Object();
    private static volatile boolean installed;
    private static volatile Thread.UncaughtExceptionHandler previous;
    private static Context appContext;

    private CrashHandler() {}

    /** Install once per process; called from the activity's {@code onCreate}. */
    public static void install(Context context) {
        if (context == null) {
            return;
        }
        synchronized (lock) {
            if (installed) {
                return;
            }
            Context app = context.getApplicationContext();
            appContext = app == null ? context : app;
            previous = Thread.getDefaultUncaughtExceptionHandler();
            Thread.setDefaultUncaughtExceptionHandler(new CrashHandler());
            installed = true;
        }
    }

    @Override
    public void uncaughtException(Thread thread, Throwable error) {
        try {
            writeReport(thread, error);
        } catch (Throwable ignored) {
            // Never mask the original failure.
        }
        Thread.UncaughtExceptionHandler next = previous;
        if (next != null && next != this) {
            next.uncaughtException(thread, error);
            return;
        }
        android.os.Process.killProcess(android.os.Process.myPid());
        System.exit(10);
    }

    private static void writeReport(Thread thread, Throwable error) {
        Context context = appContext;
        if (context == null) {
            return;
        }
        File directory = new File(context.getFilesDir(), "wunder/logs");
        if (!directory.isDirectory() && !directory.mkdirs()) {
            return;
        }
        File report = new File(directory, PREFIX + System.currentTimeMillis() + ".log");
        try (PrintWriter writer = new PrintWriter(new BufferedWriter(new FileWriter(report, true)))) {
            writer.println("崩溃时间: "
                    + new SimpleDateFormat("yyyy-MM-dd HH:mm:ss.SSS Z", Locale.US).format(new Date()));
            writer.println("进程: wunder 安卓版 " + versionName(context));
            writer.println("线程: " + thread.getName());
            writer.println("说明: Java 未捕获异常。以下堆栈供事后定位。");
            error.printStackTrace(writer);
        } catch (Throwable ignored) {
            //noinspection ResultOfMethodCallIgnored
            report.delete();
            return;
        }
        prune(directory);
    }

    private static String versionName(Context context) {
        try {
            PackageInfo info = context.getPackageManager().getPackageInfo(context.getPackageName(), 0);
            return info.versionName == null ? "" : info.versionName;
        } catch (Throwable ignored) {
            return "";
        }
    }

    /** Keep the newest reports across Java and Rust crashes; names sort by time. */
    private static void prune(File directory) {
        File[] entries = directory.listFiles();
        if (entries == null) {
            return;
        }
        Arrays.sort(entries, Comparator.comparing(File::getName));
        int remaining = 0;
        for (int index = entries.length - 1; index >= 0; index--) {
            String name = entries[index].getName();
            if (!name.startsWith("crash-") || !name.endsWith(".log")) {
                continue;
            }
            remaining++;
            if (remaining > KEEP_REPORTS) {
                //noinspection ResultOfMethodCallIgnored
                entries[index].delete();
            }
        }
    }
}
