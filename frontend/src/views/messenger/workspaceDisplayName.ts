import { ref } from 'vue';

// The cloud workspace is a single locked root per user, so only its display name
// is user-editable and it is a presentation-only preference (§5.2).
export const WORKSPACE_DISPLAY_NAME_STORAGE_KEY = 'messenger:workspace:displayName';

const readStoredName = (): string => {
  try {
    return String(localStorage.getItem(WORKSPACE_DISPLAY_NAME_STORAGE_KEY) || '').trim();
  } catch {
    return '';
  }
};

export const workspaceDisplayNameOverride = ref(readStoredName());

export const setWorkspaceDisplayNameOverride = (value: string): void => {
  const next = String(value || '').trim();
  try {
    if (next) {
      localStorage.setItem(WORKSPACE_DISPLAY_NAME_STORAGE_KEY, next);
    } else {
      localStorage.removeItem(WORKSPACE_DISPLAY_NAME_STORAGE_KEY);
    }
  } catch {
    // Display-only preference; a storage failure must not block the rename.
  }
  workspaceDisplayNameOverride.value = next;
};
