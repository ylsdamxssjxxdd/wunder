package dev.wunder.android;

import android.accessibilityservice.AccessibilityService;
import android.accessibilityservice.GestureDescription;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.content.pm.PackageManager;
import android.content.pm.ResolveInfo;
import android.graphics.Bitmap;
import android.graphics.Path;
import android.graphics.Rect;
import android.net.ConnectivityManager;
import android.net.NetworkCapabilities;
import android.net.Uri;
import android.os.BatteryManager;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.os.StatFs;
import android.provider.Settings;
import android.view.Display;
import android.view.WindowManager;
import android.view.accessibility.AccessibilityNodeInfo;
import android.util.DisplayMetrics;

import org.json.JSONArray;
import org.json.JSONObject;

import java.io.File;
import java.io.FileOutputStream;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Collections;
import java.util.Comparator;
import java.util.HashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.concurrent.Callable;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.FutureTask;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicReference;

/**
 * Stateless device executor called from Rust over JNI.
 *
 * <p>Every method returns a JSON object. Failures are values, not exceptions:
 * the agent needs the reason (for example a missing accessibility connection)
 * so it can ask the user to act instead of retrying blindly.
 */
final class PhoneBridge {
    private static final int MAX_TREE_NODES = 400;
    private static final int MAX_TREE_CHARS = 8000;
    private static final int KEEP_SCREENSHOTS = 4;

    /** Display size of the last captured frame; gesture coordinates are
     * normalized against the frame the model actually saw. */
    private static volatile int[] lastFrameSize;

    private PhoneBridge() {
    }

    static String execute(Context context, String action, String argsJson) {
        JSONObject result = new JSONObject();
        try {
            JSONObject args = argsJson == null || argsJson.isEmpty()
                    ? new JSONObject()
                    : new JSONObject(argsJson);
            WunderAccessibilityService service = WunderAccessibilityService.getInstance();
            boolean connected = service != null;
            int[] size = screenSize(context);
            result.put("accessibility_connected", connected);
            result.put("screen_width", size[0]);
            result.put("screen_height", size[1]);
            result.put("settled", true);

            boolean ok = true;
            String code = "";
            String message = "";
            JSONObject data = new JSONObject();

            switch (action) {
                case "screenshot":
                case "ui_tree":
                    ok = requireService(result, service);
                    break;
                case "tap":
                    if (requireService(result, service)) {
                        int[] point = mapPoint(context, args);
                        ok = tap(service, point[0], point[1], 80);
                        sleep(250);
                    } else {
                        ok = false;
                    }
                    break;
                case "long_press":
                    if (requireService(result, service)) {
                        int[] point = mapPoint(context, args);
                        int duration = args.optInt("duration_ms", 650);
                        ok = tap(service, point[0], point[1], Math.max(500, duration));
                        sleep(Math.max(500, duration) + 100);
                    } else {
                        ok = false;
                    }
                    break;
                case "swipe":
                    if (requireService(result, service)) {
                        int[] from = mapPointXY(context, args.optInt("x1", 0), args.optInt("y1", 0));
                        int[] to = mapPointXY(context, args.optInt("x2", 0), args.optInt("y2", 0));
                        int duration = Math.max(80, args.optInt("duration_ms", 300));
                        ok = swipe(service, from[0], from[1], to[0], to[1], duration);
                        sleep(duration + 120);
                    } else {
                        ok = false;
                    }
                    break;
                case "type_text":
                    if (requireService(result, service)) {
                        if (args.has("x") && args.has("y")) {
                            int[] point = mapPoint(context, args);
                            tap(service, point[0], point[1], 80);
                            sleep(320);
                        }
                        CodeOutcome outcome = typeText(service, args.optString("text", ""));
                        ok = outcome.ok;
                        code = outcome.code;
                        message = outcome.message;
                        if (ok) {
                            sleep(280);
                        }
                    } else {
                        ok = false;
                    }
                    break;
                case "press_key":
                    if (requireService(result, service)) {
                        ok = globalAction(service, args.optString("key", ""));
                        sleep(350);
                    } else {
                        ok = false;
                    }
                    break;
                case "open_app": {
                    CodeOutcome outcome = openApp(context, args.optString("app", ""));
                    ok = outcome.ok;
                    code = outcome.code;
                    message = outcome.message;
                    if (ok) {
                        data.put("app", outcome.extra);
                        sleep(1000);
                    }
                    break;
                }
                case "open_url": {
                    CodeOutcome outcome = openUrl(context, args.optString("url", ""));
                    ok = outcome.ok;
                    code = outcome.code;
                    message = outcome.message;
                    if (ok) {
                        sleep(900);
                    }
                    break;
                }
                case "open_settings": {
                    CodeOutcome outcome = openSettings(context, args.optString("page", "settings"));
                    ok = outcome.ok;
                    code = outcome.code;
                    message = outcome.message;
                    if (ok) {
                        sleep(700);
                    }
                    break;
                }
                case "list_apps":
                    data.put("apps", listApps(context));
                    break;
                case "device_info":
                    data = deviceInfo(context, service);
                    break;
                case "clipboard": {
                    CodeOutcome outcome = clipboard(context, args);
                    ok = outcome.ok;
                    code = outcome.code;
                    message = outcome.message;
                    if (outcome.extra != null) {
                        data.put("text", outcome.extra);
                    }
                    break;
                }
                case "wait": {
                    int delay = Math.min(30000, Math.max(0, args.optInt("delay_ms", 0)));
                    sleep(delay);
                    break;
                }
                default:
                    ok = false;
                    code = "unknown_action";
                    message = "Unknown phone action: " + action;
                    break;
            }

            if (ok && capturesFrame(action)) {
                Frame frame = captureFrame(context, service);
                if (frame != null) {
                    result.put("screenshot_path", frame.path);
                    if (frame.tree != null && !frame.tree.isEmpty()) {
                        result.put("ui_tree", frame.tree);
                    }
                } else if (connected) {
                    result.put("settled", false);
                    if (Build.VERSION.SDK_INT < 30) {
                        code = "screenshot_unsupported";
                        message = "Screenshots need Android 11 or newer.";
                    }
                }
            }

            // Report the foreground app after the action, so open_app and
            // open_settings describe the screen the returned frame shows.
            result.put("foreground_app", connected ? foregroundPackage(service) : "");
            result.put("ok", ok);
            if (!code.isEmpty()) {
                result.put("code", code);
            }
            if (!message.isEmpty()) {
                result.put("message", message);
            }
            if (data.length() > 0) {
                result.put("data", data);
            }
        } catch (Throwable error) {
            try {
                result.put("ok", false);
                result.put("code", "internal_error");
                result.put("message", String.valueOf(error));
            } catch (Throwable ignored) {
                // JSONObject only throws on invalid values; Strings cannot.
            }
        }
        return result.toString();
    }

