package dev.wunder.android;

import android.content.Context;
import android.content.Intent;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.graphics.Canvas;
import android.graphics.Paint;
import android.graphics.PixelFormat;
import android.graphics.Rect;
import android.graphics.RectF;
import android.net.Uri;
import android.os.Handler;
import android.os.Looper;
import android.provider.Settings;
import android.text.Layout;
import android.text.StaticLayout;
import android.text.TextPaint;
import android.text.TextUtils;
import android.util.DisplayMetrics;
import android.util.Log;
import android.view.Gravity;
import android.view.MotionEvent;
import android.view.View;
import android.view.ViewConfiguration;
import android.view.WindowManager;

import org.json.JSONArray;
import org.json.JSONObject;

import java.util.Locale;
import java.util.concurrent.Callable;
import java.util.concurrent.ConcurrentLinkedQueue;
import java.util.concurrent.FutureTask;
import java.util.concurrent.TimeUnit;

/**
 * System-level floating companion (desktop pet) for the Android form factor.
 *
 * <p>The Rust companion controller owns the state projection and drives this
 * bridge through a small action contract; this class owns only the overlay
 * window, the sprite frame clock and touch handling. All window work happens
 * on the main thread. Touch results wait in a static queue that the controller
 * polls: NativeActivity loads the native library through the framework class
 * loader, so app classes must not call native symbols.
 */
public final class CompanionBridge {
    private static final String TAG = "WunderCompanion";
    // Sprite table shared with the desktop (companion_sprite.rs): 192x208
    // frames, one animation per row, frames laid out horizontally.
    private static final int FRAME_WIDTH = 192;
    private static final int FRAME_HEIGHT = 208;
    private static final int DRAG_RIGHT_ROW = 1;
    private static final int DRAG_LEFT_ROW = 2;
    private static final int DRAG_FRAMES = 8;
    private static final int DRAG_FRAME_MS = 132;
    private static final long LONG_PRESS_MS = 500L;
    private static final int BUBBLE_MAX_WIDTH_DP = 220;
    private static final int BUBBLE_PADDING_DP = 10;
    private static final int BUBBLE_GAP_DP = 6;
    private static final int SCREEN_MARGIN_DP = 8;

    private static final Handler MAIN = new Handler(Looper.getMainLooper());
    private static final ConcurrentLinkedQueue<String> EVENTS = new ConcurrentLinkedQueue<>();

    private static Context app;
    private static WindowManager windowManager;
    private static PetView petView;
    private static WindowManager.LayoutParams params;
    private static Bitmap sheet;
    private static String sheetPath = "";
    private static int sheetColumns;
    private static int sheetRows;
    private static float scale = 0.5f;
    private static int spriteWidth;
    private static int spriteHeight;
    // Sprite top-left anchor in screen pixels; the window itself grows upward
    // and sideways when a bubble is visible, the sprite never moves.
    private static int petX = -1;
    private static int petY = -1;
    private static int baseRow;
    private static int baseFrames = 6;
    private static int baseFrameMs = 183;
    private static int frameIndex;
    private static boolean dragging;
    private static int dragRow = -1;
    private static String bubble = "";
    private static int bubbleTone;
    private static long lastPermissionRequestAt;

    private CompanionBridge() {}

    /** Entry point used by the Rust companion controller. */
    public static String execute(Context context, String action, String argsJson) {
        try {
            if (context != null) {
                app = context.getApplicationContext();
            }
            if (app == null) {
                return error("应用上下文不可用");
            }
            JSONObject args = new JSONObject(argsJson == null || argsJson.isEmpty() ? "{}" : argsJson);
            String name = action == null ? "" : action.trim().toLowerCase(Locale.ROOT);
            switch (name) {
                case "status":
                    return JSONObject.quote(Settings.canDrawOverlays(app) ? "granted" : "denied");
                case "take_events":
                    return takeEvents();
                case "request_permission":
                    requestOverlayPermission();
                    return ok();
                case "present":
                    return requireMain(() -> presentOnMain(args));
                case "update":
                    return requireMain(() -> updateOnMain(args));
                case "dismiss":
                    return requireMain(() -> {
                        dismissOnMain();
                        return ok();
                    });
                default:
                    return error("未知动作: " + name);
            }
        } catch (Throwable failure) {
            Log.w(TAG, "companion action failed", failure);
            return error(failure.getMessage() == null ? "执行失败" : failure.getMessage());
        }
    }

    // ---- overlay lifecycle -------------------------------------------------

