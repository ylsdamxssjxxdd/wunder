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
 * Short-lived foreground service for one MediaProjection capture.
 *
 * <p>Android 14+ refuses {@code MediaProjectionManager.getMediaProjection()}
 * unless a foreground service with {@code foregroundServiceType="mediaProjection"}
 * is already running. The bridge starts this service only for the frame that
 * consumes a one-shot consent and stops it as soon as the PNG is saved, so the
 * ongoing notification never outlives the capture.
 *
 * <p>The service never captures by itself: the bridge creates a virtual display
 * per frame, so the notification copy avoids implying continuous recording.
 */
public final class ScreenCaptureService extends Service {
    private static final String TAG = "wunder";
    private static final String CHANNEL_ID = "wunder-screen-capture";
    private static final String LEGACY_CHANNEL_ID = "wunder-screen";
    private static final int NOTIFICATION_ID = 4201;
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
                .setContentTitle("正在采集屏幕")
                .setContentText("仅本次截图读取当前画面")
                .setContentIntent(launchIntent())
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
