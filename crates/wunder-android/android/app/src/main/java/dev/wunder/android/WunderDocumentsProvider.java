package dev.wunder.android;

import android.database.Cursor;
import android.database.MatrixCursor;
import android.os.CancellationSignal;
import android.os.ParcelFileDescriptor;
import android.provider.DocumentsContract.Document;
import android.provider.DocumentsContract.Root;
import android.provider.DocumentsProvider;
import android.webkit.MimeTypeMap;

import java.io.File;
import java.io.FileNotFoundException;
import java.util.Arrays;
import java.util.Locale;

/**
 * Read-only system document provider over the wunder workspace catalogue.
 *
 * <p>Each workspace root appears as one Storage Access Framework root, named
 * after the workspace. Documents are addressed as
 * {@code <workspaceId>:<relative/path>}; the empty relative path is the root
 * itself. Every resolved path is canonicalised and must stay inside the
 * registered root, so {@code ..}, absolute doc ids and symlinks cannot escape.
 *
 * <p>Writing is intentionally absent: the provider exists so other apps can
 * read workspace files, while the engine remains the only writer.
 */
public final class WunderDocumentsProvider extends DocumentsProvider {
    /** Authority suffix; the full authority is {@code <applicationId>.documents.workspace}. */
    public static final String AUTHORITY_SUFFIX = ".documents.workspace";

    private static final String[] DEFAULT_ROOT_PROJECTION = new String[]{
            Root.COLUMN_ROOT_ID,
            Root.COLUMN_DOCUMENT_ID,
            Root.COLUMN_TITLE,
            Root.COLUMN_SUMMARY,
            Root.COLUMN_FLAGS,
            Root.COLUMN_ICON,
            Root.COLUMN_MIME_TYPES,
            Root.COLUMN_AVAILABLE_BYTES,
    };

    private static final String[] DEFAULT_DOCUMENT_PROJECTION = new String[]{
            Document.COLUMN_DOCUMENT_ID,
            Document.COLUMN_DISPLAY_NAME,
            Document.COLUMN_MIME_TYPE,
            Document.COLUMN_SIZE,
            Document.COLUMN_LAST_MODIFIED,
            Document.COLUMN_FLAGS,
    };

    @Override
    public boolean onCreate() {
        return true;
    }

    @Override
    public Cursor queryRoots(String[] projection) {
        String[] columns = projection != null ? projection : DEFAULT_ROOT_PROJECTION;
        MatrixCursor cursor = new MatrixCursor(columns);
        int icon = 0;
        if (getContext() != null) {
            icon = getContext().getApplicationInfo().icon;
        }
        for (WorkspaceRegistry.Workspace workspace : WorkspaceRegistry.list()) {
            File root = canonicalDirectory(workspace.root);
            if (root == null) {
                continue;
            }
            Object[] values = new Object[columns.length];
            for (int index = 0; index < columns.length; index++) {
                switch (columns[index]) {
                    case Root.COLUMN_ROOT_ID:
                    case Root.COLUMN_DOCUMENT_ID:
                        values[index] = workspace.id;
                        break;
                    case Root.COLUMN_TITLE:
                        values[index] = workspace.name;
                        break;
                    case Root.COLUMN_SUMMARY:
                        values[index] = "wunder 工作区";
                        break;
                    case Root.COLUMN_FLAGS:
                        values[index] = Root.FLAG_LOCAL_ONLY | Root.FLAG_SUPPORTS_IS_CHILD;
                        break;
                    case Root.COLUMN_ICON:
                        values[index] = icon;
                        break;
                    case Root.COLUMN_MIME_TYPES:
                        values[index] = "*/*";
                        break;
                    case Root.COLUMN_AVAILABLE_BYTES:
                        values[index] = root.getUsableSpace();
                        break;
                    default:
                        values[index] = null;
                        break;
                }
            }
            cursor.addRow(values);
        }
        return cursor;
    }

    @Override
    public Cursor queryChildDocuments(String parentDocumentId, String[] projection, String sortOrder)
            throws FileNotFoundException {
        String[] columns = projection != null ? projection : DEFAULT_DOCUMENT_PROJECTION;
        MatrixCursor cursor = new MatrixCursor(columns);
        File parent = resolve(parentDocumentId);
        if (parent == null || !parent.isDirectory()) {
            throw new FileNotFoundException(parentDocumentId);
        }
        File[] children = parent.listFiles();
        if (children == null) {
            return cursor;
        }
        Arrays.sort(children, (left, right) -> {
            boolean leftDir = left.isDirectory();
            boolean rightDir = right.isDirectory();
            if (leftDir != rightDir) {
                return leftDir ? -1 : 1;
            }
            return left.getName().compareToIgnoreCase(right.getName());
        });
        String workspaceId = workspaceIdOf(parentDocumentId);
        String relative = relativeOf(parentDocumentId);
        for (File child : children) {
            String name = child.getName();
            String childId = workspaceId + ":" + (relative.isEmpty() ? name : relative + "/" + name);
            File canonical;
            try {
                canonical = child.getCanonicalFile();
            } catch (Exception ignored) {
                continue;
            }
            if (!isInside(parent, canonical)) {
                continue;
            }
            cursor.addRow(documentRow(columns, childId, canonical, name));
        }
        return cursor;
    }

