package dev.wunder.android;

import android.content.Context;

import org.json.JSONArray;
import org.json.JSONObject;

/**
 * Device-side executor for the runtime's android_notifications tool.
 *
 * <p>All arguments and results are JSON values. A missing notification-access
 * grant returns a stable {@code notification_access_required} code so the agent
 * can guide the user to system settings instead of retrying the same call.
 */
final class NotificationBridge {
    private static final int MAX_LIMIT = 50;
    private static final int DEFAULT_LIMIT = 20;

    private NotificationBridge() {
    }

    static String execute(Context context, String action, String argsJson) {
        JSONObject result = new JSONObject();
        try {
            JSONObject args = argsJson == null || argsJson.isEmpty()
                    ? new JSONObject()
                    : new JSONObject(argsJson);
            boolean ok = true;
            String code = "";
            String message = "";
            JSONObject data = new JSONObject();
            String name = action == null ? "" : action.trim();

            switch (name) {
                case "list": {
                    if (!WunderNotificationListenerService.isConnected()) {
                        ok = false;
                        code = "notification_access_required";
                        message = "Notification access is not granted.";
                        break;
                    }
                    int limit = clamp(args.optInt("limit", DEFAULT_LIMIT), 1, MAX_LIMIT);
                    JSONArray all = WunderNotificationListenerService.snapshot();
                    JSONArray page = new JSONArray();
                    for (int index = 0; index < all.length() && index < limit; index++) {
                        page.put(all.get(index));
                    }
                    data.put("notifications", page);
                    data.put("count", page.length());
                    data.put("total", all.length());
                    break;
                }
                case "dismiss": {
                    if (!WunderNotificationListenerService.isConnected()) {
                        ok = false;
                        code = "notification_access_required";
                        message = "Notification access is not granted.";
                        break;
                    }
                    String key = args.optString("key", "").trim();
                    if (key.isEmpty()) {
                        ok = false;
                        code = "missing_key";
                        message = "A notification key is required.";
                        break;
                    }
                    boolean dismissed = WunderNotificationListenerService.dismiss(key);
                    if (!dismissed) {
                        ok = false;
                        code = "dismiss_failed";
                        message = "The notification could not be dismissed.";
                        break;
                    }
                    data.put("key", key);
                    data.put("dismissed", true);
                    break;
                }
                case "status": {
                    data.put("connected", WunderNotificationListenerService.isConnected());
                    data.put("count", WunderNotificationListenerService.count());
                    break;
                }
                default:
                    ok = false;
                    code = "unknown_action";
                    message = "Unknown notification action: " + name;
                    break;
            }

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
            }
        }
        return result.toString();
    }

    private static int clamp(int value, int min, int max) {
        return Math.max(min, Math.min(max, value));
    }
}