    private static String presentOnMain(JSONObject args) {
        if (!Settings.canDrawOverlays(app)) {
            return error("no_permission");
        }
        String path = args.optString("sheet", "");
        if (path.isEmpty()) {
            return error("缺少精灵图");
        }
        if (sheet == null || !path.equals(sheetPath)) {
            Bitmap decoded = BitmapFactory.decodeFile(path);
            if (decoded == null) {
                return error("精灵图解码失败");
            }
            int columns = decoded.getWidth() / FRAME_WIDTH;
            int rows = decoded.getHeight() / FRAME_HEIGHT;
            if (columns == 0 || rows == 0) {
                decoded.recycle();
                return error("精灵图尺寸不正确");
            }
            if (sheet != null) {
                sheet.recycle();
            }
            sheet = decoded;
            sheetPath = path;
            sheetColumns = columns;
            sheetRows = rows;
        }
        scale = (float) args.optDouble("scale", 0.5);
        scale = Math.max(0.1f, Math.min(1.6f, scale));
        spriteWidth = Math.round(FRAME_WIDTH * scale * density());
        spriteHeight = Math.round(FRAME_HEIGHT * scale * density());
        petX = args.optInt("x", -1);
        petY = args.optInt("y", -1);
        if (petX < 0 || petY < 0) {
            DisplayMetrics metrics = app.getResources().getDisplayMetrics();
            petX = metrics.widthPixels - spriteWidth - dp(SCREEN_MARGIN_DP);
            petY = metrics.heightPixels - spriteHeight - dp(96);
        }
        applyAnim(args.optInt("row", 0), args.optInt("frames", 6), args.optInt("frameMs", 183));
        bubble = args.optString("bubble", "");
        bubbleTone = args.optInt("tone", 0);
        if (petView == null) {
            windowManager = (WindowManager) app.getSystemService(Context.WINDOW_SERVICE);
            if (windowManager == null) {
                return error("窗口服务不可用");
            }
            petView = new PetView(app);
            params = new WindowManager.LayoutParams(
                    spriteWidth,
                    spriteHeight,
                    WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,
                    WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL
                            | WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE
                            | WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN,
                    PixelFormat.TRANSLUCENT);
            params.gravity = Gravity.TOP | Gravity.START;
        }
        petView.rebuildBubble();
        relayoutOnMain();
        startClock();
        return ok();
    }

    private static String updateOnMain(JSONObject args) {
        if (petView == null) {
            return ok();
        }
        applyAnim(args.optInt("row", baseRow), args.optInt("frames", baseFrames),
                args.optInt("frameMs", baseFrameMs));
        String text = args.optString("bubble", "");
        int tone = args.optInt("tone", 0);
        if (!text.equals(bubble) || tone != bubbleTone) {
            bubble = text;
            bubbleTone = tone;
            petView.rebuildBubble();
            relayoutOnMain();
        }
        petView.invalidate();
        return ok();
    }

    private static void dismissOnMain() {
        stopClock();
        if (petView != null && windowManager != null) {
            try {
                windowManager.removeView(petView);
            } catch (Throwable ignored) {
            }
        }
        petView = null;
        params = null;
        if (sheet != null) {
            sheet.recycle();
            sheet = null;
        }
        sheetPath = "";
        bubble = "";
        dragging = false;
        dragRow = -1;
    }

    private static void applyAnim(int row, int frames, int frameMs) {
        if (row != baseRow) {
            frameIndex = 0;
        }
        baseRow = Math.max(0, row);
        baseFrames = Math.max(1, frames);
        baseFrameMs = Math.max(40, frameMs);
    }

    // ---- layout and frame clock --------------------------------------------

    private static void relayoutOnMain() {
        if (petView == null || params == null || windowManager == null) {
            return;
        }
        int bubbleArea = 0;
        int windowWidth = spriteWidth;
        if (petView.bubbleLayout != null) {
            bubbleArea = petView.bubbleLayout.getHeight() + 2 * dp(BUBBLE_PADDING_DP) + dp(BUBBLE_GAP_DP);
            windowWidth = Math.max(spriteWidth, petView.bubbleWidth);
        }
        int windowHeight = spriteHeight + bubbleArea;
        DisplayMetrics metrics = app.getResources().getDisplayMetrics();
        int windowX = petX - (windowWidth - spriteWidth) / 2;
        int windowY = petY - bubbleArea;
        windowX = Math.max(0, Math.min(windowX, metrics.widthPixels - windowWidth));
        windowY = Math.max(0, Math.min(windowY, metrics.heightPixels - windowHeight));
        petX = windowX + (windowWidth - spriteWidth) / 2;
        petY = windowY + bubbleArea;
        params.width = windowWidth;
        params.height = windowHeight;
        params.x = windowX;
        params.y = windowY;
        if (petView.isAttachedToWindow()) {
            windowManager.updateViewLayout(petView, params);
        } else {
            windowManager.addView(petView, params);
        }
        petView.invalidate();
    }

