package dev.wunder.android;

import android.app.NativeActivity;
import android.content.Intent;

/**
 * NativeActivity plus the activity-result channel.
 *
 * <p>The workspace picker and the legacy save dialog are started with
 * {@code startActivityForResult}; android.app.NativeActivity has no hook for
 * the reply, so this thin subclass forwards it to {@link UiBridge}. Everything
 * else (window, lifecycle, native library loading) stays NativeActivity's.
 */
public class WunderActivity extends NativeActivity {
    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        UiBridge.handleActivityResult(this, requestCode, resultCode, data);
    }

    @Override
    public void onRequestPermissionsResult(int requestCode, String[] permissions, int[] grantResults) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults);
        UiBridge.handlePermissionResult(requestCode, grantResults);
    }
}
