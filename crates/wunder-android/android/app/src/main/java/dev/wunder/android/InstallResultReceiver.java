package dev.wunder.android;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageInstaller;
import android.util.Log;

/**
 * Final status of an APK install session. The system installer owns the
 * confirmation UI; this receiver only records the outcome for diagnostics.
 * The process is usually replaced by the new APK, so the UI never waits on it.
 */
public class InstallResultReceiver extends BroadcastReceiver {
    private static final String TAG = "WunderInstall";

    @Override
    public void onReceive(Context context, Intent intent) {
        int status = intent.getIntExtra(
                PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE);
        String message = intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE);
        if (status == PackageInstaller.STATUS_PENDING_USER_ACTION) {
            // The first broadcast only says "ask the user". The confirmation
            // activity arrives as an Intent extra and must be launched by the
            // app, otherwise a committed session sits invisible forever.
            Intent confirmation = intent.getParcelableExtra(Intent.EXTRA_INTENT);
            if (confirmation != null) {
                confirmation.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
                try {
                    context.startActivity(confirmation);
                } catch (Exception error) {
                    Log.w(TAG, "launch install confirmation failed", error);
                }
            }
            return;
        }
        Log.i(TAG, "install finished status=" + status + " message=" + message);
        if (status == PackageInstaller.STATUS_SUCCESS) {
            UiBridge.publishInstallResult("succeeded");
        } else if (status == PackageInstaller.STATUS_FAILURE_ABORTED) {
            UiBridge.publishInstallResult("aborted");
        } else {
            UiBridge.publishInstallResult(
                    "failed:" + (message == null || message.trim().isEmpty()
                            ? "安装失败"
                            : message));
        }
    }
}
