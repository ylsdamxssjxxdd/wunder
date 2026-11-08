package dev.wunder.android;

import android.content.Context;
import android.util.Log;

/**
 * JNI entry point of the {@code :background} process.
 *
 * <p>The scheduled wake must run without a window, without Slint and without the
 * UI engine, so it lives in its own process: {@code BackgroundRuntime.tick}
 * starts a headless native desktop, executes whatever scheduled job is due and
 * returns. The native side answers with a small JSON object
 * ({@code {"ok":true,"ran":N}} or {@code {"ok":false,"error":"..."}}); this
 * class never throws at the call site, because a wake that crashes the process
 * turns into a crash loop in the system's alarm statistics.
 */
public final class BackgroundRuntime {
    private static final String TAG = "wunder-bg";
    private static final int ERROR_MAX_CHARS = 200;

    /** {@code false} when the cdylib could not be mapped into this process. */
    private static final boolean LOADED;

    static {
        boolean loaded = false;
        try {
            System.loadLibrary("wunder_android");
            loaded = true;
        } catch (UnsatisfiedLinkError error) {
            // A missing or ABI-mismatched library must degrade the wake, not
            // kill the process the system just started for it.
            Log.e(TAG, "wunder_android library unavailable: " + error);
        }
        LOADED = loaded;
    }

    private BackgroundRuntime() {
    }

    /**
     * Native tick: start a headless runtime, drain due jobs, exit.
     *
     * <p>Call {@link #tickSafely(Context)} instead; this method throws
     * {@link UnsatisfiedLinkError} if the library failed to load and returns
     * {@code null} if the native side could not build a reply.
     */
    public static native String tick(Context context);

    /**
     * Crash-free wrapper around {@link #tick(Context)}. Always returns a JSON
     * object, even when the library is missing.
     */
    public static String tickSafely(Context context) {
        if (!LOADED) {
            return "{\"ok\":false,\"error\":\"native library unavailable\"}";
        }
        Context safe = context == null ? null : context.getApplicationContext();
        if (safe == null) {
            safe = context;
        }
        if (safe == null) {
            return "{\"ok\":false,\"error\":\"missing context\"}";
        }
        try {
            String reply = tick(safe);
            if (reply == null || reply.trim().isEmpty()) {
                return "{\"ok\":false,\"error\":\"native tick returned no result\"}";
            }
            return reply;
        } catch (Throwable error) {
            Log.e(TAG, "background tick failed", error);
            return "{\"ok\":false,\"error\":\"" + escape(error.toString()) + "\"}";
        }
    }

    /** JSON-safe, length-capped text for the error reply. */
    private static String escape(String value) {
        if (value == null) {
            return "unknown error";
        }
        StringBuilder out = new StringBuilder(Math.min(value.length(), ERROR_MAX_CHARS));
        for (int i = 0; i < value.length() && out.length() < ERROR_MAX_CHARS; i++) {
            char ch = value.charAt(i);
            switch (ch) {
                case '"':
                    out.append("\\\"");
                    break;
                case '\\':
                    out.append("\\\\");
                    break;
                case '\n':
                case '\r':
                case '\t':
                    out.append(' ');
                    break;
                default:
                    out.append(ch < 0x20 ? ' ' : ch);
                    break;
            }
        }
        return out.toString();
    }
}
