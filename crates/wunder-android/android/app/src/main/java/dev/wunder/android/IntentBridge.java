package dev.wunder.android;

import android.content.ClipData;
import android.content.Context;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.os.Build;
import android.provider.OpenableColumns;
import android.util.Log;

import org.json.JSONArray;
import org.json.JSONObject;

import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Locale;
import java.util.Set;

/**
 * System share / VIEW intake.
 *
 * <p>The activity receives the intent before the Rust engine is ready (cold
 * start from a share sheet) and again while it runs (singleTask
 * {@code onNewIntent}). The payload is normalised here, copied off the UI
 * thread into {@code filesDir/wunder/shared/<timestamp>/}, and parked in a
 * small static queue that the Rust render tick drains through JNI. Java must
 * not call into the native library: NativeActivity loads it through the
 * framework class loader, so app classes cannot resolve its JNI symbols.
 *
 * <p>Queue records are JSON: {@code {"kind":"text","text":"..."}},
 * {@code {"kind":"view","url":"https://..."}} or
 * {@code {"kind":"files","files":["/abs/path",...],"text":"..."}}.
 */
public final class IntentBridge {
    private static final String TAG = "wunder-share";
    // A shared body is pasted into the composer; 2 MB is far beyond any real
    // note yet small enough to keep the Slint text model responsive.
    private static final int MAX_TEXT_BYTES = 2 * 1024 * 1024;
    private static final int MAX_FILES = 8;
    // Bounded queue: a burst of shares must not grow the process forever.
    private static final int MAX_PENDING = 8;
    private static final long STAGING_MAX_AGE_MS = 7L * 24 * 60 * 60 * 1000;
    private static final int STAGING_MAX_DIRS = 32;

    private static final Object queueLock = new Object();
    private static final ArrayDeque<String> pending = new ArrayDeque<>();

    private IntentBridge() {}

    /** Normalise one launch/new intent. Safe to call from the UI thread. */
    public static void handleIntent(Context context, Intent intent) {
        if (context == null || intent == null) {
            return;
        }
        // Recents can restore the original share intent; replaying it would
        // duplicate text the engine already consumed.
        if ((intent.getFlags() & Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY) != 0) {
            return;
        }
        final Context app = applicationContext(context);
        String action = intent.getAction();
        if (action == null) {
            return;
        }
        if (Intent.ACTION_SEND.equals(action) || Intent.ACTION_SEND_MULTIPLE.equals(action)) {
            final String text = boundedText(intent.getCharSequenceExtra(Intent.EXTRA_TEXT));
            List<Uri> uris = collectStreams(intent);
            if (!uris.isEmpty()) {
                copyOnWorker(app, uris, text);
                return;
            }
            if (text != null) {
                enqueue(textRecord(text));
            }
            return;
        }
        if (Intent.ACTION_VIEW.equals(action)) {
            Uri data = intent.getData();
            if (data == null) {
                return;
            }
            String scheme = data.getScheme();
            if (scheme == null) {
                return;
            }
            scheme = scheme.toLowerCase(Locale.ROOT);
            if (!scheme.equals("http") && !scheme.equals("https")) {
                return;
            }
            try {
                JSONObject record = new JSONObject();
                record.put("kind", "view");
                record.put("url", data.toString());
                enqueue(record.toString());
            } catch (Exception ignored) {
            }
        }
    }

    /** Drain one queued share; null until the activity has delivered one. */
    public static String takeSharedIntent() {
        synchronized (queueLock) {
            return pending.pollFirst();
        }
    }

    private static Context applicationContext(Context context) {
        Context app = context.getApplicationContext();
        return app == null ? context : app;
    }

    private static String boundedText(CharSequence text) {
        if (text == null) {
            return null;
        }
        String value = text.toString();
        if (value.trim().isEmpty()) {
            return null;
        }
        byte[] bytes = value.getBytes(StandardCharsets.UTF_8);
        if (bytes.length <= MAX_TEXT_BYTES) {
            return value;
        }
        // Truncate on a code-point boundary so the payload stays valid UTF-8.
        int low = 0;
        int high = value.length();
        while (low < high) {
            int mid = (low + high + 1) >>> 1;
            if (value.substring(0, mid).getBytes(StandardCharsets.UTF_8).length <= MAX_TEXT_BYTES) {
                low = mid;
            } else {
                high = mid - 1;
            }
        }
        // Never cut a surrogate pair: JSON and the Slint text model both need
        // complete code points.
        if (low > 0 && Character.isHighSurrogate(value.charAt(low - 1))) {
            low--;
        }
        return value.substring(0, low);
    }

    private static String textRecord(String text) {
        try {
            JSONObject record = new JSONObject();
            record.put("kind", "text");
            record.put("text", text);
            return record.toString();
        } catch (Exception ignored) {
            return null;
        }
    }

