package dev.wunder.android;

import android.accessibilityservice.AccessibilityService;
import android.content.Intent;
import android.view.accessibility.AccessibilityEvent;

/**
 * The device-side half of phone control.
 *
 * <p>The service holds no business logic: PhoneBridge reads the active window,
 * dispatches gestures and captures the screen through this connection. Being a
 * separate class also keeps the Android permission boundary explicit: without
 * the user enabling it in system settings, no touch or screenshot can happen.
 */
public final class WunderAccessibilityService extends AccessibilityService {
    private static volatile WunderAccessibilityService instance;

    static WunderAccessibilityService getInstance() {
        return instance;
    }

    @Override
    protected void onServiceConnected() {
        super.onServiceConnected();
        instance = this;
    }

    @Override
    public void onAccessibilityEvent(AccessibilityEvent event) {
        // Phone control is demand-driven; no event stream is kept.
    }

    @Override
    public void onInterrupt() {
    }

    @Override
    public boolean onUnbind(Intent intent) {
        instance = null;
        return super.onUnbind(intent);
    }
}
