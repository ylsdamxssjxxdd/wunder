package dev.wunder.android;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Context;
import android.content.Intent;
import android.content.pm.ServiceInfo;
import android.os.Build;
import android.os.IBinder;
import android.util.Log;

/**
 * Foreground service that owns the reusable MediaProjection session.
 *
 * <p>Android 14+ refuses {@code MediaProjectionManager.getMediaProjection()}
 * unless a foreground service with {@code foregroundServiceType="mediaProjection"}
 * is already running, and it must stay alive for the whole session. The bridge
 * starts this service before the first projection and keeps it until the user
 * stops the session from the notification, revokes the projection, or the
 * process ends.
 *
 * <p>The service never captures by itself: the bridge takes single frames from
 * the virtual display, so the notification states that a screenshot session is
 * active and offers a stop action.
 */
public final class ScreenCaptureService extends Service {
    private static final String TAG = "wunder";
    private static final String CHANNEL_ID = "wunder-screen-capture";
    private static final String LEGACY_CHANNEL_ID = "wunder-screen";
    private static final String ACTION_STOP = "dev.wunder.android.action.STOP_SCREEN_CAPTURE";
    private static final int NOTIFICATION_ID = 4201;
    private static final int STOP_REQUEST = 4202;
    private static volatile boolean running;

    /** Start the service, or refresh it when it already runs. */
    public static void start(Context context) {
        if (context == null) {
            return;
        }
        Intent intent = new Intent(context, ScreenCaptureService.class);
        try {
            context.startForegroundService(intent);
        } catch (Throwable error) {
            // A background start can be rejected by the platform on Android
            // 12+; the bridge turns that into a readable tool error.
            Log.w(TAG, "screen capture service start failed: " + error);
        }
    }

    public static void stop(Context context) {
        if (context == null) {
            return;
        }
        try {
            context.stopService(new Intent(context, ScreenCaptureService.class));
        } catch (Throwable ignored) {
        }
    }

    /** True once the foreground notification is posted, so projection is legal. */
    public static boolean isRunning() {
        return running;
    }

    @Override
    public void onCreate() {
        super.onCreate();
        createChannel();
        startForegroundCompat();
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        if (intent != null && ACTION_STOP.equals(intent.getAction())) {
            // The user ended the session from the notification; release the
            // projection and clear the grant before the service goes away.
            ScreenCaptureBridge.stopSession(this);
            stopSelf();
            return START_NOT_STICKY;
        }
        if (!running) {
            startForegroundCompat();
        }
        // The projection grant does not survive the process; a sticky restart
        // would leave an empty notification with no usable session.
        return START_NOT_STICKY;
    }

    @Override
    public void onDestroy() {
        running = false;
        stopForeground(STOP_FOREGROUND_REMOVE);
        // A projection is only legal while this service runs, so drop the
        // session when the system takes the service down.
        ScreenCaptureBridge.onServiceStopped();
        super.onDestroy();
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }

    private void startForegroundCompat() {
        Notification notification = buildNotification();
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                startForeground(
                        NOTIFICATION_ID,
                        notification,
                        ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PROJECTION);
            } else {
                startForeground(NOTIFICATION_ID, notification);
            }
            running = true;
        } catch (Throwable error) {
            running = false;
            Log.w(TAG, "screen capture foreground notification failed: " + error);
            stopSelf();
        }
    }

    private Notification buildNotification() {
        Notification.Builder builder = new Notification.Builder(this, CHANNEL_ID)
                .setSmallIcon(android.R.drawable.ic_menu_camera)
                .setContentTitle("屏幕截图会话进行中")
                .setContentText("截图可反复进行；点“停止”结束会话并释放屏幕授权")
                .setContentIntent(launchIntent())
                .addAction(new Notification.Action.Builder(
                        android.R.drawable.ic_menu_close_clear_cancel,
                        "停止",
                        stopIntent()).build())
                .setCategory(Notification.CATEGORY_SERVICE)
                .setVisibility(Notification.VISIBILITY_PRIVATE)
                .setOngoing(true)
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
        return PendingIntent.getActivity(this, 0, launch, immutableFlags());
    }

    private PendingIntent stopIntent() {
        Intent stop = new Intent(this, ScreenCaptureService.class);
        stop.setAction(ACTION_STOP);
        return PendingIntent.getService(this, STOP_REQUEST, stop, immutableFlags());
    }

    private static int immutableFlags() {
        int flags = PendingIntent.FLAG_UPDATE_CURRENT;
        if (Build.VERSION.SDK_INT >= 23) {
            flags |= PendingIntent.FLAG_IMMUTABLE;
        }
        return flags;
    }

    private void createChannel() {
        NotificationManager manager =
                (NotificationManager) getSystemService(NOTIFICATION_SERVICE);
        if (manager == null) {
            return;
        }
        NotificationChannel channel = new NotificationChannel(
                CHANNEL_ID, "屏幕采集", NotificationManager.IMPORTANCE_MIN);
        channel.setDescription("屏幕采集会话的运行状态");
        channel.setShowBadge(false);
        channel.setSound(null, null);
        channel.enableVibration(false);
        manager.createNotificationChannel(channel);
        // Channel importance is immutable after creation; replace the old
        // low-importance channel so existing installs pick up the quieter one.
        manager.deleteNotificationChannel(LEGACY_CHANNEL_ID);
    }
}