    @Override
    public Cursor queryDocument(String documentId, String[] projection)
            throws FileNotFoundException {
        String[] columns = projection != null ? projection : DEFAULT_DOCUMENT_PROJECTION;
        MatrixCursor cursor = new MatrixCursor(columns);
        File file = resolve(documentId);
        if (file == null || !file.exists()) {
            throw new FileNotFoundException(documentId);
        }
        String displayName;
        if (relativeOf(documentId).isEmpty()) {
            WorkspaceRegistry.Workspace workspace = WorkspaceRegistry.find(workspaceIdOf(documentId));
            displayName = workspace == null ? file.getName() : workspace.name;
        } else {
            displayName = file.getName();
        }
        cursor.addRow(documentRow(columns, documentId, file, displayName));
        return cursor;
    }

    @Override
    public ParcelFileDescriptor openDocument(String documentId, String mode,
                                             CancellationSignal signal)
            throws FileNotFoundException {
        if (!"r".equals(mode) && !"rt".equals(mode)) {
            throw new FileNotFoundException("wunder 工作区只读");
        }
        File file = resolve(documentId);
        if (file == null || !file.isFile() || !file.canRead()) {
            throw new FileNotFoundException(documentId);
        }
        try {
            return ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_READ_ONLY);
        } catch (Exception error) {
            throw new FileNotFoundException(documentId + ": " + error.getMessage());
        }
    }

    @Override
    public boolean isChildDocument(String parentDocumentId, String childDocumentId) {
        File parent = resolve(parentDocumentId);
        File child = resolve(childDocumentId);
        return parent != null && child != null && isInside(parent, child);
    }

    private Object[] documentRow(String[] columns, String documentId, File file, String displayName) {
        Object[] values = new Object[columns.length];
        boolean directory = file.isDirectory();
        for (int index = 0; index < columns.length; index++) {
            switch (columns[index]) {
                case Document.COLUMN_DOCUMENT_ID:
                    values[index] = documentId;
                    break;
                case Document.COLUMN_DISPLAY_NAME:
                    values[index] = displayName;
                    break;
                case Document.COLUMN_MIME_TYPE:
                    values[index] = directory ? Document.MIME_TYPE_DIR : mimeOf(file);
                    break;
                case Document.COLUMN_SIZE:
                    values[index] = directory ? 0L : file.length();
                    break;
                case Document.COLUMN_LAST_MODIFIED:
                    values[index] = file.lastModified();
                    break;
                case Document.COLUMN_FLAGS:
                    values[index] = 0;
                    break;
                default:
                    values[index] = null;
                    break;
            }
        }
        return values;
    }

    /** Resolve one doc id to a canonical file inside its registered root. */
    private File resolve(String documentId) {
        if (documentId == null || documentId.isEmpty()) {
            return null;
        }
        String workspaceId = workspaceIdOf(documentId);
        String relative = relativeOf(documentId);
        WorkspaceRegistry.Workspace workspace = WorkspaceRegistry.find(workspaceId);
        if (workspace == null) {
            return null;
        }
        File root = canonicalDirectory(workspace.root);
        if (root == null) {
            return null;
        }
        if (relative.isEmpty()) {
            return root;
        }
        // Reject traversal spellings outright; the canonical prefix check below
        // is the authoritative guard and also catches symlink escapes.
        if (relative.startsWith("/") || relative.indexOf('\0') >= 0 || relative.indexOf('\\') >= 0) {
            return null;
        }
        for (String segment : relative.split("/")) {
            if (segment.isEmpty() || segment.equals(".") || segment.equals("..")) {
                return null;
            }
        }
        try {
            File target = new File(root, relative).getCanonicalFile();
            return isInside(root, target) ? target : null;
        } catch (Exception ignored) {
            return null;
        }
    }

    private static String workspaceIdOf(String documentId) {
        int separator = documentId.indexOf(':');
        return separator < 0 ? documentId : documentId.substring(0, separator);
    }

    private static String relativeOf(String documentId) {
        int separator = documentId.indexOf(':');
        return separator < 0 ? "" : documentId.substring(separator + 1);
    }

    private static boolean isInside(File root, File target) {
        String rootPath = root.getPath();
        String targetPath = target.getPath();
        if (targetPath.equals(rootPath)) {
            return true;
        }
        return targetPath.startsWith(rootPath + File.separator);
    }

    private static File canonicalDirectory(String path) {
        if (path == null || path.trim().isEmpty()) {
            return null;
        }
        try {
            File directory = new File(path.trim()).getCanonicalFile();
            return directory.isDirectory() ? directory : null;
        } catch (Exception ignored) {
            return null;
        }
    }

    private static String mimeOf(File file) {
        if (file.isDirectory()) {
            return Document.MIME_TYPE_DIR;
        }
        String name = file.getName();
        int dot = name.lastIndexOf('.');
        String extension = dot >= 0 && dot + 1 < name.length()
                ? name.substring(dot + 1).toLowerCase(Locale.ROOT)
                : "";
        if (!extension.isEmpty()) {
            String known = MimeTypeMap.getSingleton().getMimeTypeFromExtension(extension);
            if (known != null) {
                return known;
            }
            switch (extension) {
                case "md":
                case "markdown":
                    return "text/markdown";
                case "yaml":
                case "yml":
                    return "text/yaml";
                case "log":
                    return "text/plain";
                default:
                    break;
            }
        }
        return "application/octet-stream";
    }

}
