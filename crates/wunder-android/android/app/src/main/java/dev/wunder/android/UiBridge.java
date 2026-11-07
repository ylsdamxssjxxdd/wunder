package dev.wunder.android;

import android.Manifest;
import android.app.Activity;
import android.content.ClipData;
import android.content.ContentValues;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.database.Cursor;
import android.media.AudioFormat;
import android.media.AudioRecord;
import android.media.MediaRecorder;
import android.net.Uri;
import android.os.Build;
import android.os.Environment;
import android.provider.MediaStore;
import android.provider.OpenableColumns;
import android.view.View;
import android.view.inputmethod.InputMethodManager;
import android.webkit.MimeTypeMap;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.lang.ref.WeakReference;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Comparator;
import java.util.Locale;

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
    private static final int REQUEST_SAVE_FILE = 0x5702;
    private static final int REQUEST_RECORD_AUDIO = 0x5703;
    // 桌面版 caps one drop at twenty files; keep the same bound on the phone.
    private static final int MAX_PICKED = 20;
    // Same capture contract as the desktop recorder: 16 kHz mono PCM WAV,
    // two minutes max.
    private static final int VOICE_RATE = 16_000;
    private static final int VOICE_MAX_SECONDS = 120;
    private static final int KEEP_VOICE_FILES = 2;

    private static WeakReference<Activity> activity = new WeakReference<>(null);
    private static String pendingSaveSource;
    private static String pendingSaveName;
    // Results wait here until the engine's UI tick polls them. Java must not
    // call into the native library: NativeActivity loads it through the
    // framework class loader, so app classes cannot resolve its JNI symbols.
    private static final Object queueLock = new Object();
    private static ArrayList<String> pickedPaths; // null = no result pending
    private static String saveResult; // null or "1|name" / "0|name"
    private static final ArrayList<String> voiceEvents = new ArrayList<>();
    private static final ArrayList<String> screenshotEvents = new ArrayList<>();
    private static final Object voiceLock = new Object();
    private static volatile boolean voiceRecording;
    private static volatile boolean voiceStopRequested;
    private static boolean pendingVoicePermission;

    private UiBridge() {}

    public static void attach(Activity current) {
        activity = new WeakReference<>(current);
    }

    /** Dismiss the soft keyboard, e.g. when the plus function panel opens. */
    public static void hideKeyboard() {
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

    /** Drain capture events: recording / denied / done|path / error|message. */
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

    /**
     * Capture the current screen through the accessibility service. The caller
     * closes the function panel first, so the frame is what the user was
     * looking at.
     *
     * @return {@code pending} or {@code failed:<reason>}
     */
    public static String captureScreenshot() {
        Activity current = activity.get();
        if (current == null) {
            return "failed:no activity";
        }
        if (WunderAccessibilityService.getInstance() == null) {
            publishScreenshot("no_service");
            return "pending";
        }
        Context context = current.getApplicationContext();
        new Thread(() -> {
            String path = PhoneBridge.captureScreenForUi(context);
            publishScreenshot(path == null ? "error:无法截取屏幕" : "ok|" + path);
        }, "wunder-shot").start();
        return "pending";
    }

    /** Drain screenshot results: ok|path / no_service / error|message. */
    public static String[] takeScreenshotEvents() {
        synchronized (queueLock) {
            if (screenshotEvents.isEmpty()) {
                return null;
            }
            String[] events = screenshotEvents.toArray(new String[0]);
            screenshotEvents.clear();
            return events;
        }
    }

    static void handlePermissionResult(int requestCode, int[] grantResults) {
        if (requestCode != REQUEST_RECORD_AUDIO || !pendingVoicePermission) {
            return;
        }
        pendingVoicePermission = false;
        boolean granted = grantResults.length > 0
                && grantResults[0] == PackageManager.PERMISSION_GRANTED;
        if (!granted) {
            publishVoice("denied");
            return;
        }
        Activity current = activity.get();
        if (current == null) {
            publishVoice("error:界面已关闭");
            return;
        }
        startVoiceThread(current.getApplicationContext());
    }

    private static void startVoiceThread(Context context) {
        synchronized (voiceLock) {
            if (voiceRecording) {
                return;
            }
            voiceRecording = true;
            voiceStopRequested = false;
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

    private static void publishScreenshot(String event) {
        synchronized (queueLock) {
            screenshotEvents.add(event);
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
        } else if (requestCode == REQUEST_SAVE_FILE) {
            finishSave(current, resultCode, data);
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

    private static void publishSave(boolean success, String displayName) {
        synchronized (queueLock) {
            saveResult = (success ? "1|" : "0|") + displayName;
        }
    }
}
