package dev.wunder.android;

import android.content.ContentProvider;
import android.content.ContentValues;
import android.database.Cursor;
import android.database.MatrixCursor;
import android.net.Uri;
import android.os.ParcelFileDescriptor;
import android.provider.OpenableColumns;

import java.io.File;
import java.io.FileNotFoundException;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.UUID;

/**
 * One-shot read-only provider for transcript files that live outside every
 * workspace: chat attachments, temp artifacts and local cards elsewhere on the
 * device.
 *
 * <p>{@link WunderDocumentsProvider} can only mint URIs for registered
 * workspace roots. A rendered picture or document outside them still has to
 * reach the viewer the user picked, so the UI registers that one file here
 * right before launching {@code ACTION_VIEW} and receives an unguessable token.
 * The provider is not exported — other apps only ever see the single URI they
 * were granted — and the registry is bounded and dies with the process.
 */
public final class WunderOpenProvider extends ContentProvider {
    /** Authority suffix; the full authority is {@code <applicationId>.open.file}. */
    public static final String AUTHORITY_SUFFIX = ".open.file";

    /** A burst of taps must not grow the registry without bound. */
    private static final int MAX_FILES = 16;

    private static final Map<String, GrantedFile> entries = new LinkedHashMap<String, GrantedFile>() {
        @Override
        protected boolean removeEldestEntry(Map.Entry<String, GrantedFile> eldest) {
            return size() > MAX_FILES;
        }
    };

    private static final class GrantedFile {
        final File file;
        final String mime;

        GrantedFile(File file, String mime) {
            this.file = file;
            this.mime = mime;
        }
    }

    /**
     * Register one host file for a single view. Returns the token to embed in
     * the content URI, or {@code null} when the path is not a readable file.
     */
    public static String register(String path) {
        if (path == null || path.trim().isEmpty()) {
            return null;
        }
        File file;
        try {
            file = new File(path.trim()).getCanonicalFile();
        } catch (Exception ignored) {
            return null;
        }
        if (!file.isFile() || !file.canRead()) {
            return null;
        }
        String token = UUID.randomUUID().toString().replace("-", "");
        String mime = WunderDocumentsProvider.mimeTypeOfName(file.getName());
        synchronized (entries) {
            entries.put(token, new GrantedFile(file, mime));
        }
        return token;
    }

    @Override
    public boolean onCreate() {
        return true;
    }

    @Override
    public Cursor query(Uri uri, String[] projection, String selection,
            String[] selectionArgs, String sortOrder) {
        GrantedFile entry = entryOf(uri);
        if (entry == null) {
            return null;
        }
        String[] columns = projection != null
                ? projection
                : new String[]{OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE};
        MatrixCursor cursor = new MatrixCursor(columns, 1);
        Object[] row = new Object[columns.length];
        for (int index = 0; index < columns.length; index++) {
            switch (columns[index]) {
                case OpenableColumns.DISPLAY_NAME:
                    row[index] = entry.file.getName();
                    break;
                case OpenableColumns.SIZE:
                    row[index] = entry.file.length();
                    break;
                default:
                    row[index] = null;
                    break;
            }
        }
        cursor.addRow(row);
        return cursor;
    }

    @Override
    public String getType(Uri uri) {
        GrantedFile entry = entryOf(uri);
        return entry == null ? null : entry.mime;
    }

    @Override
    public ParcelFileDescriptor openFile(Uri uri, String mode) throws FileNotFoundException {
        if (!"r".equals(mode) && !"rt".equals(mode)) {
            throw new FileNotFoundException("wunder 文件只读");
        }
        GrantedFile entry = entryOf(uri);
        if (entry == null || !entry.file.isFile() || !entry.file.canRead()) {
            throw new FileNotFoundException(String.valueOf(uri));
        }
        return ParcelFileDescriptor.open(entry.file, ParcelFileDescriptor.MODE_READ_ONLY);
    }

    @Override
    public Uri insert(Uri uri, ContentValues values) {
        return null;
    }

    @Override
    public int delete(Uri uri, String selection, String[] selectionArgs) {
        return 0;
    }

    @Override
    public int update(Uri uri, ContentValues values, String selection, String[] selectionArgs) {
        return 0;
    }

    /** Token lookup only: the URI never carries a path, so traversal is moot. */
    private static GrantedFile entryOf(Uri uri) {
        if (uri == null) {
            return null;
        }
        String token = uri.getLastPathSegment();
        if (token == null || token.isEmpty()) {
            return null;
        }
        synchronized (entries) {
            return entries.get(token);
        }
    }
}
