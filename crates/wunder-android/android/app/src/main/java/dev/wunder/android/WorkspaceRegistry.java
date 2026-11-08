package dev.wunder.android;

import org.json.JSONArray;
import org.json.JSONObject;

import java.util.ArrayList;
import java.util.Collections;
import java.util.List;

/**
 * In-process snapshot of the workspace catalogue for the system
 * {@link WunderDocumentsProvider}.
 *
 * <p>The engine owns the durable catalogue and pushes id/name/root after every
 * refresh. The provider runs in the same process but on Binder threads and may
 * be queried before the first push, so readers always see an immutable list
 * and an empty catalogue is a valid (empty) answer.
 */
public final class WorkspaceRegistry {
    /** One read-only workspace root. */
    public static final class Workspace {
        public final String id;
        public final String name;
        public final String root;

        Workspace(String id, String name, String root) {
            this.id = id;
            this.name = name;
            this.root = root;
        }
    }

    private static final int MAX_WORKSPACES = 64;
    private static final Object lock = new Object();
    private static volatile List<Workspace> workspaces = Collections.emptyList();
    private static volatile String snapshot = "{\"workspaces\":[]}";

    private WorkspaceRegistry() {}

    /**
     * Replace the catalogue. {@code json} is either a JSON array of
     * {@code {"id","name","root"}} objects or an object with a
     * {@code workspaces} array. Invalid records are skipped instead of
     * breaking the provider.
     */
    public static void update(String json) {
        if (json == null) {
            return;
        }
        JSONArray array = null;
        try {
            String trimmed = json.trim();
            if (trimmed.startsWith("{")) {
                array = new JSONObject(trimmed).optJSONArray("workspaces");
            } else {
                array = new JSONArray(trimmed);
            }
        } catch (Exception ignored) {
            return;
        }
        if (array == null) {
            return;
        }
        ArrayList<Workspace> parsed = new ArrayList<>();
        for (int index = 0; index < array.length() && parsed.size() < MAX_WORKSPACES; index++) {
            JSONObject record = array.optJSONObject(index);
            if (record == null) {
                continue;
            }
            String id = record.optString("id", "").trim();
            String name = record.optString("name", "").trim();
            String root = record.optString("root", "").trim();
            if (id.isEmpty() || root.isEmpty()) {
                continue;
            }
            parsed.add(new Workspace(id, name.isEmpty() ? id : name, root));
        }
        JSONArray canonical = new JSONArray();
        for (Workspace workspace : parsed) {
            try {
                JSONObject record = new JSONObject();
                record.put("id", workspace.id);
                record.put("name", workspace.name);
                record.put("root", workspace.root);
                canonical.put(record);
            } catch (Exception ignored) {
            }
        }
        JSONObject payload = new JSONObject();
        try {
            payload.put("workspaces", canonical);
        } catch (Exception ignored) {
        }
        synchronized (lock) {
            workspaces = Collections.unmodifiableList(parsed);
            snapshot = payload.toString();
        }
    }

    /** Latest catalogue as canonical JSON; the provider and diagnostics both read it. */
    public static String snapshot() {
        return snapshot;
    }

    /** Immutable snapshot; safe to iterate without holding the lock. */
    public static List<Workspace> list() {
        return workspaces;
    }

    /** Look up one workspace by the id embedded in a document id. */
    public static Workspace find(String id) {
        if (id == null || id.isEmpty()) {
            return null;
        }
        for (Workspace workspace : workspaces) {
            if (workspace.id.equals(id)) {
                return workspace;
            }
        }
        return null;
    }
}
