package dev.wunder.android;

import android.graphics.Bitmap;
import android.graphics.PixelFormat;
import android.hardware.display.DisplayManager;
import android.hardware.display.VirtualDisplay;
import android.media.Image;
import android.media.ImageReader;
import android.media.projection.MediaProjection;
import android.os.Handler;
import android.os.HandlerThread;
import android.os.Looper;
import android.os.SystemClock;
import android.util.Log;

import java.nio.ByteBuffer;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicReference;

/**
 * Long-lived MediaProjection session shared by every screenshot of one grant.
 *
 * <p>Android 14+ allows exactly one {@code getMediaProjection} and one virtual
 * display per consent, and Android 15+ ties a projection token to a single
 * capture session. The bridge therefore asks the user once, keeps this session
 * (projection + virtual display) alive behind the mediaProjection foreground
 * service, and reuses it for later frames. The user ends the session from the
 * service notification at any time.
 *
 * <p>A frame is taken by attaching a fresh {@link ImageReader} surface to the
 * display: a new surface forces the compositor to produce a current frame, so
 * a screenshot never returns a frame left over from an older screen. The
 * reader is detached and closed right after the bitmap is copied.
 */
final class ScreenCaptureSession {
    private static final String TAG = "wunder";
    private static final long FRAME_TIMEOUT_MS = 4_000L;
    private static final long POLL_INTERVAL_MS = 40L;

    private final MediaProjection projection;
    private final HandlerThread thread;
    private final Handler handler;
    private final Runnable onStopped;

    private volatile boolean closed;
    private volatile VirtualDisplay display;

    private final MediaProjection.Callback callback = new MediaProjection.Callback() {
        @Override
        public void onStop() {
            // The system or the user ended the projection (for example from the
            // status bar). The grant is unusable now; release everything and
            // let the bridge drop the session and its notification.
            handleProjectionStopped();
        }
    };

    private ScreenCaptureSession(
            MediaProjection projection,
            HandlerThread thread,
            Handler handler,
            Runnable onStopped) {
        this.projection = projection;
        this.thread = thread;
        this.handler = handler;
        this.onStopped = onStopped;
    }

    /** Create the session and its single virtual display. Null on failure. */
    static ScreenCaptureSession open(
            MediaProjection projection,
            int width,
            int height,
            int density,
            Runnable onStopped) {
        if (projection == null || width <= 0 || height <= 0) {
            return null;
        }
        HandlerThread thread = new HandlerThread("wunder-screen-session");
        thread.start();
        Handler handler = new Handler(thread.getLooper());
        ScreenCaptureSession session =
                new ScreenCaptureSession(projection, thread, handler, onStopped);
        // Android 14+ requires a registered callback before the first virtual
        // display is created.
        try {
            projection.registerCallback(session.callback, new Handler(Looper.getMainLooper()));
        } catch (Throwable error) {
            Log.w(TAG, "projection callback registration failed", error);
        }
        VirtualDisplay created;
        try {
            created = projection.createVirtualDisplay(
                    "wunder-screen",
                    width,
                    height,
                    density,
                    DisplayManager.VIRTUAL_DISPLAY_FLAG_AUTO_MIRROR,
                    null,
                    null,
                    handler);
        } catch (Throwable error) {
            Log.w(TAG, "createVirtualDisplay failed", error);
            session.close();
            return null;
        }
        if (created == null) {
            session.close();
            return null;
        }
        session.display = created;
        return session;
    }

    boolean isActive() {
        return !closed && display != null;
    }

    /**
     * Capture one current frame. Returns null when the session ended or the
     * display produced no frame before the timeout. The caller owns the
     * bitmap.
     */
    Bitmap capture(int width, int height, int density) {
        if (closed || width <= 0 || height <= 0) {
            return null;
        }
        CountDownLatch latch = new CountDownLatch(1);
        AtomicReference<Bitmap> frame = new AtomicReference<>();
        handler.post(() -> {
            if (closed) {
                latch.countDown();
                return;
            }
            ImageReader reader = null;
            try {
                reader = ImageReader.newInstance(
                        width, height, PixelFormat.RGBA_8888, 2);
                VirtualDisplay active = display;
                if (active == null) {
                    latch.countDown();
                    return;
                }
                active.setSurface(reader.getSurface());
                // A freshly attached surface plus a resize makes the mirrored
                // display compose immediately even when the screen is static.
                active.resize(width, height, density);
                long deadline = SystemClock.uptimeMillis() + FRAME_TIMEOUT_MS;
                while (!closed && SystemClock.uptimeMillis() < deadline) {
                    Image image = reader.acquireLatestImage();
                    if (image != null) {
                        try {
                            Bitmap bitmap = toBitmap(image, width, height);
                            if (bitmap != null) {
                                frame.set(bitmap);
                                break;
                            }
                        } finally {
                            image.close();
                        }
                    }
                    Thread.sleep(POLL_INTERVAL_MS);
                }
            } catch (InterruptedException interrupted) {
                Thread.currentThread().interrupt();
            } catch (Throwable error) {
                Log.w(TAG, "screen frame failed", error);
            } finally {
                VirtualDisplay active = display;
                if (active != null) {
                    try {
                        active.setSurface(null);
                    } catch (Throwable ignored) {
                    }
                }
                if (reader != null) {
                    try {
                        reader.close();
                    } catch (Throwable ignored) {
                    }
                }
                latch.countDown();
            }
        });
        try {
            latch.await(FRAME_TIMEOUT_MS + 2_000L, TimeUnit.MILLISECONDS);
        } catch (InterruptedException interrupted) {
            Thread.currentThread().interrupt();
        }
        return frame.get();
    }

    /** Release the display and projection. Safe to call more than once. */
    void close() {
        closed = true;
        releaseAsync();
    }

    private void handleProjectionStopped() {
        boolean wasClosed = closed;
        closed = true;
        releaseAsync();
        Runnable notify = onStopped;
        if (!wasClosed && notify != null) {
            notify.run();
        }
    }

    private void releaseAsync() {
        handler.post(() -> {
            VirtualDisplay active = display;
            display = null;
            if (active != null) {
                try {
                    active.setSurface(null);
                } catch (Throwable ignored) {
                }
                try {
                    active.release();
                } catch (Throwable ignored) {
                }
            }
            try {
                projection.unregisterCallback(callback);
            } catch (Throwable ignored) {
            }
            try {
                projection.stop();
            } catch (Throwable ignored) {
            }
            thread.quitSafely();
        });
    }

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
}
