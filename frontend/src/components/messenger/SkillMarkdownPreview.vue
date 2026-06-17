<template>
  <div ref="rootRef" class="messenger-skill-markdown markdown-body" v-html="renderedHtml"></div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';

import { downloadUserSkillFile } from '@/api/userTools';
import { renderMarkdown } from '@/utils/markdown';

const SKILL_ASSET_SCHEME = 'wunder-skill-asset://';

const props = defineProps<{
  content: string;
  skillName: string;
  contentPath?: string;
}>();

const rootRef = ref<HTMLElement | null>(null);
const objectUrls = new Set<string>();
let hydrateVersion = 0;

const releaseObjectUrls = () => {
  objectUrls.forEach((url) => URL.revokeObjectURL(url));
  objectUrls.clear();
};

const normalizePathSeparators = (value: string) => value.replace(/\\/g, '/');

const dirname = (path: string) => {
  const normalized = normalizePathSeparators(path).replace(/\/+$/, '');
  const index = normalized.lastIndexOf('/');
  return index >= 0 ? normalized.slice(0, index) : '';
};

const decodeMarkdownUrl = (value: string) => {
  try {
    return decodeURI(value);
  } catch {
    return value;
  }
};

const encodeSkillAssetUrl = (path: string) =>
  `${SKILL_ASSET_SCHEME}asset?path=${encodeURIComponent(path)}`;

const extractMarkdownImagePath = (rawTarget: string) => {
  const target = String(rawTarget || '').trim();
  if (!target) {
    return '';
  }
  if (target.startsWith('<') && target.includes('>')) {
    return target.slice(1, target.indexOf('>')).trim();
  }
  const titleMatch = target.match(/^(.+?)\s+(?:"[^"]*"|'[^']*'|\([^)]*\))$/);
  return String(titleMatch?.[1] || target).trim();
};

const resolveSkillAssetPath = (rawPath: string) => {
  const decoded = normalizePathSeparators(decodeMarkdownUrl(extractMarkdownImagePath(rawPath)));
  if (!decoded || decoded.startsWith('#') || decoded.startsWith('?')) {
    return '';
  }
  if (/^[a-z][a-z0-9+.-]*:/i.test(decoded) || decoded.startsWith('//') || decoded.startsWith('/')) {
    return '';
  }
  const contentDir = dirname(props.contentPath || 'SKILL.md');
  const parts = `${contentDir ? `${contentDir}/` : ''}${decoded}`
    .split('/')
    .map((part) => part.trim())
    .filter((part) => part && part !== '.');
  const safeParts: string[] = [];
  for (const part of parts) {
    if (part === '..') {
      safeParts.pop();
      continue;
    }
    safeParts.push(part);
  }
  return safeParts.join('/');
};

const rewriteSkillMarkdownImages = (content: string) =>
  String(content || '').replace(/!\[([^\]]*)\]\(([^)]*)\)/g, (match, alt, rawPath) => {
    const assetPath = resolveSkillAssetPath(rawPath);
    if (!assetPath) {
      return match;
    }
    return `![${alt}](${encodeSkillAssetUrl(assetPath)})`;
  });

const renderedHtml = computed(() => renderMarkdown(rewriteSkillMarkdownImages(props.content)));

const parseSkillAssetUrl = (value: string) => {
  const source = String(value || '').trim();
  if (!source.startsWith(SKILL_ASSET_SCHEME)) {
    return '';
  }
  try {
    const parsed = new URL(source);
    return String(parsed.searchParams.get('path') || '').trim();
  } catch {
    const marker = '?path=';
    const index = source.indexOf(marker);
    if (index < 0) {
      return '';
    }
    try {
      return decodeURIComponent(source.slice(index + marker.length));
    } catch {
      return '';
    }
  }
};

const replaceCardWithFallback = (card: HTMLElement, text: string) => {
  if (!card.isConnected) return;
  const fallback = document.createElement('span');
  fallback.className = 'ai-resource-fallback';
  fallback.textContent = text;
  card.replaceWith(fallback);
};

const hydrateSkillImages = async () => {
  const root = rootRef.value;
  const skillName = String(props.skillName || '').trim();
  if (!root || !skillName) {
    return;
  }
  const currentVersion = ++hydrateVersion;
  releaseObjectUrls();
  await nextTick();
  if (currentVersion !== hydrateVersion || rootRef.value !== root) {
    return;
  }
  const cards = Array.from(root.querySelectorAll<HTMLElement>('.ai-external-image-card'));
  await Promise.all(
    cards.map(async (card) => {
      const rawSource = String(card.dataset.externalImageSrc || '').trim();
      const assetPath = parseSkillAssetUrl(rawSource);
      if (!assetPath) {
        return;
      }
      const image = card.querySelector<HTMLImageElement>('img.ai-external-image-preview');
      if (!image) {
        return;
      }
      try {
        const response = await downloadUserSkillFile(skillName, assetPath);
        if (currentVersion !== hydrateVersion || !card.isConnected) {
          return;
        }
        const objectUrl = URL.createObjectURL(response.data);
        objectUrls.add(objectUrl);
        card.dataset.externalImageSrc = objectUrl;
        image.src = objectUrl;
      } catch {
        replaceCardWithFallback(card, String(card.dataset.markdownFallback || assetPath));
      }
    })
  );
};

watch(
  () => [props.content, props.skillName, props.contentPath],
  () => {
    void hydrateSkillImages();
  },
  { immediate: true }
);

onMounted(() => {
  void hydrateSkillImages();
});

onBeforeUnmount(() => {
  hydrateVersion += 1;
  releaseObjectUrls();
});
</script>
