package dev.wunder.android;

import android.Manifest;
import android.app.Activity;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.content.ClipData;
import android.content.ContentValues;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageInfo;
import android.content.pm.PackageInstaller;
import android.content.pm.PackageManager;
import android.content.pm.Signature;
import android.content.pm.SigningInfo;
import android.database.Cursor;
import android.media.AudioFormat;
import android.media.AudioRecord;
import android.media.MediaRecorder;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Environment;
import android.os.PowerManager;
import android.os.SystemClock;
import android.provider.DocumentsContract;
import android.provider.MediaStore;
import android.provider.OpenableColumns;
import android.provider.Settings;
import android.speech.RecognitionListener;
import android.speech.RecognizerIntent;
import android.speech.SpeechRecognizer;
import android.view.View;
import android.view.inputmethod.InputMethodManager;
import android.webkit.MimeTypeMap;

import org.json.JSONObject;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.lang.ref.WeakReference;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Comparator;
import java.util.HashSet;
import java.util.Locale;
import java.util.Set;
import java.util.function.Consumer;

/**
 * Small, process-scoped UI affordances the native shell needs.
 *
 * <p>The Slint activity is recreated for rotation/theme changes, so only a weak
 * reference is kept; UI work is re-posted to the main thread because the
 * engine may ask from any worker. The workspace picker and save dialog return
 * through {@link WunderActivity} and are delivered back to Rust via the two
 * static native callbacks.
 */
public final class UiBridge {
    private static final int REQUEST_PICK_FILES = 0x5701;
    private static final int REQUEST_PICK_DIRECTORY = 0x5702;
    private static final int REQUEST_STORAGE_PERMISSION = 0x5703;
    private static final int REQUEST_SAVE_FILE = 0x5702;
    private static final int REQUEST_RECORD_AUDIO = 0x5703;
    private static final int REQUEST_TAKE_PHOTO = 0x5704;
    // 桌面版 caps one drop at twenty files; keep the same bound on the phone.
    private static final int MAX_PICKED = 20;
    // Same capture contract as the desktop recorder: 16 kHz mono PCM WAV,
    // two minutes max.
    private static final int VOICE_RATE = 16_000;
    private static final int VOICE_MAX_SECONDS = 120;
    private static final int KEEP_VOICE_FILES = 2;

    private static WeakReference<Activity> activity = new WeakReference<>(null);
    private static volatile Consumer<Boolean> handlesSuppression;
    private static String pendingSaveSource;
    private static String pendingSaveName;
    /** Where the in-flight camera capture must write, or null. */
    private static File pendingCameraFile;
    // Results wait here until the engine's UI tick polls them. Java must not
    // call into the native library: NativeActivity loads it through the
    // framework class loader, so app classes cannot resolve its JNI symbols.
    private static final Object queueLock = new Object();
    private static ArrayList<String> pickedPaths; // null = no result pending
    /** Directory tree result: null = none, "" = cancelled, "!msg" = error. */
    private static String pickedDirectory;
    private static String saveResult; // null or "1|name" / "0|name"
    private static final ArrayList<String> voiceEvents = new ArrayList<>();
    private static final Object voiceLock = new Object();
    // Back presses are counted here for the Rust render tick. The activity
    // consumes the key so the framework never finishes it directly; Rust
    // closes the topmost page and only exits after a second Back at the root.
    private static int backRequests;
    private static long lastBackRequestMs;
    // Final status of the last package-install session, drained by the Rust
    // tick so the update card can offer a retry after a cancel or failure.
    private static String installResult;
    private static volatile boolean voiceRecording;
    private static volatile boolean voiceStopRequested;
    private static volatile boolean voiceCancelRequested;
    private static boolean pendingVoicePermission;
    // System speech recognition (RecognizerIntent). Used only when no ASR model
    // is configured: the phone's own recognizer costs nothing to set up, but
    // only returns text and depends on the device's language pack.
    private static boolean pendingSystemVoice;
    private static SpeechRecognizer speechRecognizer;
    private static volatile boolean systemListening;
    // 用户主动取消时吞掉随后的 onError：cancel() 会回一个 ERROR_CLIENT，
    // 那不是故障，不该在界面弹提示。
    private static volatile boolean systemCancelled;

    private UiBridge() {}

    public static void attach(Activity current) {
        activity = new WeakReference<>(current);
    }

    public static void registerHandlesSuppression(Consumer<Boolean> controller) {
        handlesSuppression = controller;
    }

    private static void suppressHandles(boolean suppressed) {
        Consumer<Boolean> controller = handlesSuppression;
        if (controller != null) {
            controller.accept(suppressed);
        }
    }

    /** Dismiss the soft keyboard, e.g. when the plus function panel opens. */
    public static void hideKeyboard() {
        suppressHandles(true);
        Activity current = activity.get();
        if (current == null) {
            return;
        }
        current.runOnUiThread(() -> {
            InputMethodManager imm = (InputMethodManager) current.getSystemService(Context.INPUT_METHOD_SERVICE);
            if (imm == null) {
                return;
            }
            View decor = current.getWindow().getDecorView();
            imm.hideSoftInputFromWindow(decor.getWindowToken(), 0);
        });
    }

    public static void setInputOverlayOpen(boolean open) {
        if (open) {
            hideKeyboard();
        } else {
            suppressHandles(false);
        }
    }

    /**
     * Record one Back press for the Rust shell. Called from the activity's
     * key and OnBackInvoked paths; a single physical press can arrive through
     * both on some ROMs, so presses within 40 ms collapse into one.
     */
    public static void requestBack() {
        synchronized (queueLock) {
            long now = SystemClock.uptimeMillis();
            if (now - lastBackRequestMs < 40) {
                return;
            }
            lastBackRequestMs = now;
            backRequests++;
        }
    }

    /** Drain Back presses. Rust processes them from its 33 ms render tick. */
    public static int takeBackRequests() {
        synchronized (queueLock) {
            int count = backRequests;
            backRequests = 0;
            return count;
        }
    }

    /** Close the visible activity after the shell accepted a Back-to-exit. */
    public static void finishActivity() {
        Activity current = activity.get();
        if (current != null) {
            current.runOnUiThread(current::finish);
        }
    }

    /**
     * Raw state of every permission the settings page manages. Values are
     * {@code granted}, {@code denied} or {@code unsupported}; the Rust side
     * owns the user-facing labels. Returning one JSON object keeps this call
     * to a single JNI round-trip.
     */
    public static String permissionSnapshot() {
        Activity current = activity.get();
        if (current == null) {
            return "{}";
        }
        JSONObject result = new JSONObject();
        try {
            result.put("notifications", notificationState(current));
            result.put("microphone", runtimeState(current, Manifest.permission.RECORD_AUDIO));
            result.put("overlay", Settings.canDrawOverlays(current) ? "granted" : "denied");
            result.put("accessibility", accessibilityEnabled(current) ? "granted" : "denied");
            result.put("battery", batteryExempt(current) ? "granted" : "denied");
            result.put(
                    "install_packages",
                    current.getPackageManager().canRequestPackageInstalls()
                            ? "granted"
                            : "denied");
            result.put("all_files", allFilesState(current));
            result.put(
                    "screen_capture",
                    ScreenCaptureBridge.hasPermission() ? "granted" : "denied");
            result.put("notification_access", notificationAccessState(current));
        } catch (Exception ignored) {
        }
        return result.toString();
    }

