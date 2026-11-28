<template>
  <div ref="hostRef" class="code-mirror-editor-host"></div>
</template>

<script setup lang="ts">
import { Compartment, EditorState } from '@codemirror/state';
import { EditorView, keymap, lineNumbers, highlightActiveLine, drawSelection, placeholder } from '@codemirror/view';
import { defaultKeymap, history, historyKeymap } from '@codemirror/commands';
import { searchKeymap, highlightSelectionMatches } from '@codemirror/search';
import { autocompletion, completionKeymap } from '@codemirror/autocomplete';
import {
  HighlightStyle,
  bracketMatching,
  defaultHighlightStyle,
  indentOnInput,
  syntaxHighlighting
} from '@codemirror/language';
import { tags } from '@lezer/highlight';
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';

import { useThemeStore } from '@/stores/theme';
import { resolveCodeMirrorLanguageExtension } from '@/utils/codeMirrorLanguage';

const props = defineProps<{
  modelValue: string;
  sourcePath?: string;
  readonly?: boolean;
  placeholder?: string;
  lightSurface?: boolean;
}>();

const emit = defineEmits<{
  (event: 'update:modelValue', value: string): void;
}>();

const hostRef = ref<HTMLElement | null>(null);
let editorView: EditorView | null = null;
let syncingFromProps = false;

const languageCompartment = new Compartment();
const editableCompartment = new Compartment();
const placeholderCompartment = new Compartment();

const themeStore = useThemeStore();
const isDarkPalette = computed(
  () => themeStore.palette === 'deep-sea' || themeStore.palette === 'tech-blue'
);
// `light-surface` is the caller's intent, but the deep palette must not be
// forced back to a white editor: the surrounding shell already paints the
// surface dark, and white text would otherwise sit on a dark background.
const lightSurfaceActive = computed(() => Boolean(props.lightSurface) && !isDarkPalette.value);

const deepSeaHighlightStyle = HighlightStyle.define([
  {
    tag: [
      tags.keyword,
      tags.controlKeyword,
      tags.definitionKeyword,
      tags.moduleKeyword,
      tags.operatorKeyword
    ],
    color: '#7dd3fc'
  },
  {
    tag: [tags.string, tags.docString, tags.attributeValue],
    color: '#86efac'
  },
  {
    tag: [tags.number, tags.bool, tags.null],
    color: '#facc15'
  },
  {
    tag: [tags.comment, tags.lineComment, tags.blockComment, tags.docComment],
    color: '#8fa1b2',
    fontStyle: 'italic'
  },
  {
    tag: [tags.function(tags.variableName), tags.definition(tags.variableName)],
    color: '#8fcdea'
  },
  {
    tag: [tags.typeName, tags.className, tags.namespace],
    color: '#7dd3fc'
  },
  {
    tag: [tags.propertyName, tags.attributeName, tags.tagName],
    color: '#e7b96a'
  },
  {
    tag: [tags.operator, tags.punctuation],
    color: '#c9d6e1'
  },
  {
    tag: [tags.heading, tags.heading1, tags.heading2, tags.heading3, tags.heading4, tags.heading5, tags.heading6],
    color: '#8fcdea',
    fontWeight: '600'
  },
  {
    tag: [tags.link],
    color: '#86efac',
    textDecoration: 'underline'
  },
  {
    tag: [tags.invalid],
    color: '#f28c86'
  }
]);

const buildThemeExtension = () => {
  const light = lightSurfaceActive.value;
  return EditorView.theme({
    '&': {
      height: '100%',
      minHeight: '320px',
      fontSize: '13px',
      backgroundColor: light ? '#ffffff' : 'var(--mz-surface, #121a22)',
      color: light ? '#1f2937' : 'var(--mz-text, #e7edf2)'
    },
    '.cm-scroller': {
      overflow: 'auto',
      fontFamily: '"JetBrains Mono", "Fira Code", "SFMono-Regular", Consolas, "Liberation Mono", Menlo, monospace',
      lineHeight: '1.65',
      color: light ? '#1f2937' : 'var(--mz-text, #e7edf2)'
    },
    '.cm-content': {
      padding: '12px 0 20px'
    },
    '.cm-line': {
      padding: '0 12px'
    },
    '.cm-gutters': {
      backgroundColor: light ? '#f8fafc' : 'var(--mz-surface-soft, #17222c)',
      borderRight: light ? '1px solid rgba(148, 163, 184, 0.28)' : '1px solid var(--mz-border, #293641)',
      color: light ? '#94a3b8' : 'var(--mz-text-muted, #8fa1b2)'
    },
    '.cm-activeLine': {
      backgroundColor: light ? 'rgba(59, 130, 246, 0.06)' : 'rgba(95, 168, 207, 0.08)'
    },
    '.cm-activeLineGutter': {
      backgroundColor: light ? 'rgba(59, 130, 246, 0.08)' : 'rgba(95, 168, 207, 0.1)'
    },
    '.cm-selectionBackground': {
      backgroundColor: light ? 'rgba(59, 130, 246, 0.18) !important' : 'rgba(95, 168, 207, 0.2) !important'
    },
    '.cm-cursor': {
      borderLeftColor: light ? '#2563eb' : 'var(--ui-accent-ink, #8fcdea)'
    },
    '.cm-placeholder': {
      color: light ? '#94a3b8' : 'var(--mz-text-muted, #8fa1b2)'
    },
    '.cm-lineNumbers': {
      color: light ? '#94a3b8' : 'var(--mz-text-muted, #8fa1b2)'
    },
    '.cm-content, .cm-line': {
      color: light ? '#1f2937' : 'var(--mz-text, #e7edf2)'
    },
    '&.cm-focused': {
      outline: 'none'
    }
  });
};

