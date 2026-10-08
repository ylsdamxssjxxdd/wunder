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
        backgroundColor: props.lightSurface ? '#ffffff' : 'transparent',
        color: props.lightSurface ? '#1f2937' : 'var(--chat-text)'
      },
      '.cm-scroller': {
        overflow: 'auto',
        fontFamily: '"JetBrains Mono", "Fira Code", "SFMono-Regular", Consolas, "Liberation Mono", Menlo, monospace',
        lineHeight: '1.65',
        color: props.lightSurface ? '#1f2937' : 'var(--chat-text)'
      },
      '.cm-content': {
        padding: '12px 0 20px'
      },
      '.cm-line': {
        padding: '0 12px'
      },
      '.cm-gutters': {
        backgroundColor: props.lightSurface ? '#f8fafc' : 'transparent',
        borderRight: props.lightSurface ? '1px solid rgba(148, 163, 184, 0.28)' : '1px solid rgba(77, 216, 255, 0.12)',
        color: props.lightSurface ? '#94a3b8' : 'var(--chat-muted)'
      },
      '.cm-activeLine': {
        backgroundColor: props.lightSurface ? 'rgba(59, 130, 246, 0.06)' : 'rgba(77, 216, 255, 0.06)'
      },
      '.cm-activeLineGutter': {
        backgroundColor: props.lightSurface ? 'rgba(59, 130, 246, 0.08)' : 'rgba(77, 216, 255, 0.08)'
      },
      '.cm-selectionBackground': {
        backgroundColor: props.lightSurface ? 'rgba(59, 130, 246, 0.18) !important' : 'rgba(77, 216, 255, 0.22) !important'
      },
      '.cm-cursor': {
        borderLeftColor: props.lightSurface ? '#2563eb' : 'var(--chat-primary)'
      },
      '.cm-placeholder': {
        color: props.lightSurface ? '#94a3b8' : 'var(--chat-muted)'
      },
      '.cm-lineNumbers': {
        color: props.lightSurface ? '#94a3b8' : 'var(--chat-muted)'
      },
      '.cm-content, .cm-line': {
        color: props.lightSurface ? '#1f2937' : 'var(--chat-text)'
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

watch(
  () => props.lightSurface,
  () => {
    if (!editorView) return;
    const current = editorView.state.doc.toString();
    const state = EditorState.create({
      doc: current,
      extensions: [
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
            backgroundColor: props.lightSurface ? '#ffffff' : 'transparent',
            color: props.lightSurface ? '#1f2937' : 'var(--chat-text)'
          },
          '.cm-scroller': {
            overflow: 'auto',
            fontFamily:
              '"JetBrains Mono", "Fira Code", "SFMono-Regular", Consolas, "Liberation Mono", Menlo, monospace',
            lineHeight: '1.65',
            color: props.lightSurface ? '#1f2937' : 'var(--chat-text)'
          },
          '.cm-content': {
            padding: '12px 0 20px'
          },
          '.cm-line': {
            padding: '0 12px'
          },
          '.cm-gutters': {
            backgroundColor: props.lightSurface ? '#f8fafc' : 'transparent',
            borderRight: props.lightSurface
              ? '1px solid rgba(148, 163, 184, 0.28)'
              : '1px solid rgba(77, 216, 255, 0.12)',
            color: props.lightSurface ? '#94a3b8' : 'var(--chat-muted)'
          },
          '.cm-activeLine': {
            backgroundColor: props.lightSurface ? 'rgba(59, 130, 246, 0.06)' : 'rgba(77, 216, 255, 0.06)'
          },
          '.cm-activeLineGutter': {
            backgroundColor: props.lightSurface ? 'rgba(59, 130, 246, 0.08)' : 'rgba(77, 216, 255, 0.08)'
          },
          '.cm-selectionBackground': {
            backgroundColor: props.lightSurface
              ? 'rgba(59, 130, 246, 0.18) !important'
              : 'rgba(77, 216, 255, 0.22) !important'
          },
          '.cm-cursor': {
            borderLeftColor: props.lightSurface ? '#2563eb' : 'var(--chat-primary)'
          },
          '.cm-placeholder': {
            color: props.lightSurface ? '#94a3b8' : 'var(--chat-muted)'
          },
          '.cm-lineNumbers': {
            color: props.lightSurface ? '#94a3b8' : 'var(--chat-muted)'
          },
          '.cm-content, .cm-line': {
            color: props.lightSurface ? '#1f2937' : 'var(--chat-text)'
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
      ]
    });
    editorView.setState(state);
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
