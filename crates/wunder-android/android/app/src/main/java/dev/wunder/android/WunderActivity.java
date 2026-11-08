package dev.wunder.android;

import android.app.NativeActivity;
import android.content.Intent;
import android.os.Build;
import android.os.Bundle;
import android.view.KeyEvent;
import android.window.OnBackInvokedCallback;
import android.window.OnBackInvokedDispatcher;

/**
 * NativeActivity plus the channels that the native shell cannot own.
 *
 * <p>The workspace picker and the legacy save dialog are started with
 * {@code startActivityForResult}; android.app.NativeActivity has no hook for
 * the reply, so this thin subclass forwards it to {@link UiBridge}. It also
 * owns the Back key: the shell closes its own pages first and exits only after
 * a second Back on the chat root, so Back must reach Rust instead of finishing
 * the activity.
 */
public class WunderActivity extends NativeActivity {
    // Kept as Object so class loading on API 26-32 never resolves the
    // API 33+ callback type. Casts only happen inside the guarded path.
    private Object backCallback;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        registerBackCallback();
    }

    @Override
    protected void onResume() {
        super.onResume();
        // Re-register on every resume so this callback stays the most recent
        // one for the dispatcher; Slint's window helper registers its own
        // default-priority callback while the native window is created.
        registerBackCallback();
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (hasFocus) {
            // On API 33 both callbacks share PRIORITY_DEFAULT and the last
            // registration wins. The native window (and Slint's callback) is
            // created asynchronously, so register again once the window is
            // focused; on newer versions the overlay priority already wins.
            registerBackCallback();
        }
    }

    private void registerBackCallback() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) {
            return;
        }
        OnBackInvokedDispatcher dispatcher = getOnBackInvokedDispatcher();
        if (dispatcher == null) {
            return;
        }
        if (backCallback != null) {
            try {
                dispatcher.unregisterOnBackInvokedCallback((OnBackInvokedCallback) backCallback);
            } catch (RuntimeException ignored) {
                // The window may have replaced the dispatcher; registration
                // below is what matters, so a stale entry is harmless.
            }
        }
        OnBackInvokedCallback callback = () -> UiBridge.requestBack();
        int priority = Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE
                ? OnBackInvokedDispatcher.PRIORITY_OVERLAY
                : OnBackInvokedDispatcher.PRIORITY_DEFAULT;
        try {
            dispatcher.registerOnBackInvokedCallback(priority, callback);
            backCallback = callback;
        } catch (RuntimeException ignored) {
            // Fall back to the legacy key path (dispatchKeyEvent) rather than
            // crashing the activity over an OEM dispatcher quirk.
            backCallback = null;
        }
    }

    @Override
    public boolean dispatchKeyEvent(KeyEvent event) {
        // Legacy path on API 26-32 (and hardware keyboards on newer
        // versions). Consume both actions so the framework never finishes the
        // activity on its own; the Rust shell decides what Back means.
        if (event.getKeyCode() == KeyEvent.KEYCODE_BACK) {
            if (event.getAction() == KeyEvent.ACTION_UP && event.getRepeatCount() == 0) {
                UiBridge.requestBack();
            }
            return true;
        }
        return super.dispatchKeyEvent(event);
    }

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
