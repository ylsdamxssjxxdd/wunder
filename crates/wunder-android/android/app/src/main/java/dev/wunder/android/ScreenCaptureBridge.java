package dev.wunder.android;

import android.app.Activity;
import android.content.Context;
import android.content.Intent;
import android.graphics.Bitmap;
import android.graphics.Rect;
import android.media.projection.MediaProjection;
import android.media.projection.MediaProjectionManager;
import android.os.Handler;
import android.os.Looper;
import android.os.SystemClock;
import android.util.DisplayMetrics;
import android.util.Log;
import android.view.WindowManager;

import org.json.JSONObject;

import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.lang.ref.WeakReference;
import java.lang.reflect.Field;
import java.util.Arrays;
import java.util.Comparator;
import java.util.concurrent.Callable;
import java.util.concurrent.FutureTask;
import java.util.concurrent.TimeUnit;

/**
 * Screen capture on top of a reusable MediaProjection session.
 *
 * <p>The user grants the projection once through the system consent dialog.
 * Android 14+ allows that grant to create one projection and one virtual
 * display, so the bridge keeps that pair alive in a {@link ScreenCaptureSession}
 * behind the mediaProjection foreground service: later screenshots reuse the
 * session instead of asking again. The service notification offers a stop
 * action, and the projection callback releases the session when the system or
 * the user ends it.
 *
 * <p>Every method returns JSON. Failures are values, not exceptions: the agent
 * needs a readable reason (permission missing, service unavailable) instead of
 * a JNI stack trace.
 */
final class ScreenCaptureBridge {
    private static final String TAG = "wunder";
    private static final int REQUEST_SCREEN_CAPTURE = 0x5714;
    private static final int KEEP_SCREENSHOTS = 4;
    private static final long SERVICE_START_TIMEOUT_MS = 2_000;

    private static final Object LOCK = new Object();
    // One physical display per process: parallel tool calls must not create
    // competing virtual displays, so the whole capture is serialized.
    private static final Object CAPTURE_LOCK = new Object();

    private static WeakReference<Activity> activity = new WeakReference<>(null);
    private static Context appContext;
    private static int resultCode = Activity.RESULT_CANCELED;
    private static Intent resultData;
    /** Live session created from the current grant; null after stop/revoke. */
    private static ScreenCaptureSession session;

    private ScreenCaptureBridge() {
    }

    /** Remember the current activity; called by the result hook and the bridge. */
    static void attach(Activity current) {
        if (current == null) {
            return;
        }
        activity = new WeakReference<>(current);
        appContext = current.getApplicationContext();
    }

    static String execute(Context context, String action, String argsJson) {
        try {
            if (context != null) {
                appContext = context.getApplicationContext();
            }
            Context base = appContext != null ? appContext : context;
            String name = action == null ? "" : action.trim();
            switch (name) {
                case "capture":
                    return capture(base).toString();
                case "status":
                    return status(base).toString();
                case "request_permission":
                    return requestPermission();
                case "stop":
                    stopSession(base);
                    return okJson("Screen capture session stopped.");
                default:
                    return errorJson("unknown_action", "Unknown screen action: " + name);
            }
        } catch (Throwable error) {
            Log.w(TAG, "screen bridge failed: " + error);
            return errorJson("internal_error", String.valueOf(error));
        }
    }

    /** True while a grant is waiting to be consumed or a session is active. */
    static boolean hasPermission() {
        synchronized (LOCK) {
            return (resultData != null && resultCode == Activity.RESULT_OK)
                    || (session != null && session.isActive());
        }
    }

    /**
     * Ask the user for screen capture access. The consent dialog needs an
     * Activity; without one the tool answers with a readable error instead of
     * silently doing nothing.
     */
    static String requestPermission() {
        Activity current = currentActivity();
        if (current == null) {
            return errorJson(
                    "activity_unavailable",
                    "The wunder activity is not available; open the app before requesting screen capture.");
        }
        return requestPermission(current);
    }

    static String requestPermission(Activity current) {
        if (current == null) {
            return errorJson("activity_unavailable", "No activity is available.");
        }
        attach(current);
        try {
            MediaProjectionManager manager = (MediaProjectionManager)
                    current.getSystemService(Context.MEDIA_PROJECTION_SERVICE);
            if (manager == null) {
                return errorJson("unsupported", "This system has no MediaProjection service.");
            }
            Intent intent = manager.createScreenCaptureIntent();
            Boolean started = onMain(() -> {
                current.startActivityForResult(intent, REQUEST_SCREEN_CAPTURE);
                return Boolean.TRUE;
            }, 4_000);
            if (!Boolean.TRUE.equals(started)) {
                return errorJson(
                        "activity_unavailable",
                        "The screen-capture consent dialog could not be opened.");
            }
            JSONObject result = new JSONObject();
            result.put("ok", true);
            result.put("requested", true);
            return result.toString();
        } catch (Throwable error) {
            return errorJson("internal_error", String.valueOf(error));
        }
    }

