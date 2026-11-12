package dev.wunder.android;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertTrue;
import static org.junit.Assert.fail;

import android.Manifest;
import android.app.Instrumentation;
import android.app.Notification;
import android.app.NotificationManager;
import android.content.Context;
import android.content.res.Resources;
import android.content.res.TypedValue;
import android.graphics.drawable.ColorDrawable;
import android.graphics.drawable.Drawable;
import android.os.Build;
import android.os.SystemClock;
import android.service.notification.StatusBarNotification;

import androidx.lifecycle.Lifecycle;
import androidx.test.core.app.ActivityScenario;
import androidx.test.ext.junit.runners.AndroidJUnit4;
import androidx.test.platform.app.InstrumentationRegistry;

import org.json.JSONObject;
import org.junit.BeforeClass;
import org.junit.Test;
import org.junit.runner.RunWith;

import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicReference;

/**
 * Cross-layer smoke tests for the Android shell.
 *
 * <p>These touch the same Java entry points the Rust side calls over JNI, so a
 * renamed or removed bridge method fails the gate before a release build does.
 * They deliberately avoid asserting device state that varies between emulator
 * images (accessibility connection, Ubuntu runtime installation).
 */
@RunWith(AndroidJUnit4.class)
public class WunderSmokeTest {
    private static final long RESUME_TIMEOUT_MS = 10_000L;
    private static final long POLL_INTERVAL_MS = 200L;

    /**
     * A fresh install asks for notification access when the runtime service
     * starts, and the system dialog covers WunderActivity long enough for the
     * activity to stay STARTED. Grant it up front so the launch test observes
     * the app window rather than the permission controller.
     */
    @BeforeClass
    public static void grantStartupPermissions() {
        Instrumentation instrumentation = InstrumentationRegistry.getInstrumentation();
        String packageName = instrumentation.getTargetContext().getPackageName();
        try {
            instrumentation.getUiAutomation().grantRuntimePermission(
                    packageName, Manifest.permission.POST_NOTIFICATIONS);
        } catch (Throwable ignored) {
            // Already granted, or this platform has no such runtime permission.
        }
    }

    /**
     * device_info is the one PhoneBridge action that works without the
     * accessibility service, which makes it the cheapest end-to-end check of
     * the JNI-facing executor.
     */
    @Test
    public void phoneBridgeDeviceInfoReportsCoreFields() throws Exception {
        Context context = InstrumentationRegistry.getInstrumentation().getTargetContext();
        JSONObject result = new JSONObject(PhoneBridge.execute(context, "device_info", "{}"));

        assertTrue("device_info should succeed: " + result, result.optBoolean("ok", false));
        JSONObject info = result.getJSONObject("data");
        assertTrue("sdk is missing: " + info, info.has("sdk"));
        assertTrue("unexpected sdk: " + info.getInt("sdk"), info.getInt("sdk") >= 26);
        assertTrue("app_version is missing: " + info, info.has("app_version"));
        assertFalse("app_version is empty", info.getString("app_version").isEmpty());
        assertTrue("accessibility_connected is missing: " + info,
                info.has("accessibility_connected"));
    }

    /**
     * The settings page reads one JSON object with every managed permission.
     * UiBridge keeps the current activity in a WeakReference, so the test
     * attaches a live activity before asking for the snapshot.
     */
    @Test
    public void permissionSnapshotListsManagedPermissions() throws Exception {
        try (ActivityScenario<WunderActivity> scenario = launchResumed()) {
            AtomicReference<WunderActivity> holder = new AtomicReference<>();
            scenario.onActivity(holder::set);
            WunderActivity activity = holder.get();
            assertNotNull("activity should be available after RESUMED", activity);
            UiBridge.attach(activity);

            JSONObject snapshot = new JSONObject(UiBridge.permissionSnapshot());
            assertTrue("permission snapshot should not be empty", snapshot.length() > 0);
            for (String key : new String[]{
                    "notifications", "overlay", "accessibility", "microphone"}) {
                assertTrue("missing permission key: " + key, snapshot.has(key));
                String value = snapshot.optString(key, "");
                assertTrue("unexpected state for " + key + ": " + value,
                        "granted".equals(value)
                                || "denied".equals(value)
                                || "unsupported".equals(value));
            }
        }
    }

