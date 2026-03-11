import { computed, ref, type ComputedRef, type Ref } from 'vue';

import type { MessengerSection } from '@/stores/sessionHub';

type TranslateFn = (key: string) => string;

type PreviewOptions = {
  helperWorkspace?: boolean;
};

type UseMiddlePaneOverlayPreviewOptions = {
  activeSection: ComputedRef<MessengerSection>;
  helperAppsWorkspaceMode: Ref<boolean>;
  isMiddlePaneOverlay: ComputedRef<boolean>;
  middlePaneOverlayVisible: Ref<boolean>;
  t: TranslateFn;
};

export const useMiddlePaneOverlayPreview = ({
  activeSection,
  helperAppsWorkspaceMode,
  isMiddlePaneOverlay,
  middlePaneOverlayVisible,
  t
}: UseMiddlePaneOverlayPreviewOptions) => {
  const previewSection = ref<MessengerSection | ''>('');
  const previewHelperWorkspace = ref(false);

  const isPreviewing = computed(
    () => isMiddlePaneOverlay.value && middlePaneOverlayVisible.value && Boolean(previewSection.value)
  );

  const effectiveSection = computed<MessengerSection>(() => {
    if (isPreviewing.value && previewSection.value) {
      return previewSection.value as MessengerSection;
    }
    return activeSection.value;
  });

  const effectiveHelperAppsWorkspace = computed(() => {
    if (effectiveSection.value !== 'groups') {
      return false;
    }
    return isPreviewing.value ? previewHelperWorkspace.value : helperAppsWorkspaceMode.value;
  });

  const effectiveSectionTitle = computed(() => {
    if (effectiveHelperAppsWorkspace.value) {
      return t('userWorld.helperApps.title');
    }
    return effectiveSection.value === 'more'
      ? t('messenger.section.settings')
      : t(`messenger.section.${effectiveSection.value}`);
  });

  const effectiveSectionSubtitle = computed(() => {
    if (effectiveHelperAppsWorkspace.value) {
      return t('userWorld.helperApps.subtitle');
    }
    return effectiveSection.value === 'more'
      ? t('messenger.section.settings.desc')
      : t(`messenger.section.${effectiveSection.value}.desc`);
  });

  const effectiveSearchPlaceholder = computed(() => t(`messenger.search.${effectiveSection.value}`));

  const previewMiddlePaneSection = (
    section: MessengerSection,
    options: PreviewOptions = {}
  ) => {
    if (!isMiddlePaneOverlay.value) {
      return;
    }
    // The overlay preview must stay isolated from the actual active section
    // so hovering does not mutate the main content before the user clicks.
    previewSection.value = section;
    previewHelperWorkspace.value = section === 'groups' && options.helperWorkspace === true;
    middlePaneOverlayVisible.value = true;
  };

  const clearMiddlePaneOverlayPreview = () => {
    previewSection.value = '';
    previewHelperWorkspace.value = false;
  };

  const isSectionButtonActive = (section: MessengerSection): boolean => {
    if (effectiveHelperAppsWorkspace.value && effectiveSection.value === 'groups') {
      return false;
    }
    return effectiveSection.value === section;
  };

  const isHelperWorkspaceButtonActive = computed(
    () => effectiveSection.value === 'groups' && effectiveHelperAppsWorkspace.value
  );

  return {
    clearMiddlePaneOverlayPreview,
    effectiveHelperAppsWorkspace,
    effectiveSearchPlaceholder,
    effectiveSection,
    effectiveSectionSubtitle,
    effectiveSectionTitle,
    isHelperWorkspaceButtonActive,
    isPreviewing,
    isSectionButtonActive,
    previewMiddlePaneSection
  };
};