    /**
     * Deliver a {@code startActivityForResult} reply from WunderActivity.
     *
     * @return true when the request belonged to this bridge.
     */
    static boolean handleActivityResult(
            Activity current, int requestCode, int responseCode, Intent data) {
        if (requestCode != REQUEST_SCREEN_CAPTURE) {
            return false;
        }
        attach(current);
        boolean granted = responseCode == Activity.RESULT_OK && data != null;
        ScreenCaptureSession replaced = null;
        boolean hasSession;
        synchronized (LOCK) {
            hasSession = session != null && session.isActive();
            if (granted) {
                replaced = session;
                session = null;
            }
            resultCode = granted ? Activity.RESULT_OK : Activity.RESULT_CANCELED;
            resultData = granted ? new Intent(data) : null;
        }
        if (replaced != null) {
            replaced.close();
        }
        // A denial only clears an unused grant: an existing session keeps
        // working. A fresh grant is consumed lazily by the next capture, which
        // starts the service right before getMediaProjection as Android 14+
        // requires.
        if (granted || !hasSession) {
            ScreenCaptureService.stop(current);
        }
        return true;
    }

    // ---- actions -----------------------------------------------------------

    private static JSONObject capture(Context context) {
        if (context == null) {
            return errorObject("context_unavailable", "No Android context is available.");
        }
        synchronized (CAPTURE_LOCK) {
            ScreenCaptureSession active = activeSession();
            if (active == null) {
                active = openSession(context);
                if (active == null) {
                    return errorObject(
                            "permission_required",
                            "Screen capture is not authorized; request permission first.");
                }
            }
            int[] size = screenSize(context);
            if (size[0] <= 0 || size[1] <= 0) {
                return errorObject("display_unavailable", messageFor("display_unavailable"));
            }
            Bitmap shot = active.capture(size[0], size[1], density(context));
            if (shot == null) {
                if (!active.isActive()) {
                    dropSession(active);
                    return errorObject("projection_stopped", messageFor("projection_stopped"));
                }
                return errorObject("capture_timeout", messageFor("capture_timeout"));
            }
            try {
                return saveFrame(context, shot);
            } finally {
                shot.recycle();
            }
        }
    }

    /**
     * Consume the pending grant and open the reusable session. Android 14+
     * requires the mediaProjection foreground service to be running before
     * getMediaProjection, and only one projection may be created per consent,
     * so the projection and its virtual display live for the whole session.
     */
    private static ScreenCaptureSession openSession(Context context) {
        Intent grant;
        int code;
        synchronized (LOCK) {
            if (session != null && session.isActive()) {
                return session;
            }
            if (resultData == null || resultCode != Activity.RESULT_OK) {
                return null;
            }
            grant = new Intent(resultData);
            code = resultCode;
        }
        if (!ensureServiceRunning(context)) {
            return null;
        }
        ScreenCaptureSession created = null;
        try {
            MediaProjectionManager manager = (MediaProjectionManager)
                    context.getSystemService(Context.MEDIA_PROJECTION_SERVICE);
            MediaProjection projection = manager == null
                    ? null : manager.getMediaProjection(code, grant);
            if (projection == null) {
                // The token was consumed or rejected; ask again next time.
                synchronized (LOCK) {
                    clearPermissionLocked();
                }
                ScreenCaptureService.stop(context);
                return null;
            }
            int[] size = screenSize(context);
            created = ScreenCaptureSession.open(
                    projection,
                    size[0],
                    size[1],
                    density(context),
                    ScreenCaptureBridge::onProjectionStopped);
            if (created == null) {
                try {
                    projection.stop();
                } catch (Throwable ignored) {
                }
                // The consent is single-use; a failed setup must ask again.
                synchronized (LOCK) {
                    clearPermissionLocked();
                }
                ScreenCaptureService.stop(context);
                return null;
            }
            synchronized (LOCK) {
                session = created;
                clearPermissionLocked();
            }
            return created;
        } catch (Throwable error) {
            Log.w(TAG, "screen capture session failed: " + error);
            if (created != null) {
                created.close();
            }
            synchronized (LOCK) {
                session = null;
                clearPermissionLocked();
            }
            ScreenCaptureService.stop(context);
            return null;
        }
    }

