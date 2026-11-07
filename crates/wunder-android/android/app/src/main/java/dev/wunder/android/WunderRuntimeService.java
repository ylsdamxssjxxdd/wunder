package dev.wunder.android;

import android.Manifest;
import android.app.Activity;
import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.content.pm.ServiceInfo;
import android.os.Build;
import android.os.IBinder;
import android.os.PowerManager;
import android.util.Log;

/**
 * Keeps the agent runtime alive while no activity exists.
 *
 * <p>The engine lives in the process, not in a window: a turn that is running
 * when the user leaves the app must keep streaming. An activity cannot provide
 * that guarantee, so the runtime runs under a foreground service with a
 * low-importance notification. The wake lock is held only while a turn is
 * actually running, not while the app is idle.
 */
public final class WunderRuntimeService extends Service {
    private static final String TAG = "wunder";
    private static final String CHANNEL_ID = "wunder-runtime";
    private static final String ACTION_START = "dev.wunder.android.START";
    private static final String ACTION_STATE = "dev.wunder.android.STATE";
    private static final String EXTRA_BUSY = "busy";
    private static final String EXTRA_TITLE = "title";
    private static final int NOTIFICATION_ID = 4101;
    private static final int PERMISSION_REQUEST = 4102;

    private boolean busy;
    private String title = "";
    private PowerManager.WakeLock wakeLock;

    /** Start the runtime service, or refresh its notification if it already runs. */
    public static void start(Context context) {
        Intent intent = new Intent(context, WunderRuntimeService.class).setAction(ACTION_START);
        startCompat(context, intent);
    }

    /** Reflect the engine's activity into the notification and wake lock. */
    public static void setBusy(Context context, boolean busy, String title) {
        Intent intent = new Intent(context, WunderRuntimeService.class)
                .setAction(ACTION_STATE)
                .putExtra(EXTRA_BUSY, busy)
                .putExtra(EXTRA_TITLE, title == null ? "" : title);
        startCompat(context, intent);
    }

    private static void startCompat(Context context, Intent intent) {
        try {
            if (Build.VERSION.SDK_INT >= 26) {
                context.startForegroundService(intent);
            } else {
                context.startService(intent);
            }
        } catch (Throwable error) {
            // Losing the notification must never fail the turn itself.
            Log.w(TAG, "runtime service start failed: " + error);
        }
    }

    /** Ask for the notification permission once per process on Android 13+. */
    public static void requestNotificationPermission(Activity activity) {
        if (Build.VERSION.SDK_INT < 33) {
            return;
        }
        if (activity.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS)
                == PackageManager.PERMISSION_GRANTED) {
            return;
        }
        activity.runOnUiThread(() -> {
            try {
                activity.requestPermissions(
                        new String[]{Manifest.permission.POST_NOTIFICATIONS}, PERMISSION_REQUEST);
            } catch (Throwable ignored) {
                // A missing permission only hides the notification text.
            }
        });
    }

    @Override
    public void onCreate() {
        super.onCreate();
        createChannel();
        PowerManager manager = (PowerManager) getSystemService(POWER_SERVICE);
        if (manager != null) {
            wakeLock = manager.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "wunder:agent");
            wakeLock.setReferenceCounted(false);
        }
        startForegroundCompat();
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        if (intent != null && ACTION_STATE.equals(intent.getAction())) {
            busy = intent.getBooleanExtra(EXTRA_BUSY, busy);
            String next = intent.getStringExtra(EXTRA_TITLE);
            title = next == null ? "" : next;
            applyState();
        } else {
            startForegroundCompat();
        }
        return START_STICKY;
    }

    @Override
    public void onTaskRemoved(Intent rootIntent) {
        // Removing the task while a turn runs is exactly the case the service
        // exists for: keep streaming and let the notification bring the user
        // back. Once idle, release the process and its notification.
        if (!busy) {
            stopSelf();
        }
        super.onTaskRemoved(rootIntent);
    }

    @Override
    public void onDestroy() {
        releaseWakeLock();
        super.onDestroy();
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }

    private void applyState() {
        if (busy) {
            acquireWakeLock();
        } else {
            releaseWakeLock();
        }
        startForegroundCompat();
    }

    private void acquireWakeLock() {
        try {
            if (wakeLock != null && !wakeLock.isHeld()) {
                // No timeout: the process death releases the lock. Turns can
                // legitimately run longer than a fixed window while the screen
                // is off, and a timed lock would silently stop the CPU.
                wakeLock.acquire();
            }
        } catch (Throwable error) {
            Log.w(TAG, "wake lock unavailable: " + error);
        }
    }

    private void releaseWakeLock() {
        try {
            if (wakeLock != null && wakeLock.isHeld()) {
                wakeLock.release();
            }
        } catch (Throwable ignored) {
        }
    }

    private void startForegroundCompat() {
        Notification notification = buildNotification();
        try {
            if (Build.VERSION.SDK_INT >= 34) {
                startForeground(
                        NOTIFICATION_ID,
                        notification,
                        ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE);
            } else {
                startForeground(NOTIFICATION_ID, notification);
            }
        } catch (Throwable error) {
            Log.w(TAG, "foreground notification failed: " + error);
        }
    }

    private Notification buildNotification() {
        Notification.Builder builder = new Notification.Builder(this, CHANNEL_ID)
                .setSmallIcon(android.R.drawable.stat_notify_sync)
                .setContentTitle(busy ? "茧正在执行任务" : "茧")
                .setContentText(busy && !title.isEmpty() ? title : "待机中，点击返回")
                .setContentIntent(launchIntent())
                .setCategory(Notification.CATEGORY_SERVICE)
                .setVisibility(Notification.VISIBILITY_PRIVATE)
                .setOngoing(busy)
                .setOnlyAlertOnce(true)
                .setShowWhen(false);
        return builder.build();
    }

    private PendingIntent launchIntent() {
        Intent launch = getPackageManager().getLaunchIntentForPackage(getPackageName());
        if (launch == null) {
            launch = new Intent(this, android.app.NativeActivity.class);
        }
        launch.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_SINGLE_TOP);
        int flags = PendingIntent.FLAG_UPDATE_CURRENT;
        if (Build.VERSION.SDK_INT >= 23) {
            flags |= PendingIntent.FLAG_IMMUTABLE;
        }
        return PendingIntent.getActivity(this, 0, launch, flags);
    }

    private void createChannel() {
        NotificationManager manager =
                (NotificationManager) getSystemService(NOTIFICATION_SERVICE);
        if (manager == null) {
            return;
        }
        NotificationChannel channel = new NotificationChannel(
                CHANNEL_ID, "运行状态", NotificationManager.IMPORTANCE_LOW);
        channel.setDescription("任务执行进度与后台运行状态");
        channel.setShowBadge(false);
        channel.setSound(null, null);
        channel.enableVibration(false);
        manager.createNotificationChannel(channel);
    }
}
