package dev.wunder.android;

import android.app.NativeActivity;
import android.content.Intent;
import android.os.Build;
import android.os.Bundle;
import android.util.Log;
import android.view.KeyEvent;
import android.view.View;
import android.view.Window;
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
    private static final String TAG = "WunderBack";

    static {
        // Slint's Android backend renders through Skia, so libwunder_android.so
        // needs the NDK's libc++ runtime. Load the APK copy explicitly before
        // NativeActivity loads the native shell: the emulator's system libc++
        // is older and does not provide std::__ndk1::__hash_memory.
        System.loadLibrary("c++_shared");
    }

    // Kept as Object so class loading on API 26-32 never resolves the
    // API 33+ callback type. Casts only happen inside the guarded path.
    private Object backCallback;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        // Install before any UI work: a failure in the first frame should still
        // leave a report behind. The handler only appends a file and then
        // delegates, so the platform's own crash flow is unchanged.
        CrashHandler.install(this);
        // MediaProjection consent is delivered to this activity; keep the
        // bridge's weak activity reference fresh across recreations.
        ScreenCaptureBridge.attach(this);
        // Persisted JobScheduler work is the reliable wake on Android 12+;
        // scheduling on every launch also repairs a job cleared by an OEM.
        WunderCronJobService.schedule(this);
        // A cold start from a share sheet carries the payload on the launch
        // intent. A recreated activity (saved state) must not replay a share
        // the engine already consumed.
        if (savedInstanceState == null) {
            IntentBridge.handleIntent(this, getIntent());
        }
        registerBackCallback();
        applyLightSystemBars();
        // The native window helper may install its own system-ui flags while
        // the first frame is created; reapply once that work has settled.
        getWindow().getDecorView().post(this::applyLightSystemBars);
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        // WunderActivity is singleTask, so a second share or VIEW link lands
        // here instead of creating a new activity (and a second engine view).
        IntentBridge.handleIntent(this, intent);
    }

    @Override
    protected void onResume() {
        super.onResume();
        // Re-register on every resume so this callback stays the most recent
        // one for the dispatcher; Slint's window helper registers its own
        // default-priority callback while the native window is created.
        registerBackCallback();
        applyLightSystemBars();
        ScreenCaptureBridge.attach(this);
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
            applyLightSystemBars();
        }
    }

    /**
     * The app paints its own light surface edge-to-edge, so the system bars
     * need dark icons. Setting the appearance explicitly also keeps the clock
     * and status icons readable when the platform / OEM theme does not infer
     * it from the (light) activity theme.
     */
    private void applyLightSystemBars() {
        Window window = getWindow();
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            android.view.WindowInsetsController controller = window.getInsetsController();
            if (controller != null) {
                int lightBars = android.view.WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS
                        | android.view.WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS;
                controller.setSystemBarsAppearance(lightBars, lightBars);
            }
            return;
        }
        View decor = window.getDecorView();
        int flags = decor.getSystemUiVisibility() | View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR;
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            flags |= View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR;
        }
        decor.setSystemUiVisibility(flags);
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
        // PRIORITY_OVERLAY keeps the shell ahead of the backend's own
        // default-priority callback. Both callbacks feed the same app queue,
        // so registration order cannot change page navigation. It is a hint:
        // when the platform refuses it, fall back to default instead of
        // leaving the app without any callback.
        boolean registered = false;
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            registered = tryRegisterBackCallback(
                    dispatcher, OnBackInvokedDispatcher.PRIORITY_OVERLAY, callback);
        }
        if (!registered) {
            registered = tryRegisterBackCallback(
                    dispatcher, OnBackInvokedDispatcher.PRIORITY_DEFAULT, callback);
        }
        backCallback = registered ? callback : null;
    }

    private boolean tryRegisterBackCallback(
            OnBackInvokedDispatcher dispatcher, int priority, OnBackInvokedCallback callback) {
        try {
            dispatcher.registerOnBackInvokedCallback(priority, callback);
            return true;
        } catch (RuntimeException error) {
            // OEM dispatchers and platforms with predictive back disabled can
            // reject a registration outright; the shell still owns Back
            // through the Slint key pipeline, so a rejection is not fatal.
            Log.w(TAG, "back callback rejected at priority " + priority, error);
            return false;
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
        if (!ScreenCaptureBridge.handleActivityResult(this, requestCode, resultCode, data)) {
            UiBridge.handleActivityResult(this, requestCode, resultCode, data);
        }
    }

    @Override
    public void onRequestPermissionsResult(int requestCode, String[] permissions, int[] grantResults) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults);
        UiBridge.handlePermissionResult(requestCode, grantResults);
    }
}
