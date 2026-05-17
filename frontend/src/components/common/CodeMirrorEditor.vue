<template>
  <div ref="hostRef" class="code-mirror-editor-host"></div>
</template>

<script setup lang="ts">
import { Compartment, EditorState } from '@codemirror/state';
import { EditorView, keymap, lineNumbers, highlightActiveLine, drawSelection, placeholder } from '@codemirror/view';
import { defaultKeymap, history, historyKeymap } from '@codemirror/commands';
import { searchKeymap, highlightSelectionMatches } from '@codemirror/search';
import { autocompletion, completionKeymap } from '@codemirror/autocomplete';
import { bracketMatching, syntaxHighlighting, defaultHighlightStyle, indentOnInput } from '@codemirror/language';
import { onBeforeUnmount, onMounted, ref, watch } from 'vue';

import { resolveCodeMirrorLanguageExtension } from '@/utils/codeMirrorLanguage';

const props = defineProps<{
  modelValue: string;
  sourcePath?: string;
  readonly?: boolean;
  placeholder?: string;
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

onMounted(() => {
  if (!hostRef.value) return;
  const extensions = [
    lineNumbers(),
    history(),
    drawSelection(),
    highlightActiveLine(),
    bracketMatching(),
    indentOnInput(),
    syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
    highlightSelectionMatches(),
    autocompletion(),
    EditorView.lineWrapping,
    EditorView.theme({
      '&': {
        height: '100%',
        minHeight: '320px',
        fontSize: '13px',
        backgroundColor: 'transparent',
        color: 'var(--chat-text)'
      },
      '.cm-scroller': {
        overflow: 'auto',
        fontFamily: '"JetBrains Mono", "Fira Code", "SFMono-Regular", Consolas, "Liberation Mono", Menlo, monospace',
        lineHeight: '1.65'
      },
      '.cm-content': {
        padding: '12px 0 20px'
      },
      '.cm-line': {
        padding: '0 12px'
      },
      '.cm-gutters': {
        backgroundColor: 'transparent',
        borderRight: '1px solid rgba(77, 216, 255, 0.12)',
        color: 'var(--chat-muted)'
      },
      '.cm-activeLine': {
        backgroundColor: 'rgba(77, 216, 255, 0.06)'
      },
      '.cm-activeLineGutter': {
        backgroundColor: 'rgba(77, 216, 255, 0.08)'
      },
      '.cm-selectionBackground': {
        backgroundColor: 'rgba(77, 216, 255, 0.22) !important'
      },
      '.cm-cursor': {
        borderLeftColor: 'var(--chat-primary)'
      },
      '.cm-placeholder': {
        color: 'var(--chat-muted)'
      },
      '&.cm-focused': {
        outline: 'none'
      }
    }),
    keymap.of([...defaultKeymap, ...historyKeymap, ...searchKeymap, ...completionKeymap]),
    languageCompartment.of(resolveCodeMirrorLanguageExtension(props.sourcePath || '')),
    editableCompartment.of(buildEditableExtensions(Boolean(props.readonly))),
    placeholderCompartment.of(buildPlaceholderExtension(props.placeholder || '')),
    EditorView.updateListener.of((update) => {
      if (!update.docChanged || syncingFromProps) return;
      emit('update:modelValue', update.state.doc.toString());
    })
  ];

  const state = EditorState.create({
    doc: props.modelValue || '',
    extensions
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
}

.code-mirror-editor-host :deep(.cm-editor) {
  height: 100%;
  min-height: 320px;
}

.code-mirror-editor-host :deep(.cm-scroller) {
  min-height: 320px;
}

.code-mirror-editor-host :deep(.cm-placeholder) {
  color: var(--chat-muted);
}
</style>