    private static boolean capturesFrame(String action) {
        return !("device_info".equals(action)
                || "list_apps".equals(action)
                || "clipboard".equals(action));
    }

    private static boolean requireService(JSONObject result, WunderAccessibilityService service) {
        if (service != null) {
            return true;
        }
        try {
            result.put("code", "accessibility_disabled");
            result.put("message", "The accessibility service is not connected.");
        } catch (Throwable ignored) {
        }
        return false;
    }

    // ---- gestures -----------------------------------------------------------

    private static boolean tap(AccessibilityService service, int x, int y, int durationMs) {
        Path path = new Path();
        path.moveTo(x, y);
        return dispatch(service, path, durationMs);
    }

    private static boolean swipe(
            AccessibilityService service, int x1, int y1, int x2, int y2, int durationMs) {
        Path path = new Path();
        path.moveTo(x1, y1);
        path.lineTo(x2, y2);
        return dispatch(service, path, durationMs);
    }

    private static boolean dispatch(AccessibilityService service, Path path, int durationMs) {
        Boolean applied = onMain(() -> service.dispatchGesture(
                new GestureDescription.Builder()
                        .addStroke(new GestureDescription.StrokeDescription(
                                path, 0, Math.max(20, durationMs)))
                        .build(),
                null,
                null), 3000);
        return Boolean.TRUE.equals(applied);
    }

    private static int[] mapPoint(Context context, JSONObject args) {
        return mapPointXY(context, args.optInt("x", 0), args.optInt("y", 0));
    }

    private static int[] mapPointXY(Context context, int normX, int normY) {
        int[] size = lastFrameSize != null ? lastFrameSize : screenSize(context);
        int x = clamp(normX, 0, 1000) * Math.max(0, size[0] - 1) / 1000;
        int y = clamp(normY, 0, 1000) * Math.max(0, size[1] - 1) / 1000;
        return new int[]{x, y};
    }

