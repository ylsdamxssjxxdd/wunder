package dev.wunder.android;

import android.app.Activity;
import android.content.Context;
import android.content.Intent;
import android.graphics.Bitmap;
import android.graphics.PixelFormat;
import android.graphics.Rect;
import android.hardware.display.DisplayManager;
import android.hardware.display.VirtualDisplay;
import android.media.Image;
import android.media.ImageReader;
import android.media.projection.MediaProjection;
import android.media.projection.MediaProjectionManager;
import android.os.Handler;
import android.os.HandlerThread;
import android.os.Looper;
import android.os.SystemClock;
import android.util.DisplayMetrics;
import android.util.Log;
import android.view.WindowManager;

import org.json.JSONObject;

import java.io.File;
import java.io.FileOutputStream;
import java.lang.ref.WeakReference;
import java.lang.reflect.Field;
import java.nio.ByteBuffer;
import java.util.Arrays;
import java.util.Comparator;
import java.util.concurrent.Callable;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.FutureTask;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicReference;

/**
 * Single-frame screen capture on top of MediaProjection.
 *
 * <p>The user grants the projection through the system consent dialog. Android
 * 14+ allows that grant to create one projection and one virtual display, so
 * every capture consumes a fresh grant: the bridge starts the foreground
 * service for the frame, releases the projection, and stops the service as
 * soon as the PNG is saved. A new consent is required for the next capture.
 *
 * <p>Every method returns JSON. Failures are values, not exceptions: the agent
 * needs a readable reason (permission missing, service unavailable) instead of
 * a JNI stack trace.
 */
final class ScreenCaptureBridge {
    private static final String TAG = "wunder";
    private static final int REQUEST_SCREEN_CAPTURE = 0x5714;
    private static final int KEEP_SCREENSHOTS = 4;
    private static final long CAPTURE_TIMEOUT_MS = 6_000;
    private static final long SERVICE_START_TIMEOUT_MS = 2_000;

    private static final Object LOCK = new Object();
    // One physical display per process: parallel tool calls must not create
    // competing virtual displays, so the whole capture is serialized.
    private static final Object CAPTURE_LOCK = new Object();