    private static ScreenCaptureSession activeSession() {
        synchronized (LOCK) {
            return session != null && session.isActive() ? session : null;
        }
    }

    /** Drop a dead session so the next capture can ask for a fresh grant. */
    private static void dropSession(ScreenCaptureSession dead) {
        synchronized (LOCK) {
            if (session == dead) {
                session = null;
            }
            clearPermissionLocked();
        }
        ScreenCaptureService.stop(appContext);
    }

    /** Projection ended by the system or the user; release everything. */
    private static void onProjectionStopped() {
        synchronized (LOCK) {
            session = null;
            clearPermissionLocked();
        }
        ScreenCaptureService.stop(appContext);
    }

    /** Stop the reusable session, for example from the service notification. */
    static void stopSession(Context context) {
        if (context != null) {
            appContext = context.getApplicationContext();
        }
        ScreenCaptureSession current;
        synchronized (LOCK) {
            current = session;
            session = null;
            clearPermissionLocked();
        }
        if (current != null) {
            current.close();
        }
        ScreenCaptureService.stop(context != null ? context : appContext);
    }

    /** The foreground service is gone, so its projection may not continue. */
    static void onServiceStopped() {
        ScreenCaptureSession current;
        synchronized (LOCK) {
            current = session;
            session = null;
        }
        if (current != null) {
            current.close();
        }
    }

    private static boolean ensureServiceRunning(Context context) {
        if (ScreenCaptureService.isRunning()) {
            return true;
        }
        ScreenCaptureService.start(context);
        long deadline = SystemClock.uptimeMillis() + SERVICE_START_TIMEOUT_MS;
        while (!ScreenCaptureService.isRunning() && SystemClock.uptimeMillis() < deadline) {
            sleep(30);
        }
        return ScreenCaptureService.isRunning();
    }

    /**
     * Reserve the next free {@code screenshot_<n>.png}. The name is what the
     * phone-control result exposes to the model, so a serial beats an epoch
     * timestamp on both readability and ordering.
     */
    private static File nextScreenshotFile(File directory) {
        int max = 0;
        File[] files = directory.listFiles();
        if (files != null) {
            String head = "screenshot_";
            for (File file : files) {
                String name = file.getName();
                if (!name.startsWith(head) || !name.endsWith(".png")) {
                    continue;
                }
                String stem = name.substring(head.length(), name.length() - 4);
                try {
                    max = Math.max(max, Integer.parseInt(stem));
                } catch (NumberFormatException ignored) {
                    // Old timestamp or unrelated name: not part of the serial.
                }
            }
        }
        for (int index = max + 1; index < max + 4096; index++) {
            File candidate = new File(directory, "screenshot_" + index + ".png");
            try {
                if (candidate.createNewFile()) {
                    return candidate;
                }
            } catch (IOException error) {
                return null;
            }
        }
        return null;
    }

    private static JSONObject saveFrame(Context context, Bitmap shot) {
        int width = shot.getWidth();
        int height = shot.getHeight();
        File directory = new File(context.getCacheDir(), "android-screen");
        if (!directory.exists() && !directory.mkdirs()) {
            return errorObject("storage_unavailable", "The screenshot directory is not writable.");
        }
        File file = nextScreenshotFile(directory);
        if (file == null) {
            return errorObject("storage_unavailable", "No free screenshot name.");
        }
        try (FileOutputStream out = new FileOutputStream(file)) {
            if (!shot.compress(Bitmap.CompressFormat.PNG, 100, out)) {
                //noinspection ResultOfMethodCallIgnored
                file.delete();
                return errorObject("storage_unavailable", "PNG encoding failed.");
            }
        } catch (Throwable error) {
            //noinspection ResultOfMethodCallIgnored
            file.delete();
            return errorObject("storage_unavailable", String.valueOf(error));
        }
        prune(directory);
        try {
            JSONObject result = new JSONObject();
            result.put("ok", true);
            result.put("path", file.getAbsolutePath());
            result.put("width", width);
            result.put("height", height);
            return result;
        } catch (Throwable error) {
            return errorObject("internal_error", String.valueOf(error));
        }
    }

    private static JSONObject status(Context context) {
        JSONObject result = new JSONObject();
        try {
            boolean pendingGrant;
            boolean projecting;
            synchronized (LOCK) {
                pendingGrant = resultData != null && resultCode == Activity.RESULT_OK;
                projecting = session != null && session.isActive();
            }
            int[] size = screenSize(context);
            result.put("ok", true);
            result.put("authorized", pendingGrant || projecting);
            result.put("projecting", projecting);
            result.put("service_running", ScreenCaptureService.isRunning());
            result.put("screen_width", size[0]);
            result.put("screen_height", size[1]);
        } catch (Throwable error) {
            return errorObject("internal_error", String.valueOf(error));
        }
        return result;
    }

