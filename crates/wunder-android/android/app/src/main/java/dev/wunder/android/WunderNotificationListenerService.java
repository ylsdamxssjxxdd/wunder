package dev.wunder.android;

import android.app.Notification;
import android.os.Bundle;
import android.service.notification.NotificationListenerService;
import android.service.notification.StatusBarNotification;

import org.json.JSONArray;
import org.json.JSONObject;

import java.util.ArrayList;
import java.util.Arrays;
import java.util.Comparator;

/**
 * Bounded, in-memory view of the notifications the user allowed us to read.
 *
 * <p>The system binds this service only after the user enables notification
 * access for wunder. Nothing is persisted and no {@code PendingIntent} or
 * remote view is kept: entries are plain strings, capped at {@link #MAX_ENTRIES}
 * and replaced by key. The agent bridge reads this queue from a worker thread,
 * so every access is synchronized.
 */
public final class WunderNotificationListenerService extends NotificationListenerService {
    private static final int MAX_ENTRIES = 50;
    private static final int MAX_TITLE_CHARS = 200;
    private static final int MAX_TEXT_CHARS = 300;
    private static final Object LOCK = new Object();
    private static final ArrayList<Entry> entries = new ArrayList<>();
    private static volatile boolean connected;
    private static volatile WunderNotificationListenerService instance;

    public static boolean isConnected() {
        return connected && instance != null;
    }

    public static int count() {
        synchronized (LOCK) {
            return entries.size();
        }
    }

    /** Newest-first copy of the queue, already bounded. */
    public static JSONArray snapshot() {
        JSONArray output = new JSONArray();
        synchronized (LOCK) {
            for (Entry entry : entries) {
                output.put(entry.toJson());
            }
        }
        return output;
    }

    /** Cancel one notification by its key; true only when the system accepted it. */
    public static boolean dismiss(String key) {
        WunderNotificationListenerService service = instance;
        if (service == null || !connected || key == null || key.trim().isEmpty()) {
            return false;
        }
        try {
            service.cancelNotification(key);
        } catch (Throwable error) {
            return false;
        }
        synchronized (LOCK) {
            entries.removeIf(entry -> entry.key.equals(key));
        }
        return true;
    }

    @Override
    public void onCreate() {
        super.onCreate();
        instance = this;
    }

    @Override
    public void onListenerConnected() {
        super.onListenerConnected();
        instance = this;
        connected = true;
        seedFromActive();
    }

    @Override
    public void onListenerDisconnected() {
        connected = false;
        super.onListenerDisconnected();
    }

    @Override
    public void onDestroy() {
        if (instance == this) {
            instance = null;
            connected = false;
        }
        super.onDestroy();
    }

    @Override
    public void onNotificationPosted(StatusBarNotification sbn) {
        if (sbn == null) {
            return;
        }
        store(sbn);
    }

    @Override
    public void onNotificationRemoved(StatusBarNotification sbn) {
        if (sbn == null || sbn.getKey() == null) {
            return;
        }
        String key = sbn.getKey();
        synchronized (LOCK) {
            entries.removeIf(entry -> entry.key.equals(key));
        }
    }

    private void seedFromActive() {
        try {
            StatusBarNotification[] active = getActiveNotifications();
            if (active == null) {
                return;
            }
            // Ascending order so the newest entry is pushed to the front last.
            Arrays.sort(active, Comparator.comparingLong(StatusBarNotification::getPostTime));
            synchronized (LOCK) {
                entries.clear();
                for (StatusBarNotification notification : active) {
                    Entry entry = toEntry(notification);
                    if (entry != null) {
                        pushLocked(entry);
                    }
                }
            }
        } catch (Throwable ignored) {
            // Seeding is best effort; live callbacks fill the queue anyway.
        }
    }

    private void store(StatusBarNotification sbn) {
        Entry entry = toEntry(sbn);
        if (entry == null) {
            return;
        }
        synchronized (LOCK) {
            pushLocked(entry);
        }
    }

    private void pushLocked(Entry entry) {
        entries.removeIf(existing -> existing.key.equals(entry.key));
        entries.add(0, entry);
        while (entries.size() > MAX_ENTRIES) {
            entries.remove(entries.size() - 1);
        }
    }

    private Entry toEntry(StatusBarNotification sbn) {
        try {
            if (sbn.getPackageName() == null || sbn.getPackageName().equals(getPackageName())) {
                // The app's own runtime/screen notifications are noise for the agent.
                return null;
            }
            String key = sbn.getKey();
            if (key == null || key.isEmpty()) {
                return null;
            }
            Notification notification = sbn.getNotification();
            Bundle extras = notification == null ? null : notification.extras;
            String title = firstNonEmpty(
                    charSequence(extras, Notification.EXTRA_TITLE),
                    charSequence(extras, Notification.EXTRA_TITLE_BIG));
            String text = firstNonEmpty(
                    charSequence(extras, Notification.EXTRA_TEXT),
                    charSequence(extras, Notification.EXTRA_BIG_TEXT),
                    charSequence(extras, Notification.EXTRA_SUB_TEXT));
            int flags = notification == null ? 0 : notification.flags;
            boolean canDismiss = !sbn.isOngoing()
                    && (flags & (Notification.FLAG_NO_CLEAR | Notification.FLAG_ONGOING_EVENT)) == 0;
            return new Entry(
                    key,
                    sbn.getPackageName(),
                    truncate(title, MAX_TITLE_CHARS),
                    truncate(text, MAX_TEXT_CHARS),
                    sbn.getPostTime(),
                    canDismiss);
        } catch (Throwable ignored) {
            return null;
        }
    }

    private static String charSequence(Bundle extras, String key) {
        if (extras == null) {
            return "";
        }
        CharSequence value = extras.getCharSequence(key);
        return value == null ? "" : value.toString().trim();
    }

    private static String firstNonEmpty(String... values) {
        for (String value : values) {
            if (value != null && !value.isEmpty()) {
                return value;
            }
        }
        return "";
    }

    private static String truncate(String value, int maxChars) {
        if (value == null) {
            return "";
        }
        if (value.length() <= maxChars) {
            return value;
        }
        return value.substring(0, maxChars) + "…";
    }

    private static final class Entry {
        final String key;
        final String packageName;
        final String title;
        final String text;
        final long postTime;
        final boolean canDismiss;

        Entry(
                String key,
                String packageName,
                String title,
                String text,
                long postTime,
                boolean canDismiss) {
            this.key = key;
            this.packageName = packageName;
            this.title = title;
            this.text = text;
            this.postTime = postTime;
            this.canDismiss = canDismiss;
        }

        JSONObject toJson() {
            JSONObject object = new JSONObject();
            try {
                object.put("key", key);
                object.put("package", packageName);
                object.put("title", title);
                object.put("text", text);
                object.put("post_time", postTime);
                object.put("can_dismiss", canDismiss);
            } catch (Throwable ignored) {
            }
            return object;
        }
    }
}