    /**
     * The shell splits the proot runtime status into five US-separated fields.
     * An x86_64 emulator reports unsupported, which is correct; only the shape
     * of the snapshot is asserted here.
     */
    @Test
    public void ubuntuRuntimeSnapshotHasFiveSections() {
        String snapshot = UbuntuRuntime.snapshot();
        assertNotNull(snapshot);
        String[] parts = snapshot.split("\u001f", -1);
        assertEquals("state/progress/stage/version/error", 5, parts.length);
        assertFalse("state section is empty", parts[0].isEmpty());
    }

    /**
     * NativeActivity plus Slint starts asynchronously, so poll instead of
     * sleeping for a fixed period and fail if the window finishes early.
     */
    @Test
    public void activityReachesResumedWithoutFinishing() {
        try (ActivityScenario<WunderActivity> scenario =
                     ActivityScenario.launch(WunderActivity.class)) {
            long deadline = SystemClock.uptimeMillis() + RESUME_TIMEOUT_MS;
            while (scenario.getState() != Lifecycle.State.RESUMED
                    && SystemClock.uptimeMillis() < deadline) {
                SystemClock.sleep(POLL_INTERVAL_MS);
            }
            assertEquals("WunderActivity should reach RESUMED within "
                            + RESUME_TIMEOUT_MS + " ms",
                    Lifecycle.State.RESUMED, scenario.getState());

            AtomicBoolean finishing = new AtomicBoolean(true);
            scenario.onActivity(activity -> finishing.set(activity.isFinishing()));
            assertFalse("WunderActivity must not be finishing after launch", finishing.get());
        }
    }