    private static WeakReference<Activity> activity = new WeakReference<>(null);
    private static Context appContext;
    private static int resultCode = Activity.RESULT_CANCELED;
    private static Intent resultData;
    private static MediaProjection projection;

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
                default:
                    return errorJson("unknown_action", "Unknown screen action: " + name);
            }
        } catch (Throwable error) {
            Log.w(TAG, "screen bridge failed: " + error);
            return errorJson("internal_error", String.valueOf(error));
        }
    }

    /** True while a one-shot projection grant is waiting to be consumed. */
    static boolean hasPermission() {
        synchronized (LOCK) {
            return resultData != null && resultCode == Activity.RESULT_OK;
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
        synchronized (LOCK) {
            releaseProjectionLocked();
            ScreenCaptureBridge.resultCode =
                    granted ? Activity.RESULT_OK : Activity.RESULT_CANCELED;
            ScreenCaptureBridge.resultData = granted ? new Intent(data) : null;
        }
        // Do not start the foreground service on consent alone. Android shows
        // its notification immediately for mediaProjection, so a long-lived
        // service would leave "screen capture ready" in the shade until the
        // process dies. The service starts only for the capture that consumes
        // this one-shot grant.
        ScreenCaptureService.stop(current);
        return true;
    }

    // ---- actions -----------------------------------------------------------

    private static JSONObject capture(Context context) {
        if (context == null) {
            return errorObject("context_unavailable", "No Android context is available.");
        }
        synchronized (CAPTURE_LOCK) {
            synchronized (LOCK) {
                if (resultData == null || resultCode != Activity.RESULT_OK) {
                    return errorObject(
                            "permission_required",
                            "Screen capture is not authorized; request permission first.");
                }
            }
            if (!ensureServiceRunning(context)) {
                return errorObject(
                        "service_unavailable",
                        "The screen-capture foreground service did not start.");
            }
            try {
                return captureFrame(context);
            } finally {
                // Android 14+ allows one getMediaProjection/createVirtualDisplay
                // per consent. End the session with the frame so the next
                // screenshot asks for a fresh grant and the notification does
                // not outlive the capture.
                endCaptureSession(context);
            }
        }
    }

    private static void endCaptureSession(Context context) {
        synchronized (LOCK) {
            releaseProjectionLocked();
            clearPermissionLocked();
        }
        ScreenCaptureService.stop(context);
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

    private static JSONObject captureFrame(Context context) {
        int[] size = screenSize(context);
        int density = density(context);
        HandlerThread thread = new HandlerThread("wunder-screen-capture");
        thread.start();
        Handler handler = new Handler(thread.getLooper());
        CountDownLatch setupDone = new CountDownLatch(1);
        CountDownLatch frameDone = new CountDownLatch(1);
        Object frameLock = new Object();
        AtomicBoolean done = new AtomicBoolean(false);
        AtomicReference<Bitmap> frame = new AtomicReference<>();
        AtomicReference<String> failure = new AtomicReference<>();
        AtomicReference<ImageReader> readerRef = new AtomicReference<>();
        AtomicReference<VirtualDisplay> displayRef = new AtomicReference<>();

        handler.post(() -> {
            try {
                if (size[0] <= 0 || size[1] <= 0) {
                    failure.compareAndSet(null, "display_unavailable");
                    return;
                }
                MediaProjection active = ensureProjection();
                if (active == null) {
                    failure.compareAndSet(null, "permission_required");
                    return;
                }
                ImageReader reader = ImageReader.newInstance(
                        size[0], size[1], PixelFormat.RGBA_8888, 2);
                reader.setOnImageAvailableListener(source -> {
                    Image image = null;
                    try {
                        image = source.acquireLatestImage();
                        if (image != null) {
                            Bitmap bitmap = toBitmap(image, size[0], size[1]);
                            if (bitmap != null) {
                                boolean stored;
                                synchronized (frameLock) {
                                    stored = !done.get() && frame.compareAndSet(null, bitmap);
                                }
                                if (!stored) {
                                    bitmap.recycle();
                                }
                            }
                        }
                    } catch (Throwable error) {
                        failure.compareAndSet(null, "capture_failed");
                    } finally {
                        if (image != null) {
                            image.close();
                        }
                        frameDone.countDown();
                    }
                }, handler);
                readerRef.set(reader);
                VirtualDisplay display = active.createVirtualDisplay(
                        "wunder-screen",
                        size[0],
                        size[1],
                        density,
                        DisplayManager.VIRTUAL_DISPLAY_FLAG_AUTO_MIRROR,
                        reader.getSurface(),
                        null,
                        handler);
                displayRef.set(display);
            } catch (Throwable error) {
                failure.compareAndSet(null, projectionErrorCode(error));
            } finally {
                setupDone.countDown();
            }
        });

        try {
            if (!setupDone.await(CAPTURE_TIMEOUT_MS, TimeUnit.MILLISECONDS)) {
                failure.compareAndSet(null, "capture_timeout");
            }
            if (failure.get() == null
                    && !frameDone.await(CAPTURE_TIMEOUT_MS, TimeUnit.MILLISECONDS)) {
                failure.compareAndSet(null, "capture_timeout");
            }
        } catch (InterruptedException interrupted) {
            Thread.currentThread().interrupt();
            failure.compareAndSet(null, "capture_failed");
        }

        Bitmap shot;
        synchronized (frameLock) {
            done.set(true);
            shot = frame.getAndSet(null);
        }
        releaseCapture(handler, displayRef, readerRef);
        thread.quitSafely();
        try {
            thread.join(1_000);
        } catch (InterruptedException interrupted) {
            Thread.currentThread().interrupt();
        }

        if (shot == null) {
            String code = failure.get() != null ? failure.get() : "capture_failed";
            return errorObject(code, messageFor(code));
        }
        try {
            return saveFrame(context, shot);
        } finally {
            shot.recycle();
        }
    }

    private static JSONObject saveFrame(Context context, Bitmap shot) {
        int width = shot.getWidth();
        int height = shot.getHeight();
        File directory = new File(context.getCacheDir(), "android-screen");
        if (!directory.exists() && !directory.mkdirs()) {
            return errorObject("storage_unavailable", "The screenshot directory is not writable.");
        }
        File file = new File(directory, "shot-" + System.currentTimeMillis() + ".png");
        try (FileOutputStream out = new FileOutputStream(file)) {
            if (!shot.compress(Bitmap.CompressFormat.PNG, 100, out)) {
                return errorObject("storage_unavailable", "PNG encoding failed.");
            }
        } catch (Throwable error) {
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
            boolean authorized;
            boolean projecting;
            synchronized (LOCK) {
                authorized = resultData != null && resultCode == Activity.RESULT_OK;
                projecting = projection != null;
            }
            int[] size = screenSize(context);
            result.put("ok", true);
            result.put("authorized", authorized);
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

    private static MediaProjection ensureProjection() {
        synchronized (LOCK) {
            if (projection != null) {
                return projection;
            }
            if (resultData == null || resultCode != Activity.RESULT_OK || appContext == null) {
                return null;
            }
            MediaProjectionManager manager = (MediaProjectionManager)
                    appContext.getSystemService(Context.MEDIA_PROJECTION_SERVICE);
            if (manager == null) {
                return null;
            }
            final MediaProjection created;
            try {
                created = manager.getMediaProjection(resultCode, new Intent(resultData));
            } catch (Throwable error) {
                Log.w(TAG, "getMediaProjection failed: " + error);
                clearPermissionLocked();
                stopServiceOnFailure();
                return null;
            }
            if (created == null) {
                clearPermissionLocked();
                stopServiceOnFailure();
                return null;
            }
            // Android 14 requires a registered callback before the first
            // virtual display. Main looper keeps the callback alive after the
            // per-capture handler thread quits.
            created.registerCallback(new MediaProjection.Callback() {
                @Override
                public void onStop() {
                    boolean current;
                    synchronized (LOCK) {
                        current = projection == created;
                        if (current) {
                            projection = null;
                            clearPermissionLocked();
                        }
                    }
                    if (current && appContext != null) {
                        ScreenCaptureService.stop(appContext);
                    }
                }
            }, new Handler(Looper.getMainLooper()));
            projection = created;
            return created;
        }
    }

    private static void releaseProjectionLocked() {
        MediaProjection current = projection;
        projection = null;
        if (current != null) {
            // onStop sees projection == null and therefore leaves the new
            // grant and its service untouched.
            try {
                current.stop();
            } catch (Throwable ignored) {
            }
        }
    }

    private static void clearPermissionLocked() {
        resultData = null;
        resultCode = Activity.RESULT_CANCELED;
    }

    private static String projectionErrorCode(Throwable error) {
        synchronized (LOCK) {
            releaseProjectionLocked();
            clearPermissionLocked();
        }
        stopServiceOnFailure();
        if (error instanceof SecurityException) {
            return "permission_required";
        }
        if (error instanceof IllegalStateException) {
            return "projection_stopped";
        }
        return "capture_failed";
    }

    private static void stopServiceOnFailure() {
        Context context = appContext;
        if (context != null) {
            ScreenCaptureService.stop(context);
        }
    }

    private static void releaseCapture(
            Handler handler,
            AtomicReference<VirtualDisplay> displayRef,
            AtomicReference<ImageReader> readerRef) {
        handler.post(() -> {
            VirtualDisplay display = displayRef.getAndSet(null);
            if (display != null) {
                try {
                    display.release();
                } catch (Throwable ignored) {
                }
            }
            ImageReader reader = readerRef.getAndSet(null);
            if (reader != null) {
                try {
                    reader.close();
                } catch (Throwable ignored) {
                }
            }
        });
    }

    // ---- pixels and files --------------------------------------------------

    private static Bitmap toBitmap(Image image, int width, int height) {
        Image.Plane plane = image.getPlanes()[0];
        ByteBuffer buffer = plane.getBuffer();
        int pixelStride = plane.getPixelStride();
        int rowStride = plane.getRowStride();
        int rowPadding = rowStride - pixelStride * width;
        int paddedWidth = width + Math.max(0, rowPadding) / Math.max(1, pixelStride);
        Bitmap padded;
        try {
            padded = Bitmap.createBitmap(paddedWidth, height, Bitmap.Config.ARGB_8888);
        } catch (Throwable error) {
            return null;
        }
        try {
            buffer.rewind();
            padded.copyPixelsFromBuffer(buffer);
            if (paddedWidth == width) {
                return padded;
            }
            Bitmap cropped = Bitmap.createBitmap(padded, 0, 0, width, height);
            padded.recycle();
            return cropped;
        } catch (Throwable error) {
            padded.recycle();
            return null;
        }
    }

    private static void prune(File directory) {
        File[] files = directory.listFiles((dir, name) -> name.startsWith("shot-"));
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
