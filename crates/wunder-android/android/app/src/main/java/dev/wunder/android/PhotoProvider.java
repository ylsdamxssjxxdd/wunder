package dev.wunder.android;

import android.content.ContentProvider;
import android.content.ContentValues;
import android.content.Context;
import android.database.Cursor;
import android.database.MatrixCursor;
import android.net.Uri;
import android.os.ParcelFileDescriptor;
import android.provider.OpenableColumns;

import java.io.File;
import java.io.FileNotFoundException;

/**
 * Writes the camera's output into the app's own cache directory.
 *
 * <p>The system camera app can only write where it is granted access, so it
 * needs a {@code content://} target. Serving one ourselves (instead of pulling
 * in androidx's FileProvider) keeps the APK's dependency set unchanged and lets
 * the shot stay private: it never enters the gallery or shared storage, and no
 * storage permission is involved. {@link UiBridge} then hands the staged file
 * to the engine, which moves it into the workspace.
 *
 * <p>Only the {@code <pkg>.camera} authority's {@code photo/<name>} paths are
 * served, and the name is resolved inside {@code cacheDir/wunder-camera}, so a
 * crafted URI cannot escape that directory.
 */
public final class PhotoProvider extends ContentProvider {
    static final String AUTHORITY_SUFFIX = ".camera";
    private static final String DIRECTORY = "wunder-camera";

    /** The URI the camera app may write this file through. */
    static Uri uriFor(Context context, File file) {
        return new Uri.Builder()
                .scheme("content")
                .authority(context.getPackageName() + AUTHORITY_SUFFIX)
                .appendPath("photo")
                .appendPath(file.getName())
                .build();
    }

    @Override
    public boolean onCreate() {
        return true;
    }

    @Override
    public ParcelFileDescriptor openFile(Uri uri, String mode) throws FileNotFoundException {
        File file = resolve(uri);
        if (file == null) {
            throw new FileNotFoundException(String.valueOf(uri));
        }
        File parent = file.getParentFile();
        if (parent != null && !parent.isDirectory()) {
            //noinspection ResultOfMethodCallIgnored
            parent.mkdirs();
        }
        int flags;
        if ("r".equals(mode)) {
            flags = ParcelFileDescriptor.MODE_READ_ONLY;
        } else {
            flags = ParcelFileDescriptor.MODE_CREATE | ParcelFileDescriptor.MODE_WRITE_ONLY;
            if (mode != null && mode.contains("t")) {
                flags |= ParcelFileDescriptor.MODE_TRUNCATE;
            }
        }
        return ParcelFileDescriptor.open(file, flags);
    }

    @Override
    public String getType(Uri uri) {
        return "image/jpeg";
    }

    /** Camera apps ask for the display name / size before writing. */
    @Override
    public Cursor query(Uri uri, String[] projection, String selection, String[] selectionArgs,
            String sortOrder) {
        File file = resolve(uri);
        if (file == null) {
            return null;
        }
        String[] columns = projection != null ? projection
                : new String[]{OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE};
        MatrixCursor cursor = new MatrixCursor(columns);
        Object[] row = new Object[columns.length];
        for (int index = 0; index < columns.length; index++) {
            if (OpenableColumns.DISPLAY_NAME.equals(columns[index])) {
                row[index] = file.getName();
            } else if (OpenableColumns.SIZE.equals(columns[index])) {
                row[index] = file.length();
            } else {
                row[index] = null;
            }
        }
        cursor.addRow(row);
        return cursor;
    }

    @Override
    public Uri insert(Uri uri, ContentValues values) {
        throw new UnsupportedOperationException("read-only photo provider");
    }

    @Override
    public int delete(Uri uri, String selection, String[] selectionArgs) {
        throw new UnsupportedOperationException("read-only photo provider");
    }

    @Override
    public int update(Uri uri, ContentValues values, String selection, String[] selectionArgs) {
        throw new UnsupportedOperationException("read-only photo provider");
    }

    /**
     * Map {@code content://<pkg>.camera/photo/<name>} to a cache file. Anything
     * else (other authority, extra path segments, a name that is not a plain
     * file name) resolves to null.
     */
    private File resolve(Uri uri) {
        Context context = getContext();
        if (context == null || uri == null) {
            return null;
        }
        if (!(context.getPackageName() + AUTHORITY_SUFFIX).equals(uri.getAuthority())) {
            return null;
        }
        if (uri.getPathSegments().size() != 2 || !"photo".equals(uri.getPathSegments().get(0))) {
            return null;
        }
        String name = uri.getPathSegments().get(1);
        if (name == null || name.isEmpty() || name.contains("/") || name.contains("\\")
                || name.equals(".") || name.equals("..")) {
            return null;
        }
        return new File(new File(context.getCacheDir(), DIRECTORY), name);
    }
}