    private static final Runnable FRAME_TICK = new Runnable() {
        @Override
        public void run() {
            if (petView == null) {
                return;
            }
            frameIndex++;
            petView.invalidate();
            MAIN.postDelayed(this, dragging && dragRow >= 0 ? DRAG_FRAME_MS : baseFrameMs);
        }
    };

    private static void startClock() {
        MAIN.removeCallbacks(FRAME_TICK);
        MAIN.postDelayed(FRAME_TICK, baseFrameMs);
    }

    private static void stopClock() {
        MAIN.removeCallbacks(FRAME_TICK);
    }

    // ---- events and permission ---------------------------------------------

    private static String takeEvents() {
        JSONArray events = new JSONArray();
        String event;
        while ((event = EVENTS.poll()) != null) {
            events.put(event);
        }
        return events.toString();
    }

    private static void requestOverlayPermission() {
        long now = System.currentTimeMillis();
        if (now - lastPermissionRequestAt < 8_000L) {
            return;
        }
        lastPermissionRequestAt = now;
        try {
            Intent intent = new Intent(
                    Settings.ACTION_MANAGE_OVERLAY_PERMISSION,
                    Uri.parse("package:" + app.getPackageName()));
            intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            app.startActivity(intent);
        } catch (Throwable failure) {
            Log.w(TAG, "open overlay settings failed", failure);
        }
    }