    private static int clamp(int value, int min, int max) {
        return Math.max(min, Math.min(max, value));
    }

    // ---- text input ---------------------------------------------------------

    private static CodeOutcome typeText(AccessibilityService service, String text) {
        // Tapping a search entry often launches a new activity: the field can
        // appear a few hundred milliseconds later. Retry briefly instead of
        // failing the turn on the first frame.
        boolean sawField = false;
        for (int attempt = 0; attempt < 10; attempt++) {
            String outcome = onMain(() -> {
                AccessibilityNodeInfo root = service.getRootInActiveWindow();
                if (root == null) {
                    return "no_root";
                }
                AccessibilityNodeInfo target = root.findFocus(AccessibilityNodeInfo.FOCUS_INPUT);
                if (target == null || !target.isEditable()) {
                    target = firstEditable(root);
                }
                if (target == null) {
                    return "no_editable_field";
                }
                target.performAction(AccessibilityNodeInfo.ACTION_FOCUS);
                Bundle bundle = new Bundle();
                bundle.putCharSequence(
                        AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, text);
                boolean ok = target.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, bundle);
                return ok ? null : "set_text_rejected";
            }, 4000);
            if (outcome == null) {
                return CodeOutcome.ok();
            }
            if (!"no_editable_field".equals(outcome) && !"no_root".equals(outcome)) {
                sawField = true;
            }
            sleep(150);
        }
        if (!sawField) {
            return CodeOutcome.fail("no_editable_field", "No editable text field appeared.");
        }
        return CodeOutcome.fail("input_failed", "The text field rejected the input.");
    }

    private static AccessibilityNodeInfo firstEditable(AccessibilityNodeInfo root) {
        ArrayDeque<AccessibilityNodeInfo> queue = new ArrayDeque<>();
        queue.add(root);
        int visited = 0;
        while (!queue.isEmpty() && visited < 300) {
            AccessibilityNodeInfo node = queue.removeFirst();
            visited++;
            if (node.isEditable() && node.isEnabled()) {
                return node;
            }
            for (int index = 0; index < node.getChildCount(); index++) {
                AccessibilityNodeInfo child = node.getChild(index);
                if (child != null) {
                    queue.add(child);
                }
            }
        }
        return null;
    }

    // ---- system actions -----------------------------------------------------

    private static boolean globalAction(AccessibilityService service, String key) {
        int action;
        switch (key) {
            case "back":
                action = AccessibilityService.GLOBAL_ACTION_BACK;
                break;
            case "home":
                action = AccessibilityService.GLOBAL_ACTION_HOME;
                break;
            case "recents":
                action = AccessibilityService.GLOBAL_ACTION_RECENTS;
                break;
            case "notifications":
                action = AccessibilityService.GLOBAL_ACTION_NOTIFICATIONS;
                break;
            case "quick_settings":
                action = AccessibilityService.GLOBAL_ACTION_QUICK_SETTINGS;
                break;
            default:
                return false;
        }
        Boolean applied = onMain(() -> service.performGlobalAction(action), 2000);
        return Boolean.TRUE.equals(applied);
    }

    private static CodeOutcome openApp(Context context, String target) {
        String wanted = target == null ? "" : target.trim();
        if (wanted.isEmpty()) {
            return CodeOutcome.fail("app_not_found", "No app name was provided.");
        }
        PackageManager pm = context.getPackageManager();
        Intent launch = null;
        String label = wanted;
        try {
            launch = pm.getLaunchIntentForPackage(wanted);
            if (launch != null) {
                label = String.valueOf(pm.getApplicationLabel(pm.getApplicationInfo(wanted, 0)));
            }
        } catch (Throwable ignored) {
            launch = null;
        }
        if (launch == null) {
            String needle = normalize(wanted);
            for (AppEntry entry : launchableApps(pm)) {
                String byPackage = normalize(entry.packageName);
                String byLabel = normalize(entry.label);
                if (byPackage.equals(needle) || byLabel.equals(needle)) {
                    launch = new Intent(Intent.ACTION_MAIN)
                            .addCategory(Intent.CATEGORY_LAUNCHER)
                            .setClassName(entry.packageName, entry.activityName);
                    label = entry.label;
                    break;
                }
            }
        }
        if (launch == null) {
            for (AppEntry entry : launchableApps(pm)) {
                if (normalize(entry.label).contains(normalize(wanted))) {
                    launch = new Intent(Intent.ACTION_MAIN)
                            .addCategory(Intent.CATEGORY_LAUNCHER)
                            .setClassName(entry.packageName, entry.activityName);
                    label = entry.label;
                    break;
                }
            }
        }
        if (launch == null) {
            return CodeOutcome.fail("app_not_found", "No launchable app matches " + wanted + ".");
        }
        launch.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
        try {
            context.startActivity(launch);
            return CodeOutcome.ok(label);
        } catch (Throwable error) {
            return CodeOutcome.fail("app_launch_failed", String.valueOf(error));
        }
    }

    private static CodeOutcome openUrl(Context context, String raw) {
        String url = raw == null ? "" : raw.trim();
        if (!url.startsWith("https://") && !url.startsWith("http://")) {
            return CodeOutcome.fail("invalid_url", "Only http/https URLs are allowed.");
        }
        try {
            Intent intent = new Intent(Intent.ACTION_VIEW, Uri.parse(url));
            intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            context.startActivity(intent);
            return CodeOutcome.ok();
        } catch (Throwable error) {
            return CodeOutcome.fail("url_open_failed", String.valueOf(error));
        }
    }

    private static CodeOutcome openSettings(Context context, String page) {
        String action;
        switch (page == null ? "" : page) {
            case "wifi":
                action = Settings.ACTION_WIFI_SETTINGS;
                break;
            case "bluetooth":
                action = Settings.ACTION_BLUETOOTH_SETTINGS;
                break;
            case "display":
                action = Settings.ACTION_DISPLAY_SETTINGS;
                break;
            case "sound":
                action = Settings.ACTION_SOUND_SETTINGS;
                break;
            case "battery":
                action = Settings.ACTION_BATTERY_SAVER_SETTINGS;
                break;
            case "apps":
                action = Settings.ACTION_APPLICATION_SETTINGS;
                break;
            case "accessibility":
                action = Settings.ACTION_ACCESSIBILITY_SETTINGS;
                break;
            case "storage":
                action = Settings.ACTION_INTERNAL_STORAGE_SETTINGS;
                break;
            case "location":
                action = Settings.ACTION_LOCATION_SOURCE_SETTINGS;
                break;
            case "settings":
            default:
                action = Settings.ACTION_SETTINGS;
                break;
        }
        try {
            Intent intent = new Intent(action);
            intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            context.startActivity(intent);
            return CodeOutcome.ok();
        } catch (Throwable error) {
            return CodeOutcome.fail("settings_open_failed", String.valueOf(error));
        }
    }

    // ---- clipboard ----------------------------------------------------------

    private static CodeOutcome clipboard(Context context, JSONObject args) {
        ClipboardManager manager =
                (ClipboardManager) context.getSystemService(Context.CLIPBOARD_SERVICE);
        if (manager == null) {
            return CodeOutcome.fail("clipboard_unavailable", "Clipboard service is unavailable.");
        }
        String operation = args.optString("operation", "get");
        if ("set".equals(operation)) {
            String text = args.optString("text", "");
            if (text.isEmpty()) {
                return CodeOutcome.fail("invalid_target", "Clipboard text is empty.");
            }
            manager.setPrimaryClip(ClipData.newPlainText("wunder", text));
            return CodeOutcome.ok();
        }
        try {
            ClipData clip = manager.getPrimaryClip();
            if (clip == null || clip.getItemCount() == 0) {
                return CodeOutcome.ok("");
            }
            CharSequence text = clip.getItemAt(0).coerceToText(context);
            return CodeOutcome.ok(text == null ? "" : text.toString());
        } catch (Throwable error) {
            return CodeOutcome.fail("clipboard_unavailable", String.valueOf(error));
        }
    }

    // ---- inventory and status ----------------------------------------------

    private static JSONArray listApps(Context context) {
        JSONArray apps = new JSONArray();
        List<AppEntry> entries = launchableApps(context.getPackageManager());
        Collections.sort(
                entries, Comparator.comparing((AppEntry entry) -> entry.label.toLowerCase(Locale.ROOT)));
        int limit = Math.min(entries.size(), 200);
        for (int index = 0; index < limit; index++) {
            AppEntry entry = entries.get(index);
            JSONObject item = new JSONObject();
            try {
                item.put("label", entry.label);
                item.put("package", entry.packageName);
                apps.put(item);
            } catch (Throwable ignored) {
            }
        }
        return apps;
    }

    private static List<AppEntry> launchableApps(PackageManager pm) {
        Intent intent = new Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER);
        List<ResolveInfo> resolved = pm.queryIntentActivities(intent, 0);
        Map<String, AppEntry> deduped = new HashMap<>();
        for (ResolveInfo info : resolved) {
            if (info.activityInfo == null) {
                continue;
            }
            String packageName = info.activityInfo.packageName;
            if (packageName == null || deduped.containsKey(packageName)) {
                continue;
            }
            String label;
            try {
                label = String.valueOf(info.loadLabel(pm));
            } catch (Throwable ignored) {
                label = packageName;
            }
            deduped.put(packageName, new AppEntry(label, packageName, info.activityInfo.name));
        }
        return new ArrayList<>(deduped.values());
    }

    private static JSONObject deviceInfo(Context context, WunderAccessibilityService service)
            throws Exception {
        JSONObject info = new JSONObject();
        info.put("manufacturer", Build.MANUFACTURER);
        info.put("model", Build.MODEL);
        info.put("android", Build.VERSION.RELEASE);
        info.put("sdk", Build.VERSION.SDK_INT);
        DisplayMetrics metrics = context.getResources().getDisplayMetrics();
        info.put("density_dpi", metrics.densityDpi);
        info.put("rotation", rotation(context));
        try {
            Intent battery = context.registerReceiver(
                    null, new IntentFilter(Intent.ACTION_BATTERY_CHANGED));
            if (battery != null) {
                int level = battery.getIntExtra(BatteryManager.EXTRA_LEVEL, -1);
                int scale = battery.getIntExtra(BatteryManager.EXTRA_SCALE, -1);
                info.put("battery_percent", level >= 0 && scale > 0 ? level * 100 / scale : -1);
                int status = battery.getIntExtra(BatteryManager.EXTRA_STATUS, -1);
                info.put("charging", status == BatteryManager.BATTERY_STATUS_CHARGING
                        || status == BatteryManager.BATTERY_STATUS_FULL);
            }
        } catch (Throwable ignored) {
        }
        info.put("network", networkType(context));
        try {
            StatFs stat = new StatFs(context.getFilesDir().getAbsolutePath());
            info.put("storage_total_mb", stat.getTotalBytes() / (1024 * 1024));
            info.put("storage_free_mb", stat.getAvailableBytes() / (1024 * 1024));
        } catch (Throwable ignored) {
        }
        info.put("locale", Locale.getDefault().toString());
        try {
            info.put("app_version",
                    context.getPackageManager().getPackageInfo(context.getPackageName(), 0).versionName);
        } catch (Throwable ignored) {
        }
        info.put("accessibility_connected", service != null);
        return info;
    }

    private static String networkType(Context context) {
        try {
            ConnectivityManager manager =
                    (ConnectivityManager) context.getSystemService(Context.CONNECTIVITY_SERVICE);
            if (manager == null) {
                return "unknown";
            }
            NetworkCapabilities caps = manager.getNetworkCapabilities(manager.getActiveNetwork());
            if (caps == null) {
                return "offline";
            }
            if (caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI)) {
                return "wifi";
            }
            if (caps.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR)) {
                return "cellular";
            }
            if (caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET)) {
                return "ethernet";
            }
            if (caps.hasTransport(NetworkCapabilities.TRANSPORT_VPN)) {
                return "vpn";
            }
            return "other";
        } catch (Throwable ignored) {
            return "unknown";
        }
    }

    // ---- accessibility tree -------------------------------------------------

    private static String tree(AccessibilityService service) {
        String value = onMain(() -> {
            AccessibilityNodeInfo root = service.getRootInActiveWindow();
            if (root == null) {
                return "";
            }
            StringBuilder out = new StringBuilder();
            walk(root, out, 0, new int[]{0});
            return out.toString();
        }, 4000);
        return value == null ? "" : value;
    }

    private static void walk(AccessibilityNodeInfo node, StringBuilder out, int depth, int[] count) {
        if (node == null || count[0] >= MAX_TREE_NODES || out.length() >= MAX_TREE_CHARS || depth > 24) {
            return;
        }
        count[0]++;
        if (node.isVisibleToUser()) {
            CharSequence text = node.getText();
            CharSequence hint = node.getHintText();
            CharSequence description = node.getContentDescription();
            boolean clickable = node.isClickable();
            boolean editable = node.isEditable();
            boolean scrollable = node.isScrollable();
            boolean focused = node.isFocused();
            boolean interesting = clickable || editable || scrollable || focused
                    || hasText(text) || hasText(hint) || hasText(description);
            if (interesting) {
                Rect bounds = new Rect();
                node.getBoundsInScreen(bounds);
                out.append('#').append(count[0])
                        .append(' ').append(simpleName(node.getClassName()));
                if (hasText(text)) {
                    out.append(" text=\"").append(escape(text)).append('"');
                }
                if (hasText(hint)) {
                    out.append(" hint=\"").append(escape(hint)).append('"');
                }
                if (hasText(description)) {
                    out.append(" desc=\"").append(escape(description)).append('"');
                }
                out.append(" [").append(bounds.left).append(',').append(bounds.top)
                        .append("][").append(bounds.right).append(',').append(bounds.bottom).append(']');
                if (clickable) {
                    out.append(" clickable");
                }
                if (editable) {
                    out.append(" editable");
                }
                if (scrollable) {
                    out.append(" scrollable");
                }
                if (focused) {
                    out.append(" focused");
                }
                out.append('\n');
            }
        }
        for (int index = 0; index < node.getChildCount(); index++) {
            if (count[0] >= MAX_TREE_NODES || out.length() >= MAX_TREE_CHARS) {
                break;
            }
            walk(node.getChild(index), out, depth + 1, count);
        }
    }

    private static boolean hasText(CharSequence value) {
        return value != null && value.toString().trim().length() > 0;
    }

    private static String escape(CharSequence value) {
        String text = value.toString().replace('\n', ' ').replace('\r', ' ');
        if (text.length() > 80) {
            text = text.substring(0, 80) + "…";
        }
        return text.replace("\\", "\\\\").replace("\"", "\\\"");
    }

    private static String simpleName(CharSequence className) {
        String value = className == null ? "" : className.toString();
        int dot = value.lastIndexOf('.');
        return dot >= 0 ? value.substring(dot + 1) : value;
    }

    private static String foregroundPackage(AccessibilityService service) {
        String value = onMain(() -> {
            AccessibilityNodeInfo root = service.getRootInActiveWindow();
            if (root == null) {
                return "";
            }
            CharSequence packageName = root.getPackageName();
            return packageName == null ? "" : packageName.toString();
        }, 2000);
        return value == null ? "" : value;
    }

    // ---- screenshots --------------------------------------------------------

    private static Frame captureFrame(Context context, WunderAccessibilityService service) {
        if (service == null) {
            return null;
        }
        String tree = tree(service);
        String path = ScreenshotApi30.capture(context, service);
        if (path == null) {
            return null;
        }
        return new Frame(path, tree);
    }

    /**
     * Screenshot calls live in a nested class so the API 30 types are only
     * verified when this class is actually loaded on Android 11+.
     */
    private static final class ScreenshotApi30 {
        static String capture(Context context, AccessibilityService service) {
            if (Build.VERSION.SDK_INT < 30) {
                return null;
            }
            CountDownLatch latch = new CountDownLatch(1);
            AtomicReference<Bitmap> bitmap = new AtomicReference<>();
            Handler main = new Handler(Looper.getMainLooper());
            main.post(() -> {
                try {
                    service.takeScreenshot(
                            Display.DEFAULT_DISPLAY,
                            command -> main.post(command),
                            new AccessibilityService.TakeScreenshotCallback() {
                                @Override
                                public void onSuccess(AccessibilityService.ScreenshotResult result) {
                                    try {
                                        Bitmap wrapped = Bitmap.wrapHardwareBuffer(
                                                result.getHardwareBuffer(), result.getColorSpace());
                                        if (wrapped != null) {
                                            bitmap.set(wrapped.copy(Bitmap.Config.ARGB_8888, false));
                                        }
                                    } finally {
                                        result.getHardwareBuffer().close();
                                    }
                                    latch.countDown();
                                }

                                @Override
                                public void onFailure(int errorCode) {
                                    latch.countDown();
                                }
                            });
                } catch (Throwable error) {
                    latch.countDown();
                }
            });
            try {
                if (!latch.await(4, TimeUnit.SECONDS)) {
                    return null;
                }
            } catch (InterruptedException interrupted) {
                Thread.currentThread().interrupt();
                return null;
            }
            Bitmap shot = bitmap.get();
            if (shot == null) {
                return null;
            }
            File directory = new File(context.getCacheDir(), "phone");
            if (!directory.exists() && !directory.mkdirs()) {
                shot.recycle();
                return null;
            }
            File file = new File(directory, "shot-" + System.currentTimeMillis() + ".png");
            try (FileOutputStream out = new FileOutputStream(file)) {
                shot.compress(Bitmap.CompressFormat.PNG, 100, out);
                lastFrameSize = new int[]{shot.getWidth(), shot.getHeight()};
                prune(directory);
                return file.getAbsolutePath();
            } catch (Throwable error) {
                return null;
            } finally {
                shot.recycle();
            }
        }

        private static void prune(File directory) {
            File[] files = directory.listFiles((dir, name) -> name.startsWith("shot-"));
            if (files == null || files.length <= KEEP_SCREENSHOTS) {
                return;
            }
            java.util.Arrays.sort(files, Comparator.comparingLong(File::lastModified));
            int remove = files.length - KEEP_SCREENSHOTS;
            for (int index = 0; index < remove; index++) {
                // Best effort: a frame still open for reading is simply kept.
                files[index].delete();
            }
        }
    }

    // ---- helpers ------------------------------------------------------------

    private static int[] screenSize(Context context) {
        if (Build.VERSION.SDK_INT >= 30) {
            return MetricsApi30.windowSize(context);
        }
        DisplayMetrics metrics = new DisplayMetrics();
        WindowManager manager = (WindowManager) context.getSystemService(Context.WINDOW_SERVICE);
        if (manager == null) {
            return new int[]{0, 0};
        }
        manager.getDefaultDisplay().getRealMetrics(metrics);
        return new int[]{metrics.widthPixels, metrics.heightPixels};
    }

    private static final class MetricsApi30 {
        static int[] windowSize(Context context) {
            try {
                WindowManager manager =
                        (WindowManager) context.getSystemService(Context.WINDOW_SERVICE);
                if (manager == null) {
                    return new int[]{0, 0};
                }
                android.graphics.Rect bounds = manager.getCurrentWindowMetrics().getBounds();
                return new int[]{bounds.width(), bounds.height()};
            } catch (Throwable ignored) {
                return new int[]{0, 0};
            }
        }
    }

    private static int rotation(Context context) {
        try {
            WindowManager manager = (WindowManager) context.getSystemService(Context.WINDOW_SERVICE);
            return manager == null ? 0 : manager.getDefaultDisplay().getRotation();
        } catch (Throwable ignored) {
            return 0;
        }
    }

    private static String normalize(String value) {
        return value == null ? "" : value.trim().toLowerCase(Locale.ROOT).replace(" ", "");
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
            Thread.sleep(Math.min(millis, 30000));
        } catch (InterruptedException interrupted) {
            Thread.currentThread().interrupt();
        }
    }

    private static final class AppEntry {
        final String label;
        final String packageName;
        final String activityName;

        AppEntry(String label, String packageName, String activityName) {
            this.label = label;
            this.packageName = packageName;
            this.activityName = activityName;
        }
    }

    private static final class Frame {
        final String path;
        final String tree;

        Frame(String path, String tree) {
            this.path = path;
            this.tree = tree;
        }
    }

    private static final class CodeOutcome {
        final boolean ok;
        final String code;
        final String message;
        final String extra;

        private CodeOutcome(boolean ok, String code, String message, String extra) {
            this.ok = ok;
            this.code = code;
            this.message = message;
            this.extra = extra;
        }

        static CodeOutcome ok() {
            return new CodeOutcome(true, "", "", null);
        }

        static CodeOutcome ok(String extra) {
            return new CodeOutcome(true, "", "", extra);
        }

        static CodeOutcome fail(String code, String message) {
            return new CodeOutcome(false, code, message, null);
        }
    }
}
