package dev.wunder.android;

import android.content.Context;
import android.util.Log;

import java.io.BufferedInputStream;
import java.io.BufferedOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Comparator;
import java.util.Enumeration;
import java.util.Locale;
import java.util.zip.ZipEntry;
import java.util.zip.ZipFile;
import java.util.zip.ZipOutputStream;

/**
 * Local backup and restore for the engine's own state.
 *
 * <p>A backup archives {@code WUNDER_HOME/config} plus the top-level SQLite
 * databases in {@code WUNDER_HOME/data} (including {@code -wal}/{@code -shm}).
 * Workspace contents are deliberately excluded: they are user files, often
 * large, and not engine state.
 *
 * <p>Restore never overwrites a live database. The archive is validated and
 * unpacked into {@code WUNDER_HOME/.restore}, then a marker makes the Rust
 * engine apply it at the start of the next process. The caller must restart
 * the app; the reply {@code restart} says exactly that.
 */
public final class BackupBridge {
    private static final String TAG = "wunder-backup";
    private static final String ZIP_PREFIX = "wunder-backup-";
    private static final String ZIP_SUFFIX = ".zip";
    private static final int KEEP_BACKUPS = 5;
    private static final long MAX_ENTRY_BYTES = 256L * 1024 * 1024;
    private static final long MAX_TOTAL_BYTES = 1024L * 1024 * 1024;
    private static final int MAX_TREE_DEPTH = 32;
    private static final String PENDING_MARKER = "pending";

    private BackupBridge() {}

    /**
     * Package config and databases into a new archive.
     *
     * @return the archive path, or {@code !message} on failure
     */
    public static String backup(Context context) {
        if (context == null) {
            return "!没有应用上下文";
        }
        Context app = applicationContext(context);
        File home = new File(app.getFilesDir(), "wunder");
        if (!home.isDirectory()) {
            return "!本地数据目录不存在";
        }
        File directory = new File(app.getCacheDir(), "backup");
        if (!directory.isDirectory() && !directory.mkdirs()) {
            return "!无法创建备份目录";
        }
        File target = new File(directory,
                ZIP_PREFIX + System.currentTimeMillis() + ZIP_SUFFIX);
        FileOutputStream file = null;
        ZipOutputStream zip = null;
        try {
            file = new FileOutputStream(target);
            zip = new ZipOutputStream(new BufferedOutputStream(file));
            File config = new File(home, "config");
            if (config.isDirectory()) {
                File canonicalConfig = config.getCanonicalFile();
                addTree(zip, canonicalConfig, canonicalConfig, "config", 0);
            }
            File data = new File(home, "data");
            if (data.isDirectory()) {
                File[] databases = data.listFiles();
                if (databases != null) {
                    Arrays.sort(databases, Comparator.comparing(File::getName));
                    for (File database : databases) {
                        if (!database.isFile() || !isDatabaseName(database.getName())) {
                            continue;
                        }
                        addFile(zip, database, "data/" + database.getName());
                    }
                }
            }
            zip.finish();
            zip.flush();
            file.getFD().sync();
        } catch (Exception error) {
            closeQuietly(zip);
            closeQuietly(file);
            //noinspection ResultOfMethodCallIgnored
            target.delete();
            Log.w(TAG, "backup failed: " + messageOf(error));
            return "!" + messageOf(error);
        } finally {
            closeQuietly(zip);
            closeQuietly(file);
        }
        prune(directory);
        return target.getAbsolutePath();
    }

    /**
     * Validate {@code zipPath} and stage its contents for the next start.
     *
     * @return {@code restart} when staged, or {@code !message} on failure
     */
    public static String restore(Context context, String zipPath) {
        if (context == null) {
            return "!没有应用上下文";
        }
        if (zipPath == null || zipPath.trim().isEmpty()) {
            return "!未指定备份文件";
        }
        Context app = applicationContext(context);
        File home = new File(app.getFilesDir(), "wunder");
        File archive;
        try {
            archive = new File(zipPath.trim()).getCanonicalFile();
        } catch (Exception error) {
            return "!备份文件路径无效";
        }
        if (!archive.isFile() || !archive.canRead()) {
            return "!备份文件不存在或不可读";
        }
        File staging = new File(home, ".restore");
        deleteTree(staging);
        if (!staging.mkdirs() && !staging.isDirectory()) {
            return "!无法创建恢复暂存目录";
        }
        long total = 0;
        boolean sawEntry = false;
        try (ZipFile zip = new ZipFile(archive)) {
            Enumeration<? extends ZipEntry> entries = zip.entries();
            while (entries.hasMoreElements()) {
                ZipEntry entry = entries.nextElement();
                String name = entry.getName();
                if (entry.isDirectory()) {
                    continue;
                }
                if (!safeEntryName(name)) {
                    deleteTree(staging);
                    return "!备份包包含不安全路径";
                }
                File entryTarget = new File(staging, name);
                File parent = entryTarget.getParentFile();
                if (parent != null && !parent.isDirectory() && !parent.mkdirs()) {
                    deleteTree(staging);
                    return "!无法写入恢复暂存目录";
                }
                long written;
                try (InputStream input = new BufferedInputStream(zip.getInputStream(entry));
                     OutputStream output = new BufferedOutputStream(new FileOutputStream(entryTarget))) {
                    written = copyBounded(input, output, MAX_ENTRY_BYTES);
                }
                total += written;
                if (total > MAX_TOTAL_BYTES) {
                    deleteTree(staging);
                    return "!备份包过大";
                }
                sawEntry = true;
            }
        } catch (Exception error) {
            deleteTree(staging);
            return "!" + messageOf(error);
        }
        if (!sawEntry) {
            deleteTree(staging);
            return "!备份包为空或格式不受支持";
        }
        File marker = new File(staging, PENDING_MARKER);
        try (OutputStream output = new FileOutputStream(marker)) {
            output.write(String.valueOf(System.currentTimeMillis())
                    .getBytes(StandardCharsets.US_ASCII));
        } catch (Exception error) {
            deleteTree(staging);
            return "!无法写入恢复标记";
        }
        return "restart";
    }