    private static String notificationAccessState(Context context) {
        if (WunderNotificationListenerService.isConnected()) {
            return "granted";
        }
        try {
            android.content.ComponentName component =
                    new android.content.ComponentName(context, WunderNotificationListenerService.class);
            String enabled = Settings.Secure.getString(
                    context.getContentResolver(), "enabled_notification_listeners");
            return enabled != null && enabled.contains(component.flattenToString())
                    ? "granted"
                    : "denied";
        } catch (Exception ignored) {
            return "denied";
        }
    }

    /** Last time the installed package was replaced, for nightly update checks. */
    public static long installedAtMillis() {
        Activity current = activity.get();
        if (current == null) {
            return 0L;
        }
        try {
            return current.getPackageManager()
                    .getPackageInfo(current.getPackageName(), 0)
                    .lastUpdateTime;
        } catch (Exception ignored) {
            return 0L;
        }
    }

    /**
     * Hand a downloaded, verified APK to the system package installer.
     * Returns {@code started}, {@code permission} (unknown-sources settings
     * opened) or {@code failed:<reason>}. The APK is verified against the
     * installed package before the installer ever sees it.
     */
    public static String installApk(String path) {
        Activity current = activity.get();
        if (current == null) {
            return "failed:no activity";
        }
        File apk = new File(path);
        if (!apk.isFile()) {
            return "failed:更新包不存在";
        }
        if (!current.getPackageManager().canRequestPackageInstalls()) {
            current.runOnUiThread(() -> {
                try {
                    current.startActivity(new Intent(
                            Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
                            Uri.parse("package:" + current.getPackageName())));
                } catch (Exception ignored) {
                    try {
                        current.startActivity(new Intent(Settings.ACTION_SECURITY_SETTINGS));
                    } catch (Exception ignoredAgain) {
                    }
                }
            });
            return "permission";
        }
        String verification = verifyUpdateApk(current, apk);
        if (verification != null) {
            return "failed:" + verification;
        }
        PackageInstaller installer = current.getPackageManager().getPackageInstaller();
        int sessionId;
        try {
            PackageInstaller.SessionParams params = new PackageInstaller.SessionParams(
                    PackageInstaller.SessionParams.MODE_FULL_INSTALL);
            params.setAppPackageName(current.getPackageName());
            sessionId = installer.createSession(params);
        } catch (Exception error) {
            return "failed:" + messageOf(error, "无法创建安装会话");
        }
        try (PackageInstaller.Session session = installer.openSession(sessionId);
             InputStream input = new FileInputStream(apk)) {
            try (OutputStream output = session.openWrite("base.apk", 0, apk.length())) {
                copy(input, output);
                session.fsync(output);
            }
            Intent result = new Intent(current, InstallResultReceiver.class)
                    .setAction("dev.wunder.android.INSTALL_RESULT");
            int flags = PendingIntent.FLAG_UPDATE_CURRENT;
            if (Build.VERSION.SDK_INT >= 31) {
                // The package installer fills the result extras, so the
                // platform requires a mutable PendingIntent here.
                flags |= PendingIntent.FLAG_MUTABLE;
            }
            PendingIntent pending = PendingIntent.getBroadcast(current, sessionId, result, flags);
            session.commit(pending.getIntentSender());
            return "started";
        } catch (Exception error) {
            try {
                installer.abandonSession(sessionId);
            } catch (Exception ignored) {
            }
            return "failed:" + messageOf(error, "无法启动安装器");
        }
    }

    private static String messageOf(Exception error, String fallback) {
        String message = error.getMessage();
        return message == null || message.trim().isEmpty() ? fallback : message;
    }

    /** Record the terminal status of an install session for the Rust tick. */
    public static void publishInstallResult(String result) {
        synchronized (queueLock) {
            installResult = result;
        }
    }

    /** Drain the last install-session status, or null when none is pending. */
    public static String takeInstallResult() {
        synchronized (queueLock) {
            String result = installResult;
            installResult = null;
            return result;
        }
    }

    /**
     * Refuse an APK that is not this package, is signed by another key, or
     * claims a lower version. The manifest sha256 already proves integrity;
     * this proves it belongs to the installed app.
     */
    private static String verifyUpdateApk(Activity context, File apk) {
        PackageManager manager = context.getPackageManager();
        int flags = Build.VERSION.SDK_INT >= 28
                ? PackageManager.GET_SIGNING_CERTIFICATES
                : PackageManager.GET_SIGNATURES;
        PackageInfo archive;
        try {
            archive = manager.getPackageArchiveInfo(apk.getAbsolutePath(), flags);
        } catch (Exception error) {
            return "无法读取安装包：" + messageOf(error, "未知错误");
        }
        if (archive == null) {
            return "无法读取安装包";
        }
        if (!context.getPackageName().equals(archive.packageName)) {
            return "安装包包名不一致";
        }
        try {
            PackageInfo installed = manager.getPackageInfo(context.getPackageName(), flags);
            if (!sameSigners(archive, installed)) {
                return "安装包签名与当前应用不一致";
            }
            long archiveCode = Build.VERSION.SDK_INT >= 28
                    ? archive.getLongVersionCode()
                    : archive.versionCode;
            long installedCode = Build.VERSION.SDK_INT >= 28
                    ? installed.getLongVersionCode()
                    : installed.versionCode;
            if (archiveCode < installedCode) {
                return "安装包版本低于当前版本";
            }
        } catch (Exception error) {
            return "无法校验安装包：" + messageOf(error, "未知错误");
        }
        return null;
    }

    private static boolean sameSigners(PackageInfo left, PackageInfo right) {
        Set<String> leftSigners = signerFingerprints(left);
        Set<String> rightSigners = signerFingerprints(right);
        return !leftSigners.isEmpty() && leftSigners.equals(rightSigners);
    }

    private static Set<String> signerFingerprints(PackageInfo info) {
        if (Build.VERSION.SDK_INT >= 28) {
            return SignersApi28.fingerprints(info);
        }
        Set<String> fingerprints = new HashSet<>();
        Signature[] signers = info.signatures;
        if (signers != null) {
            for (Signature signature : signers) {
                fingerprints.add(sha256(signature.toByteArray()));
            }
        }
        return fingerprints;
    }