    /**
     * Launching must not flash a blown-up launcher icon before the shell
     * paints: the launch theme uses the shell's own panel tone (Theme.panel),
     * and on API 31+ the mandatory system splash draws no icon at all. Asserted
     * through the resolved theme so a revert of the theme or the values-v31
     * override fails here instead of being noticed by eye on a phone.
     */
    @Test
    public void launchThemeMatchesShellSurfaceAndDrawsNoSplashIcon() {
        try (ActivityScenario<WunderActivity> scenario = launchResumed()) {
            scenario.onActivity(activity -> {
                Resources resources = activity.getResources();
                Resources.Theme theme = activity.getTheme();

                TypedValue background = new TypedValue();
                assertTrue("windowBackground should resolve",
                        theme.resolveAttribute(android.R.attr.windowBackground, background, true));
                assertEquals("launch window must paint Theme.panel (#FBFAF8)",
                        (int) 0xFFFBFAF8L, resolveColor(resources, theme, background));

                if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) {
                    return;
                }
                TypedValue splashBackground = new TypedValue();
                assertTrue("windowSplashScreenBackground should resolve",
                        theme.resolveAttribute(
                                android.R.attr.windowSplashScreenBackground, splashBackground, true));
                assertEquals("splash must use the same tone as the first frame",
                        (int) 0xFFFBFAF8L, resolveColor(resources, theme, splashBackground));

                TypedValue icon = new TypedValue();
                assertTrue("windowSplashScreenAnimatedIcon should resolve",
                        theme.resolveAttribute(
                                android.R.attr.windowSplashScreenAnimatedIcon, icon, true));
                Drawable drawable = icon.resourceId == 0
                        ? null
                        : resources.getDrawable(icon.resourceId, theme);
                if (!(drawable instanceof ColorDrawable)) {
                    fail("splash icon should be a transparent colour drawable: " + drawable);
                    return;
                }
                assertEquals("splash icon must be fully transparent",
                        0, ((ColorDrawable) drawable).getColor());
            });
        }
    }

    /** Attribute values arrive either as a colour literal or as a resource. */
    private static int resolveColor(Resources resources, Resources.Theme theme, TypedValue value) {
        return value.resourceId == 0 ? value.data : resources.getColor(value.resourceId, theme);
    }

    /**
     * The foreground notification is the only progress surface while the app
     * is in the background. Busy must show the live phase/detail and stop
     * showing the idle copy; a regression here is otherwise invisible in UI
     * tests because the notification lives outside the Slint window.
     */
    @Test
    public void runtimeNotificationShowsRunningThreadCountAndReturnsToIdle() throws Exception {
        Context context = InstrumentationRegistry.getInstrumentation().getTargetContext();
        // A background instrumentation process may not start a foreground
        // service on Android 12+; the real turn starts from the visible
        // activity, so the test does the same.
        try (ActivityScenario<WunderActivity> scenario = launchResumed()) {
            // The activity starts the runtime service; wait for its first
            // notification before driving state so the assertions do not race
            // process startup or the system's FGS notification policy.
            waitForRuntimeNotification(context);
            try {
                WunderRuntimeService.setBusy(context, true);
                WunderRuntimeService.setRunningThreads(context, 1);

                Notification busy = waitForRuntimeNotification(context, "运行中 1 线程");
                assertEquals(
                        "wunder 安卓版",
                        String.valueOf(busy.extras.getCharSequence(Notification.EXTRA_TITLE)));
                assertEquals(
                        "运行中 1 线程",
                        String.valueOf(busy.extras.getCharSequence(Notification.EXTRA_TEXT)));
                assertTrue("busy notification must be ongoing",
                        (busy.flags & Notification.FLAG_ONGOING_EVENT) != 0);

                // Concurrent turns update the same simple line, not a phase log.
                WunderRuntimeService.setRunningThreads(context, 2);
                Notification two = waitForRuntimeNotification(context, "运行中 2 线程");
                assertEquals(
                        "运行中 2 线程",
                        String.valueOf(two.extras.getCharSequence(Notification.EXTRA_TEXT)));

                WunderRuntimeService.setBusy(context, false);
                Notification idle = waitForRuntimeNotification(context, "待机中，点击返回");
                assertEquals(
                        "待机中，点击返回",
                        String.valueOf(idle.extras.getCharSequence(Notification.EXTRA_TEXT)));
                assertFalse("idle notification must not be ongoing",
                        (idle.flags & Notification.FLAG_ONGOING_EVENT) != 0);
            } finally {
                // A failed assertion must not leak busy state into later tests.
                WunderRuntimeService.setBusy(context, false);
            }
        }
    }

    /**
     * The Ubuntu extraction is not an agent turn: while it runs the notification
     * must read "正在安装运行环境…" instead of "运行中 1 线程", and take priority
     * over a concurrent busy state so the install is never mislabelled as running.
     */
    @Test
    public void runtimeNotificationShowsInstallingDuringEnvironmentSetup() throws Exception {
        Context context = InstrumentationRegistry.getInstrumentation().getTargetContext();
        try (ActivityScenario<WunderActivity> scenario = launchResumed()) {
            waitForRuntimeNotification(context);
            try {
                WunderRuntimeService.setBusy(context, true);
                WunderRuntimeService.setRunningThreads(context, 1);
                WunderRuntimeService.setInstalling(context, true);
                Notification installing = waitForRuntimeNotification(context, "正在安装运行环境…");
                assertEquals(
                        "正在安装运行环境…",
                        String.valueOf(installing.extras.getCharSequence(Notification.EXTRA_TEXT)));
                assertTrue("installing notification must be ongoing",
                        (installing.flags & Notification.FLAG_ONGOING_EVENT) != 0);

                // Clearing the install falls back to the live thread count, not idle.
                WunderRuntimeService.setInstalling(context, false);
                Notification busy = waitForRuntimeNotification(context, "运行中 1 线程");
                assertEquals(
                        "运行中 1 线程",
                        String.valueOf(busy.extras.getCharSequence(Notification.EXTRA_TEXT)));

                WunderRuntimeService.setBusy(context, false);
                Notification idle = waitForRuntimeNotification(context, "待机中，点击返回");
                assertEquals(
                        "待机中，点击返回",
                        String.valueOf(idle.extras.getCharSequence(Notification.EXTRA_TEXT)));
            } finally {
                // A failed assertion must not leave installing=true behind:
                // the service is process-scoped and later tests read it.
                WunderRuntimeService.setInstalling(context, false);
                WunderRuntimeService.setBusy(context, false);
            }
        }
    }

    private static Notification waitForRuntimeNotification(Context context) {
        return waitForRuntimeNotification(context, null);
    }

    private static Notification waitForRuntimeNotification(Context context, String expectedText) {
        NotificationManager manager =
                (NotificationManager) context.getSystemService(Context.NOTIFICATION_SERVICE);
        assertNotNull("notification manager unavailable", manager);
        long deadline = SystemClock.uptimeMillis() + RESUME_TIMEOUT_MS;
        Notification last = null;
        while (SystemClock.uptimeMillis() < deadline) {
            for (StatusBarNotification active : manager.getActiveNotifications()) {
                if (active.getId() != 4101) {
                    continue;
                }
                last = active.getNotification();
                String text = String.valueOf(last.extras.getCharSequence(Notification.EXTRA_TEXT));
                if (expectedText == null || expectedText.equals(text)) {
                    return last;
                }
            }
            SystemClock.sleep(POLL_INTERVAL_MS);
        }
        assertNotNull("runtime notification 4101 was not posted", last);
        return last;
    }

    /**
     * The data page's backup button goes through the same Java bridge. A real
     * archive is the cheapest proof that config/SQLite paths are readable and
     * the restore staging contract stays writable; restore itself is applied
     * on the next process start and is not exercised here.
     */
    @Test
    public void localBackupCreatesReadableArchive() throws Exception {
        Context context = InstrumentationRegistry.getInstrumentation().getTargetContext();
        // WUNDER_HOME is created by the engine, not by the instrumentation
        // process; launch the real activity before asking for an archive.
        try (ActivityScenario<WunderActivity> scenario = launchResumed()) {
            String path = BackupBridge.backup(context);
            assertFalse("backup failed: " + path, path.startsWith("!"));
            java.io.File archive = new java.io.File(path);
            assertTrue("backup archive is missing: " + path, archive.isFile());
            assertTrue("backup archive is empty: " + path, archive.length() > 0);
            // The archive is a cache artifact; do not leave test copies behind.
            //noinspection ResultOfMethodCallIgnored
            archive.delete();
        }
    }

    /**
     * The screen-capture and notification bridges expose a status action that
     * needs no user grant, which keeps this check independent of the consent
     * dialogs those features normally require.
     */
    @Test
    public void captureAndNotificationBridgesReportStatus() throws Exception {
        Context context = InstrumentationRegistry.getInstrumentation().getTargetContext();

        JSONObject screen = new JSONObject(ScreenCaptureBridge.execute(context, "status", "{}"));
        assertTrue("screen status should succeed: " + screen, screen.optBoolean("ok", false));
        assertTrue("screen status misses authorized: " + screen, screen.has("authorized"));
        assertTrue("screen status misses projecting: " + screen, screen.has("projecting"));

        JSONObject notifications =
                new JSONObject(NotificationBridge.execute(context, "status", "{}"));
        assertTrue("notification status should succeed: " + notifications,
                notifications.optBoolean("ok", false));
        JSONObject data = notifications.getJSONObject("data");
        assertTrue("notification status misses connected: " + data, data.has("connected"));
        assertTrue("notification status misses count: " + data, data.has("count"));
    }

    private static ActivityScenario<WunderActivity> launchResumed() {
        ActivityScenario<WunderActivity> scenario = ActivityScenario.launch(WunderActivity.class);
        long deadline = SystemClock.uptimeMillis() + RESUME_TIMEOUT_MS;
        while (scenario.getState() != Lifecycle.State.RESUMED
                && SystemClock.uptimeMillis() < deadline) {
            SystemClock.sleep(POLL_INTERVAL_MS);
        }
        assertEquals("WunderActivity should reach RESUMED within "
                        + RESUME_TIMEOUT_MS + " ms",
                Lifecycle.State.RESUMED, scenario.getState());
        return scenario;
    }
}