    /** Newest archive created by {@link #backup}, or the empty string. */
    public static String latestBackup(Context context) {
        if (context == null) {
            return "";
        }
        File directory = new File(applicationContext(context).getCacheDir(), "backup");
        File[] archives = directory.listFiles();
        if (archives == null) {
            return "";
        }
        File newest = null;
        for (File archive : archives) {
            if (!archive.isFile() || !archive.getName().startsWith(ZIP_PREFIX)
                    || !archive.getName().endsWith(ZIP_SUFFIX)) {
                continue;
            }
            if (newest == null || archive.lastModified() > newest.lastModified()) {
                newest = archive;
            }
        }
        return newest == null ? "" : newest.getAbsolutePath();
    }

    private static void addTree(ZipOutputStream zip, File canonicalRoot, File file,
                                String entryName, int depth) throws IOException {
        if (depth > MAX_TREE_DEPTH) {
            return;
        }
        if (file.isDirectory()) {
            File[] children = file.listFiles();
            if (children == null) {
                return;
            }
            Arrays.sort(children, Comparator.comparing(File::getName));
            for (File child : children) {
                addTree(zip, canonicalRoot, child, entryName + "/" + child.getName(), depth + 1);
            }
            return;
        }
        if (!file.isFile()) {
            return;
        }
        File canonical = file.getCanonicalFile();
        if (!inside(canonicalRoot, canonical)) {
            return;
        }
        addFile(zip, canonical, entryName);
    }

    private static void addFile(ZipOutputStream zip, File file, String entryName)
            throws IOException {
        long length = file.length();
        if (length > MAX_ENTRY_BYTES) {
            Log.w(TAG, "skip oversized backup entry: " + entryName);
            return;
        }
        ZipEntry entry = new ZipEntry(entryName);
        entry.setTime(file.lastModified());
        zip.putNextEntry(entry);
        try (InputStream input = new BufferedInputStream(new FileInputStream(file))) {
            byte[] buffer = new byte[64 * 1024];
            int read;
            while ((read = input.read(buffer)) != -1) {
                zip.write(buffer, 0, read);
            }
        }
        zip.closeEntry();
    }

    private static long copyBounded(InputStream input, OutputStream output, long limit)
            throws IOException {
        byte[] buffer = new byte[64 * 1024];
        long total = 0;
        int read;
        while ((read = input.read(buffer)) != -1) {
            total += read;
            if (total > limit) {
                throw new IOException("单个文件超过恢复上限");
            }
            output.write(buffer, 0, read);
        }
        return total;
    }

    /** Only config trees and the top-level databases may be restored. */
    private static boolean safeEntryName(String name) {
        if (name == null || name.isEmpty() || name.startsWith("/")
                || name.indexOf('\\') >= 0 || name.indexOf('\0') >= 0) {
            return false;
        }
        String[] segments = name.split("/");
        if (segments.length == 0) {
            return false;
        }
        for (String segment : segments) {
            if (segment.isEmpty() || segment.equals(".") || segment.equals("..")) {
                return false;
            }
        }
        if (segments[0].equals("config")) {
            return segments.length >= 2;
        }
        if (segments[0].equals("data")) {
            return segments.length == 2 && isDatabaseName(segments[1]);
        }
        return false;
    }

    private static boolean isDatabaseName(String name) {
        String lower = name.toLowerCase(Locale.ROOT);
        return lower.endsWith(".sqlite")
                || lower.endsWith(".sqlite-wal")
                || lower.endsWith(".sqlite-shm");
    }

    private static boolean inside(File root, File target) {
        String rootPath = root.getPath();
        String targetPath = target.getPath();
        return targetPath.equals(rootPath)
                || targetPath.startsWith(rootPath + File.separator);
    }

    private static void prune(File directory) {
        File[] entries = directory.listFiles();
        if (entries == null) {
            return;
        }
        ArrayList<File> archives = new ArrayList<>();
        for (File entry : entries) {
            if (entry.isFile() && entry.getName().startsWith(ZIP_PREFIX)
                    && entry.getName().endsWith(ZIP_SUFFIX)) {
                archives.add(entry);
            }
        }
        archives.sort(Comparator.comparing(File::getName));
        while (archives.size() > KEEP_BACKUPS) {
            File oldest = archives.remove(0);
            //noinspection ResultOfMethodCallIgnored
            oldest.delete();
        }
    }

    private static Context applicationContext(Context context) {
        Context app = context.getApplicationContext();
        return app == null ? context : app;
    }

    private static String messageOf(Throwable error) {
        String message = error.getMessage();
        return message == null || message.trim().isEmpty()
                ? error.getClass().getSimpleName()
                : message;
    }

    private static void closeQuietly(java.io.Closeable closeable) {
        if (closeable == null) {
            return;
        }
        try {
            closeable.close();
        } catch (Exception ignored) {
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