    /** Isolated so class loading on API 26-27 never resolves SigningInfo. */
    private static final class SignersApi28 {
        static Set<String> fingerprints(PackageInfo info) {
            Set<String> fingerprints = new HashSet<>();
            SigningInfo signing = info.signingInfo;
            Signature[] signers = signing == null ? null : signing.getApkContentsSigners();
            if (signers != null) {
                for (Signature signature : signers) {
                    fingerprints.add(sha256(signature.toByteArray()));
                }
            }
            return fingerprints;
        }
    }

    private static String sha256(byte[] bytes) {
        try {
            MessageDigest digest = MessageDigest.getInstance("SHA-256");
            StringBuilder out = new StringBuilder();
            for (byte value : digest.digest(bytes)) {
                out.append(String.format(Locale.ROOT, "%02x", value));
            }
            return out.toString();
        } catch (Exception ignored) {
            return "";
        }
    }

    /** Open the system surface that grants or revokes one managed permission. */
    public static void openPermission(String id) {
        Activity current = activity.get();
        if (current == null || id == null) {
            return;
        }
        Activity activity = current;
        activity.runOnUiThread(() -> {
            Intent primary;
            Intent fallback = appDetails(activity);
            switch (id) {
                case "notifications":
                    primary = new Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS)
                            .putExtra(Settings.EXTRA_APP_PACKAGE, activity.getPackageName());
                    break;
                case "microphone":
                    primary = fallback;
                    break;
                case "overlay":
                    primary = new Intent(
                            Settings.ACTION_MANAGE_OVERLAY_PERMISSION,
                            Uri.parse("package:" + activity.getPackageName()));
                    break;
                case "accessibility":
                    primary = new Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS);
                    break;
                case "battery":
                    primary = new Intent(
                            Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS,
                            Uri.parse("package:" + activity.getPackageName()));
                    fallback = new Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS);
                    break;
                case "install_packages":
                    primary = new Intent(
                            Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
                            Uri.parse("package:" + activity.getPackageName()));
                    fallback = new Intent(Settings.ACTION_SECURITY_SETTINGS);
                    break;
                case "screen_capture":
                    ScreenCaptureBridge.requestPermission(activity);
                    return;
                case "notification_access":
                    primary = new Intent(Settings.ACTION_NOTIFICATION_LISTENER_SETTINGS);
                    break;
                case "all_files":
                    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                        primary = new Intent(
                                Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION,
                                Uri.parse("package:" + activity.getPackageName()));
                        fallback = new Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION);
                    } else {
                        activity.requestPermissions(
                                Build.VERSION.SDK_INT <= Build.VERSION_CODES.Q
                                        ? new String[]{
                                                Manifest.permission.READ_EXTERNAL_STORAGE,
                                                Manifest.permission.WRITE_EXTERNAL_STORAGE}
                                        : new String[]{Manifest.permission.READ_EXTERNAL_STORAGE},
                                REQUEST_STORAGE_PERMISSION);
                        return;
                    }
                    break;
                default:
                    return;
            }
            try {
                activity.startActivity(primary);
            } catch (Exception ignored) {
                try {
                    activity.startActivity(fallback);
                } catch (Exception ignoredAgain) {
                }
            }
        });
    }

    /**
     * Open one workspace file with an installed viewer. The read-only document
     * provider supplies the content URI, so the file never leaves the sandbox.
     */
    public static boolean openWorkspaceFile(String workspaceId, String relativePath) {
        Activity current = activity.get();
        if (current == null || workspaceId == null || workspaceId.isEmpty()
                || relativePath == null || relativePath.isEmpty()) {
            return false;
        }
        String mime = WunderDocumentsProvider.mimeTypeOfName(relativePath);
        return launchWorkspaceDocument(current, workspaceId, relativePath, mime, false);
    }

    /** Open the containing folder (or a directory itself) in a file manager. */
    public static boolean openWorkspaceLocation(String workspaceId, String relativeDir) {
        Activity current = activity.get();
        if (current == null || workspaceId == null || workspaceId.isEmpty()) {
            return false;
        }
        String relative = relativeDir == null ? "" : relativeDir;
        return launchWorkspaceDocument(current, workspaceId, relative,
                DocumentsContract.Document.MIME_TYPE_DIR, true);
    }

    /**
     * Open one local file that has no workspace route (a chat attachment, a
     * temp artifact, or a card elsewhere on the device) through a one-shot
     * read grant. Only that file becomes readable; no folder is exposed.
     */
    public static boolean openLocalFile(String path) {
        Activity current = activity.get();
        if (current == null || path == null || path.trim().isEmpty()) {
            return false;
        }
        String token = WunderOpenProvider.register(path);
        if (token == null) {
            return false;
        }
        String authority = current.getPackageName() + WunderOpenProvider.AUTHORITY_SUFFIX;
        Uri uri = Uri.parse("content://" + authority + "/" + token);
        String mime = WunderDocumentsProvider.mimeTypeOfName(new File(path.trim()).getName());
        Intent intent = new Intent(Intent.ACTION_VIEW)
                .setDataAndType(uri, mime)
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
        try {
            current.startActivity(intent);
            return true;
        } catch (Exception error) {
            return false;
        }
    }

    /** Open one http(s) link from a transcript in the system browser. */
    public static boolean openExternalUrl(String url) {
        Activity current = activity.get();
        if (current == null || url == null) {
            return false;
        }
        String target = url.trim();
        if (!target.startsWith("https://") && !target.startsWith("http://")) {
            return false;
        }
        try {
            current.startActivity(new Intent(Intent.ACTION_VIEW, Uri.parse(target)));
            return true;
        } catch (Exception error) {
            return false;
        }
    }

    private static boolean launchWorkspaceDocument(Activity activity, String workspaceId,
            String relative, String mime, boolean directory) {
        String authority = activity.getPackageName() + WunderDocumentsProvider.AUTHORITY_SUFFIX;
        Uri uri = DocumentsContract.buildDocumentUri(authority, workspaceId + ":" + relative);
        Intent intent = new Intent(Intent.ACTION_VIEW)
                .setDataAndType(uri, mime)
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
        if (directory) {
            // Let the file manager browse the folder's children too.
            intent.addFlags(Intent.FLAG_GRANT_PREFIX_URI_PERMISSION);
        }
        try {
            activity.startActivity(intent);
            return true;
        } catch (Exception error) {
            return false;
        }
    }

    private static Intent appDetails(Context context) {
        return new Intent(
                Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
                Uri.parse("package:" + context.getPackageName()));
    }

    private static String runtimeState(Context context, String permission) {
        return context.checkSelfPermission(permission) == PackageManager.PERMISSION_GRANTED
                ? "granted"
                : "denied";
    }

    private static String notificationState(Context context) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU
                && context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS)
                        != PackageManager.PERMISSION_GRANTED) {
            return "denied";
        }
        NotificationManager manager =
                (NotificationManager) context.getSystemService(Context.NOTIFICATION_SERVICE);
        return manager != null && manager.areNotificationsEnabled() ? "granted" : "denied";
    }

    private static boolean accessibilityEnabled(Context context) {
        try {
            String services = Settings.Secure.getString(
                    context.getContentResolver(),
                    Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES);
            if (services == null) {
                return false;
            }
            String prefix = context.getPackageName() + "/";
            for (String entry : services.split(":")) {
                if (entry.startsWith(prefix) && entry.contains("WunderAccessibilityService")) {
                    return true;
                }
            }
        } catch (Exception ignored) {
        }
        return false;
    }

    private static boolean batteryExempt(Context context) {
        PowerManager manager = (PowerManager) context.getSystemService(Context.POWER_SERVICE);
        return manager != null && manager.isIgnoringBatteryOptimizations(context.getPackageName());
    }

    /**
     * All-files access: Android 11+ gates direct shared-storage paths behind
     * the special app access; Android 8-10 use the runtime READ/WRITE pair.
     */
    private static String allFilesState(Activity current) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            return Environment.isExternalStorageManager() ? "granted" : "denied";
        }
        boolean read = current.checkSelfPermission(Manifest.permission.READ_EXTERNAL_STORAGE)
                == PackageManager.PERMISSION_GRANTED;
        boolean write = Build.VERSION.SDK_INT > Build.VERSION_CODES.Q
                || current.checkSelfPermission(Manifest.permission.WRITE_EXTERNAL_STORAGE)
                        == PackageManager.PERMISSION_GRANTED;
        return read && write ? "granted" : "denied";
    }

    /**
     * Open the system folder picker for a workspace root. The tree URI is
     * converted to the host path the engine and proot both use; the caller
     * must hold all-files access for paths outside the app sandbox.
     */
    public static void pickDirectory() {
        Activity current = activity.get();
        if (current == null) {
            return;
        }
        current.runOnUiThread(() -> {
            Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE);
            intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION
                    | Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                    | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION);
            try {
                current.startActivityForResult(intent, REQUEST_PICK_DIRECTORY);
            } catch (Exception ignored) {
                publishDirectory("!无法打开目录选择器");
            }
        });
    }

    /** Drain the folder picker result; null until the system answers. */
    public static String takePickedDirectory() {
        synchronized (queueLock) {
            String result = pickedDirectory;
            pickedDirectory = null;
            return result;
        }
    }

    /** Open the system document picker; picked files come back staged. */
    public static void pickFiles() {
        Activity current = activity.get();
        if (current == null) {
            return;
        }
        current.runOnUiThread(() -> {
            Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
            intent.addCategory(Intent.CATEGORY_OPENABLE);
            intent.setType("*/*");
            intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true);
            try {
                current.startActivityForResult(intent, REQUEST_PICK_FILES);
            } catch (Exception ignored) {
                publishPick(new ArrayList<>());
            }
        });
    }

    /**
     * Start the in-app camera. The shot is written into the app cache through
     * {@link PhotoProvider}, so no storage permission is involved and nothing
     * lands in the gallery; the staged path then travels back through the pick
     * queue and the engine moves it into the workspace temp directory.
     */
    public static void takePhoto() {
        Activity current = activity.get();
        if (current == null) {
            publishPick(new ArrayList<>());
            return;
        }
        current.runOnUiThread(() -> {
            // 一次只开一台相机：第二次点击不能覆盖上一张的落盘目标。
            if (pendingCameraFile != null) {
                return;
            }
            File target;
            Uri output;
            try {
                File root = new File(current.getCacheDir(), "wunder-camera");
                //noinspection ResultOfMethodCallIgnored
                root.mkdirs();
                // 照片名是用户和模型都能直接引用的序号（photo_1.jpg），
                // 不再是毫秒时间戳；createNewFile 同时占住名字，避免并发覆盖。
                target = nextNumberedFile(root, "photo", "jpg");
                if (target == null) {
                    throw new IOException("no free photo name");
                }
                output = PhotoProvider.uriFor(current, target);
            } catch (Exception error) {
                publishPick(singlePath("!无法准备拍照文件"));
                return;
            }
            Intent intent = new Intent(MediaStore.ACTION_IMAGE_CAPTURE);
            intent.putExtra(MediaStore.EXTRA_OUTPUT, output);
            intent.addFlags(Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                    | Intent.FLAG_GRANT_READ_URI_PERMISSION);
            // Some OEM camera apps only honour the grant carried on the clip
            // data, not the extra output URI alone.
            intent.setClipData(ClipData.newRawUri("wunder-photo", output));
            pendingCameraFile = target;
            try {
                current.startActivityForResult(intent, REQUEST_TAKE_PHOTO);
            } catch (Exception error) {
                pendingCameraFile = null;
                //noinspection ResultOfMethodCallIgnored
                target.delete();
                publishPick(singlePath("!没有可用的相机应用"));
            }
        });
    }

    /**
     * Reserve the next free {@code <prefix>_<n>.<extension>} in {@code directory}.
     *
     * <p>A serial keeps camera output readable and ordered: the model can say
     * "photo_2.jpg" and the user can match it at a glance. Old timestamp-named
     * shots are ignored, so the first capture under the new scheme starts at 1.
     */
    private static File nextNumberedFile(File directory, String prefix, String extension) {
        int max = 0;
        File[] files = directory.listFiles();
        if (files != null) {
            String head = prefix + "_";
            for (File file : files) {
                String name = file.getName();
                if (!name.startsWith(head)) {
                    continue;
                }
                int dot = name.lastIndexOf('.');
                String stem = dot > head.length()
                        ? name.substring(head.length(), dot)
                        : name.substring(head.length());
                try {
                    max = Math.max(max, Integer.parseInt(stem));
                } catch (NumberFormatException ignored) {
                    // Old timestamp or unrelated name: not part of the serial.
                }
            }
        }
        for (int index = max + 1; index < max + 4096; index++) {
            File candidate = new File(directory, prefix + "_" + index + "." + extension);
            try {
                if (candidate.createNewFile()) {
                    return candidate;
                }
            } catch (IOException error) {
                return null;
            }
        }
        return null;
    }

    /**
     * Begin a voice clip. The permission prompt is asynchronous; the granted
     * callback starts the recorder and publishes "recording", which the Rust
     * tick consumes.
     *
     * @return {@code recording}, {@code permission}, or {@code failed:<reason>}
     */
    public static String startVoice() {
        Activity current = activity.get();
        if (current == null) {
            return "failed:no activity";
        }
        if (voiceRecording) {
            return "recording";
        }
        if (current.checkSelfPermission(Manifest.permission.RECORD_AUDIO)
                != PackageManager.PERMISSION_GRANTED) {
            pendingVoicePermission = true;
            pendingSystemVoice = false;
            current.runOnUiThread(() -> current.requestPermissions(
                    new String[]{Manifest.permission.RECORD_AUDIO}, REQUEST_RECORD_AUDIO));
            return "permission";
        }
        startVoiceThread(current.getApplicationContext());
        return "recording";
    }

    /** Ask the capture thread to stop; the WAV lands in the event queue. */
    public static String stopVoice() {
        synchronized (voiceLock) {
            if (!voiceRecording) {
                return "failed:not recording";
            }
            voiceStopRequested = true;
        }
        return "stopping";
    }

    /**
     * Drop the clip in flight. Nothing is written and nothing is transcribed:
     * the user slid up to cancel. The capture thread still publishes
     * {@code cancelled} so the Rust side can settle its state.
     */
    public static String cancelVoice() {
        synchronized (voiceLock) {
            if (!voiceRecording) {
                return "failed:not recording";
            }
            voiceCancelRequested = true;
            voiceStopRequested = true;
        }
        return "cancelled";
    }

    /** Whether this device has a speech recognizer we can fall back to. */
    public static String systemVoiceAvailable() {
        Activity current = activity.get();
        if (current == null) {
            return "0";
        }
        try {
            return SpeechRecognizer.isRecognitionAvailable(current) ? "1" : "0";
        } catch (Throwable ignored) {
            return "0";
        }
    }

    /**
     * Start the phone's own recognizer. Chosen by Rust only when no ASR model
     * is configured; it returns text but no audio, so nothing is transcribable
     * a second time.
     *
     * @return {@code listening}, {@code permission}, or {@code failed:<reason>}
     */
    public static String startSystemVoice() {
        Activity current = activity.get();
        if (current == null) {
            return "failed:no activity";
        }
        if (current.checkSelfPermission(Manifest.permission.RECORD_AUDIO)
                != PackageManager.PERMISSION_GRANTED) {
            pendingVoicePermission = true;
            pendingSystemVoice = true;
            current.runOnUiThread(() -> current.requestPermissions(
                    new String[]{Manifest.permission.RECORD_AUDIO}, REQUEST_RECORD_AUDIO));
            return "permission";
        }
        current.runOnUiThread(() -> beginSystemListening(current));
        return "listening";
    }

    /** Ask the recognizer to finish; the text arrives as {@code system|done|}. */
    public static String stopSystemVoice() {
        Activity current = activity.get();
        if (current == null || speechRecognizer == null) {
            return "failed:not listening";
        }
        current.runOnUiThread(() -> {
            SpeechRecognizer recognizer = speechRecognizer;
            if (recognizer != null) {
                recognizer.stopListening();
            }
        });
        return "stopping";
    }

    /** Abandon the recognition in progress; no result is published. */
    public static String cancelSystemVoice() {
        Activity current = activity.get();
        if (current == null) {
            return "failed:no activity";
        }
        systemCancelled = true;
        current.runOnUiThread(UiBridge::releaseRecognizer);
        return "cancelled";
    }

    /** Drain capture events: recording / denied / done|path / error|message,
     *  plus cancelled and system|done|text / system|error|message. */
    public static String[] takeVoiceEvents() {
        synchronized (queueLock) {
            if (voiceEvents.isEmpty()) {
                return null;
            }
            String[] events = voiceEvents.toArray(new String[0]);
            voiceEvents.clear();
            return events;
        }
    }

    static void handlePermissionResult(int requestCode, int[] grantResults) {
        if (requestCode != REQUEST_RECORD_AUDIO || !pendingVoicePermission) {
            return;
        }
        pendingVoicePermission = false;
        boolean system = pendingSystemVoice;
        pendingSystemVoice = false;
        boolean granted = grantResults.length > 0
                && grantResults[0] == PackageManager.PERMISSION_GRANTED;
        if (!granted) {
            publishVoice(system ? "system|error|录音权限被拒绝" : "denied");
            return;
        }
        Activity current = activity.get();
        if (current == null) {
            publishVoice(system ? "system|error|界面已关闭" : "error:界面已关闭");
            return;
        }
        if (system) {
            beginSystemListening(current);
        } else {
            startVoiceThread(current.getApplicationContext());
        }
    }

    /**
     * Create the recognizer on the UI thread (the framework requires it) and
     * start one utterance. On-device recognition is preferred where the
     * platform exposes it, so an offline phone still works.
     */
    private static void beginSystemListening(Activity current) {
        if (systemListening) {
            return;
        }
        try {
            releaseRecognizer();
            boolean onDevice = Build.VERSION.SDK_INT >= 33
                    && SpeechRecognizer.isOnDeviceRecognitionAvailable(current);
            SpeechRecognizer recognizer = onDevice
                    ? SpeechRecognizer.createOnDeviceSpeechRecognizer(current)
                    : SpeechRecognizer.createSpeechRecognizer(current);
            recognizer.setRecognitionListener(new SystemListener());
            Intent intent = new Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH);
            intent.putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL,
                    RecognizerIntent.LANGUAGE_MODEL_FREE_FORM);
            intent.putExtra(RecognizerIntent.EXTRA_LANGUAGE,
                    Locale.getDefault().toLanguageTag());
            intent.putExtra(RecognizerIntent.EXTRA_MAX_RESULTS, 1);
            intent.putExtra(RecognizerIntent.EXTRA_PARTIAL_RESULTS, false);
            // 只有真走 on-device 时才要求离线：老机器上强制离线会直接
            // 撞上“语言包不可用”，宁可让它用在线服务识别。
            intent.putExtra(RecognizerIntent.EXTRA_PREFER_OFFLINE, onDevice);
            systemCancelled = false;
            speechRecognizer = recognizer;
            systemListening = true;
            recognizer.startListening(intent);
            publishVoice("system|listening");
        } catch (Throwable failure) {
            releaseRecognizer();
            publishVoice("system|error|"
                    + (failure.getMessage() == null ? "系统识别不可用" : failure.getMessage()));
        }
    }

    private static void releaseRecognizer() {
        systemListening = false;
        SpeechRecognizer recognizer = speechRecognizer;
        speechRecognizer = null;
        if (recognizer == null) {
            return;
        }
        try {
            recognizer.cancel();
        } catch (Throwable ignored) {
        }
        try {
            recognizer.destroy();
        } catch (Throwable ignored) {
        }
    }

    /** Turns recognizer callbacks into the same event queue as the recorder. */
    private static final class SystemListener implements RecognitionListener {
        @Override
        public void onReadyForSpeech(Bundle params) {
        }

        @Override
        public void onBeginningOfSpeech() {
        }

        @Override
        public void onRmsChanged(float rmsDb) {
        }

        @Override
        public void onBufferReceived(byte[] buffer) {
        }

        @Override
        public void onEndOfSpeech() {
        }

        @Override
        public void onError(int error) {
            boolean cancelled = systemCancelled;
            releaseRecognizer();
            if (cancelled) {
                return;
            }
            publishVoice("system|error|" + systemErrorText(error));
        }

        @Override
        public void onResults(Bundle results) {
            releaseRecognizer();
            ArrayList<String> matches = results == null
                    ? null
                    : results.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION);
            String best = matches == null || matches.isEmpty() ? "" : matches.get(0);
            publishVoice("system|done|" + (best == null ? "" : best.replace('\n', ' ')));
        }

        @Override
        public void onPartialResults(Bundle partialResults) {
        }

        @Override
        public void onEvent(int eventType, Bundle params) {
        }
    }

    private static String systemErrorText(int error) {
        switch (error) {
            case SpeechRecognizer.ERROR_NO_MATCH:
                return "没有听清";
            case SpeechRecognizer.ERROR_SPEECH_TIMEOUT:
                return "没有听到声音";
            case SpeechRecognizer.ERROR_NETWORK:
            case SpeechRecognizer.ERROR_NETWORK_TIMEOUT:
                return "识别服务需要联网，当前不可用";
            case SpeechRecognizer.ERROR_SERVER:
            case SpeechRecognizer.ERROR_SERVER_DISCONNECTED:
                return "识别服务暂时不可用";
            case SpeechRecognizer.ERROR_INSUFFICIENT_PERMISSIONS:
                return "录音权限被拒绝";
            case SpeechRecognizer.ERROR_LANGUAGE_NOT_SUPPORTED:
            case SpeechRecognizer.ERROR_LANGUAGE_UNAVAILABLE:
                return "设备缺少当前语言的识别包";
            case SpeechRecognizer.ERROR_CLIENT:
                return "识别已中断";
            default:
                return "系统识别失败（" + error + "）";
        }
    }

    private static void startVoiceThread(Context context) {
        synchronized (voiceLock) {
            if (voiceRecording) {
                return;
            }
            voiceRecording = true;
            voiceStopRequested = false;
            voiceCancelRequested = false;
            publishVoice("recording");
            new Thread(() -> recordVoice(context), "wunder-voice").start();
        }
    }

    private static void recordVoice(Context context) {
        String error = null;
        ByteArrayOutputStream pcm = new ByteArrayOutputStream(VOICE_RATE * 2 * 8);
        AudioRecord recorder = null;
        try {
            int minimum = AudioRecord.getMinBufferSize(
                    VOICE_RATE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT);
            int buffer = Math.max(minimum > 0 ? minimum : 0, VOICE_RATE / 2);
            recorder = new AudioRecord(
                    MediaRecorder.AudioSource.MIC,
                    VOICE_RATE,
                    AudioFormat.CHANNEL_IN_MONO,
                    AudioFormat.ENCODING_PCM_16BIT,
                    buffer);
            if (recorder.getState() != AudioRecord.STATE_INITIALIZED) {
                throw new IOException("麦克风不可用");
            }
            recorder.startRecording();
            byte[] chunk = new byte[VOICE_RATE / 10 * 2];
            int limit = VOICE_RATE * 2 * VOICE_MAX_SECONDS;
            while (!voiceStopRequested && pcm.size() < limit) {
                int read = recorder.read(chunk, 0, chunk.length);
                if (read > 0) {
                    pcm.write(chunk, 0, read);
                } else if (read < 0) {
                    throw new IOException("读取麦克风失败");
                }
            }
        } catch (Throwable failure) {
            error = String.valueOf(failure.getMessage() == null ? failure : failure.getMessage());
        } finally {
            if (recorder != null) {
                try {
                    recorder.stop();
                } catch (Throwable ignored) {
                }
                recorder.release();
            }
            voiceRecording = false;
        }
        // 上滑取消就不写 WAV：用户不要这段录音，缓存里也不该留。
        if (voiceCancelRequested) {
            publishVoice("cancelled");
            return;
        }
        if (error != null && pcm.size() == 0) {
            publishVoice("error|" + error);
            return;
        }
        try {
            File file = writeVoiceWav(context, pcm.toByteArray());
            publishVoice("done|" + file.getAbsolutePath());
        } catch (IOException failure) {
            publishVoice("error|" + failure.getMessage());
        }
    }

    private static File writeVoiceWav(Context context, byte[] pcm) throws IOException {
        File directory = new File(context.getCacheDir(), "wunder-voice");
        if (!directory.isDirectory() && !directory.mkdirs()) {
            throw new IOException("无法创建录音目录");
        }
        File file = new File(directory, "voice-" + System.currentTimeMillis() + ".wav");
        int dataLength = pcm.length;
        try (OutputStream output = new FileOutputStream(file)) {
            output.write(new byte[]{'R', 'I', 'F', 'F'});
            writeIntLe(output, 36 + dataLength);
            output.write(new byte[]{'W', 'A', 'V', 'E', 'f', 'm', 't', ' '});
            writeIntLe(output, 16);
            writeShortLe(output, 1);
            writeShortLe(output, 1);
            writeIntLe(output, VOICE_RATE);
            writeIntLe(output, VOICE_RATE * 2);
            writeShortLe(output, 2);
            writeShortLe(output, 16);
            output.write(new byte[]{'d', 'a', 't', 'a'});
            writeIntLe(output, dataLength);
            output.write(pcm);
        }
        File[] leftovers = directory.listFiles();
        if (leftovers != null && leftovers.length > KEEP_VOICE_FILES) {
            Arrays.sort(leftovers, Comparator.comparingLong(File::lastModified));
            int remove = leftovers.length - KEEP_VOICE_FILES;
            for (int index = 0; index < remove; index++) {
                //noinspection ResultOfMethodCallIgnored
                leftovers[index].delete();
            }
        }
        return file;
    }

    private static void writeIntLe(OutputStream output, int value) throws IOException {
        output.write(value & 0xff);
        output.write((value >> 8) & 0xff);
        output.write((value >> 16) & 0xff);
        output.write((value >> 24) & 0xff);
    }

    private static void writeShortLe(OutputStream output, int value) throws IOException {
        output.write(value & 0xff);
        output.write((value >> 8) & 0xff);
    }

    private static void publishVoice(String event) {
        synchronized (queueLock) {
            voiceEvents.add(event);
        }
    }

    /**
     * Copy one workspace file out of the sandbox. Modern devices write
     * straight to Downloads (no permission needed); older ones fall back to
     * the system save dialog and report through {@code nativeSaveFinished}.
     *
     * @return {@code saved}, {@code pending}, or {@code failed:<reason>}
     */
    public static String saveFile(String sourcePath, String displayName) {
        Activity current = activity.get();
        if (current == null) {
            return "failed:no activity";
        }
        Context context = current.getApplicationContext();
        if (Build.VERSION.SDK_INT >= 29) {
            try {
                saveToDownloads(context, sourcePath, displayName);
                return "saved";
            } catch (Exception error) {
                return "failed:" + error.getMessage();
            }
        }
        pendingSaveSource = sourcePath;
        pendingSaveName = displayName;
        current.runOnUiThread(() -> {
            Intent intent = new Intent(Intent.ACTION_CREATE_DOCUMENT);
            intent.addCategory(Intent.CATEGORY_OPENABLE);
            intent.setType(mimeOf(displayName));
            intent.putExtra(Intent.EXTRA_TITLE, displayName);
            try {
                current.startActivityForResult(intent, REQUEST_SAVE_FILE);
            } catch (Exception ignored) {
                pendingSaveSource = null;
                publishSave(false, displayName);
            }
        });
        return "pending";
    }

    /** Human-readable device name used when this phone registers with the cloud. */
    public static String deviceName() {
        String manufacturer = Build.MANUFACTURER == null ? "" : Build.MANUFACTURER.trim();
        String model = Build.MODEL == null ? "" : Build.MODEL.trim();
        String name = manufacturer.isEmpty() ? model
                : model.toLowerCase(Locale.ROOT).startsWith(manufacturer.toLowerCase(Locale.ROOT))
                    ? model : manufacturer + " " + model;
        return name.isEmpty() ? "wunder 安卓版" : name;
    }

    /** Drain files staged by the latest picker result, or null if none. */
    public static String[] takePickedFiles() {
        synchronized (queueLock) {
            if (pickedPaths == null) {
                return null;
            }
            String[] paths = pickedPaths.toArray(new String[0]);
            pickedPaths = null;
            return paths;
        }
    }

    /** Drain the legacy save dialog result, or null if none. */
    public static String takeSaveResult() {
        synchronized (queueLock) {
            String result = saveResult;
            saveResult = null;
            return result;
        }
    }

    static void handleActivityResult(Activity current, int requestCode, int resultCode, Intent data) {
        if (requestCode == REQUEST_PICK_FILES) {
            stagePickedFiles(current, resultCode, data);
        } else if (requestCode == REQUEST_PICK_DIRECTORY) {
            stagePickedDirectory(current, resultCode, data);
        } else if (requestCode == REQUEST_TAKE_PHOTO) {
            stageCameraPhoto();
        } else if (requestCode == REQUEST_SAVE_FILE) {
            finishSave(current, resultCode, data);
        }
    }

    /**
     * Hand the captured file to the engine. A few ROMs answer with
     * {@code RESULT_CANCELED} after writing the full image, so the file itself
     * is the source of truth: bytes on disk mean the shot happened.
     */
    private static void stageCameraPhoto() {
        File file = pendingCameraFile;
        pendingCameraFile = null;
        boolean captured = file != null && file.isFile() && file.length() > 0;
        if (!captured) {
            if (file != null) {
                //noinspection ResultOfMethodCallIgnored
                file.delete();
            }
            publishPick(new ArrayList<>());
            return;
        }
        publishPick(singlePath(file.getAbsolutePath()));
    }

    private static ArrayList<String> singlePath(String value) {
        ArrayList<String> list = new ArrayList<>();
        list.add(value);
        return list;
    }

    private static void stagePickedDirectory(Activity current, int resultCode, Intent data) {
        if (resultCode != Activity.RESULT_OK || data == null || data.getData() == null) {
            publishDirectory("");
            return;
        }
        Uri uri = data.getData();
        String path = treeUriToPath(uri);
        if (path == null) {
            publishDirectory("!暂不支持该目录，请选择内部存储或 SD 卡中的文件夹");
            return;
        }
        File directory = new File(path);
        if (!directory.isDirectory()) {
            publishDirectory("!目录不存在或不可访问");
            return;
        }
        if (!hasSharedStorageAccess(current, path)) {
            publishDirectory("!没有该目录的读写权限，请先在权限管理中开启「所有文件访问」");
            return;
        }
        if (!directory.canRead() || !directory.canWrite()) {
            publishDirectory("!没有该目录的读写权限，请先在权限管理中开启「所有文件访问」");
            return;
        }
        try {
            current.getContentResolver().takePersistableUriPermission(
                    uri,
                    Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION);
        } catch (Exception ignored) {
            // Direct file access is what matters; a provider that refuses the
            // persistable grant does not change the resolved host path.
        }
        publishDirectory(path);
    }

    /**
     * Map a document-tree URI to its host path. Only the external-storage
     * provider is accepted: arbitrary providers (cloud drives, archives) have
     * no filesystem path the engine or proot could open.
     */
    private static String treeUriToPath(Uri uri) {
        if (uri == null) {
            return null;
        }
        String authority = uri.getAuthority();
        if (authority == null) {
            return null;
        }
        String docId;
        try {
            docId = DocumentsContract.getTreeDocumentId(uri);
        } catch (Exception ignored) {
            return null;
        }
        if (docId == null || docId.isEmpty()) {
            return null;
        }
        if (!authority.equals("com.android.externalstorage.documents")) {
            // The downloads provider uses raw: /storage/... document ids.
            if (docId.startsWith("raw:")) {
                return verifyExternalPath(docId.substring(4));
            }
            return null;
        }
        int colon = docId.indexOf(':');
        if (colon <= 0) {
            return null;
        }
        String volume = docId.substring(0, colon);
        String relative = docId.substring(colon + 1);
        String base;
        if (volume.equalsIgnoreCase("primary")) {
            File external = Environment.getExternalStorageDirectory();
            if (external == null) {
                return null;
            }
            base = external.getAbsolutePath();
        } else {
            base = "/storage/" + volume;
        }
        File target = relative.isEmpty() ? new File(base) : new File(base, relative);
        return verifyExternalPath(target.getAbsolutePath());
    }

    /**
     * Direct File access to shared storage. The app-specific external folder
     * needs no grant; everything else is the all-files gate on Android 11+ or
     * the legacy READ/WRITE pair below it.
     */
    private static boolean hasSharedStorageAccess(Activity current, String path) {
        String packageName = current.getPackageName();
        if (path.contains("/Android/data/" + packageName + "/")
                || path.contains("/Android/media/" + packageName + "/")) {
            return true;
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            return Environment.isExternalStorageManager();
        }
        boolean read = current.checkSelfPermission(Manifest.permission.READ_EXTERNAL_STORAGE)
                == PackageManager.PERMISSION_GRANTED;
        boolean write = Build.VERSION.SDK_INT > Build.VERSION_CODES.Q
                || current.checkSelfPermission(Manifest.permission.WRITE_EXTERNAL_STORAGE)
                        == PackageManager.PERMISSION_GRANTED;
        return read && write;
    }

    /** Keep the resolved path inside shared storage, not an arbitrary mount. */
    private static String verifyExternalPath(String rawPath) {        if (rawPath == null || rawPath.isEmpty()) {
            return null;
        }
        try {
            File target = new File(rawPath).getCanonicalFile();
            if (!target.isAbsolute()) {
                return null;
            }
            String path = target.getAbsolutePath();
            if (!path.startsWith("/storage/")) {
                return null;
            }
            return path;
        } catch (Exception ignored) {
            return null;
        }
    }

    private static void stagePickedFiles(Activity current, int resultCode, Intent data) {
        if (resultCode != Activity.RESULT_OK || data == null) {
            publishPick(new ArrayList<>());
            return;
        }
        ArrayList<Uri> uris = new ArrayList<>();
        ClipData clip = data.getClipData();
        if (clip != null) {
            for (int index = 0; index < clip.getItemCount(); index++) {
                Uri uri = clip.getItemAt(index).getUri();
                if (uri != null) {
                    uris.add(uri);
                }
            }
        } else if (data.getData() != null) {
            uris.add(data.getData());
        }
        if (uris.isEmpty()) {
            publishPick(new ArrayList<>());
            return;
        }
        Context context = current.getApplicationContext();
        new Thread(() -> {
            File root = new File(context.getCacheDir(), "wunder-picked");
            File[] leftovers = root.listFiles();
            if (leftovers != null) {
                for (File leftover : leftovers) {
                    deleteTree(leftover);
                }
            }
            // One directory per pick keeps the original display names while
            // avoiding collisions with files staged by an earlier pick.
            File staging = new File(root, String.valueOf(System.currentTimeMillis()));
            //noinspection ResultOfMethodCallIgnored
            staging.mkdirs();
            ArrayList<String> paths = new ArrayList<>();
            for (Uri uri : uris) {
                if (paths.size() >= MAX_PICKED) {
                    break;
                }
                String name = displayName(context, uri);
                File target = uniqueTarget(staging, safeName(name));
                try (InputStream input = context.getContentResolver().openInputStream(uri);
                     OutputStream output = new FileOutputStream(target)) {
                    if (input == null) {
                        continue;
                    }
                    copy(input, output);
                    paths.add(target.getAbsolutePath());
                } catch (Exception ignored) {
                    //noinspection ResultOfMethodCallIgnored
                    target.delete();
                }
            }
            publishPick(paths);
        }, "wunder-pick").start();
    }

    private static void finishSave(Activity current, int resultCode, Intent data) {
        String source = pendingSaveSource;
        String name = pendingSaveName;
        pendingSaveSource = null;
        pendingSaveName = null;
        Uri target = resultCode == Activity.RESULT_OK && data != null ? data.getData() : null;
        if (source == null) {
            return;
        }
        if (target == null) {
            publishSave(false, name == null ? "" : name);
            return;
        }
        Context context = current.getApplicationContext();
        new Thread(() -> {
            boolean saved = false;
            try (InputStream input = new FileInputStream(source);
                 OutputStream output = context.getContentResolver().openOutputStream(target)) {
                if (output != null) {
                    copy(input, output);
                    saved = true;
                }
            } catch (Exception ignored) {
            }
            publishSave(saved, name == null ? "" : name);
        }, "wunder-save").start();
    }

    private static void saveToDownloads(Context context, String sourcePath, String displayName) throws IOException {
        ContentValues values = new ContentValues();
        values.put(MediaStore.MediaColumns.DISPLAY_NAME, displayName);
        values.put(MediaStore.MediaColumns.MIME_TYPE, mimeOf(displayName));
        values.put(MediaStore.MediaColumns.RELATIVE_PATH, Environment.DIRECTORY_DOWNLOADS + "/wunder 安卓版");
        Uri target = context.getContentResolver()
                .insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values);
        if (target == null) {
            throw new IOException("无法创建下载文件");
        }
        try (InputStream input = new FileInputStream(sourcePath);
             OutputStream output = context.getContentResolver().openOutputStream(target)) {
            if (output == null) {
                throw new IOException("无法写入下载目录");
            }
            copy(input, output);
        } catch (IOException error) {
            context.getContentResolver().delete(target, null, null);
            throw error;
        }
    }

    private static String displayName(Context context, Uri uri) {
        try (Cursor cursor = context.getContentResolver().query(uri, null, null, null, null)) {
            if (cursor != null && cursor.moveToFirst()) {
                int column = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME);
                if (column >= 0) {
                    String name = cursor.getString(column);
                    if (name != null && !name.trim().isEmpty()) {
                        return name;
                    }
                }
            }
        } catch (Exception ignored) {
        }
        return "file";
    }

    private static File uniqueTarget(File directory, String name) {
        File target = new File(directory, name);
        if (!target.exists()) {
            return target;
        }
        int dot = name.lastIndexOf('.');
        String stem = dot > 0 ? name.substring(0, dot) : name;
        String extension = dot > 0 ? name.substring(dot) : "";
        for (int index = 2; index < 1000; index++) {
            target = new File(directory, stem + " (" + index + ")" + extension);
            if (!target.exists()) {
                return target;
            }
        }
        return new File(directory, System.currentTimeMillis() + "-" + name);
    }

    private static void deleteTree(File file) {
        if (file.isDirectory()) {
            File[] children = file.listFiles();
            if (children != null) {
                for (File child : children) {
                    deleteTree(child);
                }
            }
        }
        //noinspection ResultOfMethodCallIgnored
        file.delete();
    }

    private static String safeName(String name) {
        String cleaned = name.replaceAll("[\\\\/:*?\"<>|\\p{Cntrl}]", "_").trim();
        if (cleaned.isEmpty()) {
            cleaned = "file";
        }
        return cleaned.length() > 96 ? cleaned.substring(0, 96) : cleaned;
    }

    private static String mimeOf(String name) {
        int dot = name.lastIndexOf('.');
        if (dot >= 0 && dot + 1 < name.length()) {
            String mime = MimeTypeMap.getSingleton()
                    .getMimeTypeFromExtension(name.substring(dot + 1).toLowerCase(Locale.ROOT));
            if (mime != null) {
                return mime;
            }
        }
        return "application/octet-stream";
    }

    private static void copy(InputStream input, OutputStream output) throws IOException {
        byte[] buffer = new byte[64 * 1024];
        int read;
        while ((read = input.read(buffer)) != -1) {
            output.write(buffer, 0, read);
        }
        output.flush();
    }

    private static void publishPick(ArrayList<String> paths) {
        synchronized (queueLock) {
            pickedPaths = new ArrayList<>(paths);
        }
    }

    private static void publishDirectory(String path) {
        synchronized (queueLock) {
            pickedDirectory = path == null ? "" : path;
        }
    }

    private static void publishSave(boolean success, String displayName) {
        synchronized (queueLock) {
            saveResult = (success ? "1|" : "0|") + displayName;
        }
    }
}