const buildBaseExtensions = () => [
  lineNumbers(),
  history(),
  highlightActiveLine(),
  drawSelection(),
  bracketMatching(),
  indentOnInput(),
  syntaxHighlighting(lightSurfaceActive.value ? defaultHighlightStyle : deepSeaHighlightStyle, {
    fallback: true
  }),
  highlightSelectionMatches(),
  autocompletion(),
  EditorView.lineWrapping,
  buildThemeExtension(),
  keymap.of([...defaultKeymap, ...historyKeymap, ...searchKeymap, ...completionKeymap]),
  languageCompartment.of(resolveCodeMirrorLanguageExtension(props.sourcePath || '')),
  editableCompartment.of(buildEditableExtensions(Boolean(props.readonly))),
  placeholderCompartment.of(buildPlaceholderExtension(props.placeholder || '')),
  EditorView.updateListener.of((update) => {
    if (!update.docChanged || syncingFromProps) return;
    emit('update:modelValue', update.state.doc.toString());
  })
];

onMounted(() => {
  if (!hostRef.value) return;
  const state = EditorState.create({
    doc: props.modelValue || '',
    extensions: buildBaseExtensions()
  });

  editorView = new EditorView({
    state,
    parent: hostRef.value
  });
});

watch(
  () => props.modelValue,
  (nextValue) => {
    if (!editorView) return;
    const current = editorView.state.doc.toString();
    const normalized = String(nextValue || '');
    if (normalized === current) return;
    syncingFromProps = true;
    editorView.dispatch({
      changes: { from: 0, to: current.length, insert: normalized }
    });
    syncingFromProps = false;
  }
);

watch(
  () => props.sourcePath,
  (nextValue) => {
    if (!editorView) return;
    editorView.dispatch({
      effects: languageCompartment.reconfigure(resolveCodeMirrorLanguageExtension(nextValue || ''))
    });
  }
);

watch(
  () => props.readonly,
  (nextValue) => {
    if (!editorView) return;
    editorView.dispatch({
      effects: editableCompartment.reconfigure(buildEditableExtensions(Boolean(nextValue)))
    });
  }
);

watch(
  () => props.placeholder,
  (nextValue) => {
    if (!editorView) return;
    editorView.dispatch({
      effects: placeholderCompartment.reconfigure(buildPlaceholderExtension(nextValue || ''))
    });
  }
);

watch(lightSurfaceActive, () => {
  if (!editorView) return;
  const current = editorView.state.doc.toString();
  editorView.setState(
    EditorState.create({
      doc: current,
      extensions: buildBaseExtensions()
    })
  );
});

onBeforeUnmount(() => {
  editorView?.destroy();
  editorView = null;
});

const buildEditableExtensions = (readonly: boolean) => [
  EditorState.readOnly.of(readonly),
  EditorView.editable.of(!readonly)
];

const buildPlaceholderExtension = (value: string) => (value ? placeholder(value) : []);
</script>

<style scoped>
.code-mirror-editor-host {
  min-height: 320px;
  height: 100%;
  width: 100%;
  overflow: hidden;
  display: flex;
  flex: 1 1 auto;
  min-width: 0;
  min-height: 0;
}

.code-mirror-editor-host :deep(.cm-editor) {
  width: 100%;
  height: 100%;
  min-height: 320px;
  overflow: hidden;
  display: flex;
  flex-direction: column;
  flex: 1 1 auto;
  min-width: 0;
  min-height: 0;
}

.code-mirror-editor-host :deep(.cm-scroller) {
  width: 100%;
  height: 100%;
  min-height: 320px;
  overflow: auto;
  flex: 1 1 auto;
  min-width: 0;
  min-height: 0;
}

.code-mirror-editor-host :deep(.cm-placeholder) {
  color: var(--chat-muted);
}
</style>