    // ---- projection --------------------------------------------------------

    private static void clearPermissionLocked() {
        resultData = null;
        resultCode = Activity.RESULT_CANCELED;
    }

    // ---- files -------------------------------------------------------------

    private static void prune(File directory) {
        File[] files = directory.listFiles((dir, name) ->
                name.startsWith("screenshot_") || name.startsWith("shot-"));
        if (files == null || files.length <= KEEP_SCREENSHOTS) {
            return;
        }
        Arrays.sort(files, Comparator.comparingLong(File::lastModified));
        int remove = files.length - KEEP_SCREENSHOTS;
        for (int index = 0; index < remove; index++) {
            // Best effort: a file still open for reading is simply kept.
            files[index].delete();
        }
    }

    // ---- helpers -----------------------------------------------------------

    private static Activity currentActivity() {
        Activity cached = activity.get();
        if (cached != null) {
            return cached;
        }
        // UiBridge owns the activity weak reference. It is private, but both
        // classes live in this package and ship in the same APK; reflection is
        // the only hook available without changing that bridge.
        try {
            Field field = UiBridge.class.getDeclaredField("activity");
            field.setAccessible(true);
            Object value = field.get(null);
            if (value instanceof WeakReference) {
                Object current = ((WeakReference<?>) value).get();
                if (current instanceof Activity) {
                    return (Activity) current;
                }
            }
        } catch (Throwable ignored) {
        }
        return null;
    }

    private static int[] screenSize(Context context) {
        if (context == null) {
            return new int[]{0, 0};
        }
        try {
            WindowManager manager =
                    (WindowManager) context.getSystemService(Context.WINDOW_SERVICE);
            if (manager == null) {
                return new int[]{0, 0};
            }
            if (android.os.Build.VERSION.SDK_INT >= 30) {
                Rect bounds = manager.getCurrentWindowMetrics().getBounds();
                return new int[]{bounds.width(), bounds.height()};
            }
            DisplayMetrics metrics = new DisplayMetrics();
            manager.getDefaultDisplay().getRealMetrics(metrics);
            return new int[]{metrics.widthPixels, metrics.heightPixels};
        } catch (Throwable ignored) {
            return new int[]{0, 0};
        }
    }

    private static int density(Context context) {
        try {
            return Math.max(1, context.getResources().getDisplayMetrics().densityDpi);
        } catch (Throwable ignored) {
            return DisplayMetrics.DENSITY_DEFAULT;
        }
    }

    private static String messageFor(String code) {
        switch (code) {
            case "permission_required":
                return "Screen capture is not authorized; request permission first.";
            case "service_unavailable":
                return "The screen-capture foreground service is not running.";
            case "projection_stopped":
                return "The screen capture session ended; request permission again.";
            case "display_unavailable":
                return "The display size is unavailable.";
            case "capture_timeout":
                return "The screen frame did not arrive in time.";
            case "storage_unavailable":
                return "The screenshot could not be written.";
            default:
                return "Screen capture failed.";
        }
    }

    private static JSONObject errorObject(String code, String message) {
        JSONObject result = new JSONObject();
        try {
            result.put("ok", false);
            result.put("code", code == null ? "capture_failed" : code);
            result.put("message", message == null ? "" : message);
        } catch (Throwable ignored) {
        }
        return result;
    }

    private static String errorJson(String code, String message) {
        return errorObject(code, message).toString();
    }

    private static String okJson(String message) {
        JSONObject result = new JSONObject();
        try {
            result.put("ok", true);
            result.put("message", message == null ? "" : message);
        } catch (Throwable ignored) {
        }
        return result.toString();
    }

    private static <T> T onMain(Callable<T> task, long timeoutMs) {
        if (Looper.myLooper() == Looper.getMainLooper()) {
            try {
                return task.call();
            } catch (Exception error) {
                return null;
            }
        }
        FutureTask<T> future = new FutureTask<>(task);
        new Handler(Looper.getMainLooper()).post(future);
        try {
            return future.get(timeoutMs, TimeUnit.MILLISECONDS);
        } catch (Exception error) {
            future.cancel(true);
            return null;
        }
    }

    private static void sleep(long millis) {
        if (millis <= 0) {
            return;
        }
        try {
            Thread.sleep(Math.min(millis, 30_000));
        } catch (InterruptedException interrupted) {
            Thread.currentThread().interrupt();
        }
    }
}