    private static void launchApp() {
        try {
            Intent intent = new Intent(app, WunderActivity.class);
            intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_SINGLE_TOP);
            app.startActivity(intent);
        } catch (Throwable failure) {
            Log.w(TAG, "launch activity failed", failure);
        }
    }

    // ---- helpers -----------------------------------------------------------

    private static String requireMain(Callable<String> task) {
        String result = onMain(task, 5_000L);
        return result == null ? error("主线程任务超时") : result;
    }

    private static <T> T onMain(Callable<T> task, long timeoutMs) {
        if (Looper.myLooper() == Looper.getMainLooper()) {
            try {
                return task.call();
            } catch (Exception failure) {
                Log.w(TAG, "main-thread task failed", failure);
                return null;
            }
        }
        FutureTask<T> future = new FutureTask<>(task);
        MAIN.post(future);
        try {
            return future.get(timeoutMs, TimeUnit.MILLISECONDS);
        } catch (Exception failure) {
            future.cancel(true);
            return null;
        }
    }

    private static String ok() {
        return JSONObject.quote("ok");
    }

    private static String error(String message) {
        return JSONObject.quote("error|" + message);
    }

    private static int dp(int value) {
        return Math.round(value * density());
    }

    private static float density() {
        return app.getResources().getDisplayMetrics().density;
    }

    /** Draws one sprite frame plus an optional speech bubble above it. */
    private static final class PetView extends View {
        private final Paint bitmapPaint = new Paint(Paint.FILTER_BITMAP_FLAG);
        private final Paint bubbleBackground = new Paint(Paint.ANTI_ALIAS_FLAG);
        private final Paint bubbleBorder = new Paint(Paint.ANTI_ALIAS_FLAG);
        private final TextPaint bubblePaint = new TextPaint(Paint.ANTI_ALIAS_FLAG);
        private final Rect src = new Rect();
        private final Rect dst = new Rect();
        private final RectF bubbleRect = new RectF();
        private final int touchSlop;
        private StaticLayout bubbleLayout;
        private int bubbleWidth;
        private float downRawX;
        private float downRawY;
        private int startPetX;
        private int startPetY;
        private boolean moved;
        private boolean longPressed;
        private final Runnable longPress = new Runnable() {
            @Override
            public void run() {
                longPressed = true;
                EVENTS.add("hide");
            }
        };

        PetView(Context context) {
            super(context);
            touchSlop = ViewConfiguration.get(context).getScaledTouchSlop();
            bubbleBackground.setColor(0xFFFFFFFF);
            bubbleBorder.setColor(0x14000000);
            bubbleBorder.setStyle(Paint.Style.STROKE);
            bubbleBorder.setStrokeWidth(Math.max(1f, density()));
            bubblePaint.setTextSize(12 * density());
        }

        void rebuildBubble() {
            bubbleLayout = null;
            bubbleWidth = 0;
            if (bubble.isEmpty()) {
                return;
            }
            bubblePaint.setColor(bubbleTone == 1 ? 0xFF1F7A3D : bubbleTone == 2 ? 0xFFB3261E : 0xFF2B2B2B);
            int pad = dp(BUBBLE_PADDING_DP);
            int maxWidth = dp(BUBBLE_MAX_WIDTH_DP);
            int textWidth = maxWidth - 2 * pad;
            int desired = (int) Math.ceil(bubblePaint.measureText(bubble)) + 2 * pad;
            bubbleWidth = Math.max(2 * pad, Math.min(desired, maxWidth));
            bubbleLayout = StaticLayout.Builder
                    .obtain(bubble, 0, bubble.length(), bubblePaint, Math.max(1, bubbleWidth - 2 * pad))
                    .setAlignment(Layout.Alignment.ALIGN_CENTER)
                    .setMaxLines(2)
                    .setEllipsize(TextUtils.TruncateAt.END)
                    .build();
        }

        @Override
        protected void onDraw(Canvas canvas) {
            super.onDraw(canvas);
            int spriteLeft = (getWidth() - spriteWidth) / 2;
            int spriteTop = getHeight() - spriteHeight;
            if (bubbleLayout != null) {
                int pad = dp(BUBBLE_PADDING_DP);
                float height = bubbleLayout.getHeight() + 2 * pad;
                float left = (getWidth() - bubbleWidth) / 2f;
                float radius = dp(10);
                bubbleRect.set(left, 0, left + bubbleWidth, height);
                canvas.drawRoundRect(bubbleRect, radius, radius, bubbleBackground);
                canvas.drawRoundRect(bubbleRect, radius, radius, bubbleBorder);
                canvas.save();
                canvas.translate(left + pad, pad);
                bubbleLayout.draw(canvas);
                canvas.restore();
            }
            if (sheet == null || sheet.isRecycled()) {
                return;
            }
            int row = dragging && dragRow >= 0 ? dragRow : baseRow;
            row = Math.min(row, sheetRows - 1);
            int frames = dragging && dragRow >= 0 ? DRAG_FRAMES : baseFrames;
            frames = Math.max(1, Math.min(frames, sheetColumns));
            int index = Math.floorMod(frameIndex, frames);
            src.set(index * FRAME_WIDTH, row * FRAME_HEIGHT,
                    (index + 1) * FRAME_WIDTH, (row + 1) * FRAME_HEIGHT);
            dst.set(spriteLeft, spriteTop, spriteLeft + spriteWidth, spriteTop + spriteHeight);
            canvas.drawBitmap(sheet, src, dst, bitmapPaint);
        }

        @Override
        public boolean onTouchEvent(MotionEvent event) {
            switch (event.getActionMasked()) {
                case MotionEvent.ACTION_DOWN:
                    downRawX = event.getRawX();
                    downRawY = event.getRawY();
                    startPetX = petX;
                    startPetY = petY;
                    moved = false;
                    longPressed = false;
                    MAIN.postDelayed(longPress, LONG_PRESS_MS);
                    return true;
                case MotionEvent.ACTION_MOVE:
                    if (longPressed) {
                        return true;
                    }
                    float dx = event.getRawX() - downRawX;
                    float dy = event.getRawY() - downRawY;
                    if (!moved && Math.hypot(dx, dy) > touchSlop) {
                        moved = true;
                        dragging = true;
                        MAIN.removeCallbacks(longPress);
                    }
                    if (moved) {
                        petX = startPetX + Math.round(dx);
                        petY = startPetY + Math.round(dy);
                        dragRow = dx >= 0 ? DRAG_RIGHT_ROW : DRAG_LEFT_ROW;
                        relayoutOnMain();
                    }
                    return true;
                case MotionEvent.ACTION_UP:
                    MAIN.removeCallbacks(longPress);
                    if (longPressed) {
                        return true;
                    }
                    if (moved) {
                        dragging = false;
                        dragRow = -1;
                        frameIndex = 0;
                        EVENTS.add("drag|" + petX + "|" + petY);
                        invalidate();
                    } else {
                        EVENTS.add("click");
                        launchApp();
                    }
                    return true;
                case MotionEvent.ACTION_CANCEL:
                    MAIN.removeCallbacks(longPress);
                    dragging = false;
                    dragRow = -1;
                    return true;
                default:
                    return false;
            }
        }
    }
}