    private static List<Uri> collectStreams(Intent intent) {
        Set<Uri> unique = new LinkedHashSet<>();
        ClipData clip = intent.getClipData();
        if (clip != null) {
            for (int index = 0; index < clip.getItemCount() && unique.size() < MAX_FILES; index++) {
                Uri uri = clip.getItemAt(index).getUri();
                if (uri != null) {
                    unique.add(uri);
                }
            }
        }
        if (unique.isEmpty()) {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                Uri single = intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri.class);
                if (single != null) {
                    unique.add(single);
                }
            } else {
                //noinspection deprecation
                Uri single = intent.getParcelableExtra(Intent.EXTRA_STREAM);
                if (single != null) {
                    unique.add(single);
                }
            }
        }
        if (unique.isEmpty()) {
            //noinspection deprecation
            ArrayList<Uri> list = intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM);
            if (list != null) {
                for (Uri uri : list) {
                    if (unique.size() >= MAX_FILES) {
                        break;
                    }
                    if (uri != null) {
                        unique.add(uri);
                    }
                }
            }
        }
        return new ArrayList<>(unique);
    }

    private static void copyOnWorker(final Context context, final List<Uri> uris, final String text) {
        new Thread(() -> {
            File root = new File(context.getFilesDir(), "wunder/shared");
            File staging = uniqueStaging(root);
            if (staging == null) {
                Log.w(TAG, "cannot create the shared staging directory");
                return;
            }
            ArrayList<String> paths = new ArrayList<>();
            for (Uri uri : uris) {
                if (paths.size() >= MAX_FILES) {
                    break;
                }
                String name = safeName(displayName(context, uri));
                File target = uniqueTarget(staging, name);
                try (InputStream input = context.getContentResolver().openInputStream(uri);
                     OutputStream output = new FileOutputStream(target)) {
                    if (input == null) {
                        //noinspection ResultOfMethodCallIgnored
                        target.delete();
                        continue;
                    }
                    copy(input, output);
                    paths.add(target.getAbsolutePath());
                } catch (Exception error) {
                    //noinspection ResultOfMethodCallIgnored
                    target.delete();
                    Log.w(TAG, "copy a shared file: " + error.getMessage());
                }
            }
            if (paths.isEmpty()) {
                deleteTree(staging);
                return;
            }
            try {
                JSONObject record = new JSONObject();
                record.put("kind", "files");
                JSONArray array = new JSONArray();
                for (String path : paths) {
                    array.put(path);
                }
                record.put("files", array);
                if (text != null) {
                    record.put("text", text);
                }
                enqueue(record.toString());
                pruneStaging(root);
            } catch (Exception error) {
                Log.w(TAG, "queue a shared file set: " + error.getMessage());
                deleteTree(staging);
            }
        }, "wunder-share").start();
    }

    private static File uniqueStaging(File root) {
        if (!root.isDirectory() && !root.mkdirs()) {
            return null;
        }
        File candidate = new File(root, String.valueOf(System.currentTimeMillis()));
        for (int attempt = 2; attempt < 100; attempt++) {
            if (candidate.mkdirs()) {
                return candidate;
            }
            if (!candidate.exists()) {
                break;
            }
            candidate = new File(root, System.currentTimeMillis() + "-" + attempt);
        }
        return candidate.isDirectory() ? candidate : null;
    }

    /** Keep abandoned staging directories bounded; successful uploads remove their own. */
    private static void pruneStaging(File root) {
        File[] entries = root.listFiles();
        if (entries == null) {
            return;
        }
        ArrayList<File> directories = new ArrayList<>();
        long now = System.currentTimeMillis();
        for (File entry : entries) {
            if (!entry.isDirectory()) {
                continue;
            }
            if (now - entry.lastModified() > STAGING_MAX_AGE_MS) {
                deleteTree(entry);
            } else {
                directories.add(entry);
            }
        }
        directories.sort((left, right) -> Long.compare(left.lastModified(), right.lastModified()));
        while (directories.size() > STAGING_MAX_DIRS) {
            deleteTree(directories.remove(0));
        }
    }

    private static void enqueue(String record) {
        if (record == null) {
            return;
        }
        synchronized (queueLock) {
            while (pending.size() >= MAX_PENDING) {
                pending.pollFirst();
                Log.w(TAG, "shared intent queue full; dropping the oldest record");
            }
            pending.addLast(record);
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
        String fallback = uri.getLastPathSegment();
        if (fallback == null || fallback.trim().isEmpty()) {
            return "file";
        }
        return fallback;
    }

    private static String safeName(String raw) {
        StringBuilder cleaned = new StringBuilder();
        for (int index = 0; index < raw.length() && cleaned.length() < 120; index++) {
            char value = raw.charAt(index);
            if (value == '/' || value == '\\' || Character.isISOControl(value)) {
                cleaned.append('-');
            } else {
                cleaned.append(value);
            }
        }
        String name = cleaned.toString().trim();
        while (name.startsWith(".")) {
            name = name.substring(1);
        }
        name = name.trim();
        return name.isEmpty() ? "file" : name;
    }

    private static File uniqueTarget(File directory, String name) {
        File target = new File(directory, name);
        if (!target.exists()) {
            return target;
        }
        String stem = name;
        String extension = "";
        int dot = name.lastIndexOf('.');
        if (dot > 0) {
            stem = name.substring(0, dot);
            extension = name.substring(dot);
        }
        for (int index = 2; index < 1000; index++) {
            target = new File(directory, stem + " (" + index + ")" + extension);
            if (!target.exists()) {
                return target;
            }
        }
        return new File(directory, System.currentTimeMillis() + "-" + name);
    }

    private static void copy(InputStream input, OutputStream output) throws Exception {
        byte[] buffer = new byte[64 * 1024];
        int read;
        while ((read = input.read(buffer)) != -1) {
            output.write(buffer, 0, read);
        }
    }

    private static void deleteTree(File file) {
        if (file == null) {
            return;
        }
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
}
