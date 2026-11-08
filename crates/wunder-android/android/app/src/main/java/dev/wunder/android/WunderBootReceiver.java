package dev.wunder.android;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.util.Log;

/**
 * Re-arms the scheduled-tick job after a reboot or package update.
 *
 * <p>A persisted JobScheduler job normally survives both, but an OEM may clear
 * it; scheduling here is idempotent and keeps the fallback cheap. Declared with
 * {@code RECEIVE_BOOT_COMPLETED} in the manifest.
 */
public class WunderBootReceiver extends BroadcastReceiver {
    private static final String TAG = "wunder-bg";

    @Override
    public void onReceive(Context context, Intent intent) {
        String action = intent == null ? null : intent.getAction();
        if (!Intent.ACTION_BOOT_COMPLETED.equals(action)
                && !"android.intent.action.LOCKED_BOOT_COMPLETED".equals(action)
                && !Intent.ACTION_MY_PACKAGE_REPLACED.equals(action)) {
            return;
        }
        try {
            WunderCronJobService.schedule(context);
            Log.i(TAG, "cron job re-armed after " + action);
        } catch (Throwable error) {
            Log.w(TAG, "cron job re-arm failed: " + error);
        }
    }
}
