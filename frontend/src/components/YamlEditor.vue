<script setup lang="ts">
import { yaml } from '@codemirror/lang-yaml'
import { EditorView, basicSetup } from 'codemirror'
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'

const props = withDefaults(defineProps<{ modelValue: string; height?: string }>(), {
  height: '22rem',
})
const emit = defineEmits<{ 'update:modelValue': [value: string] }>()

const host = ref<HTMLDivElement | null>(null)
let view: EditorView | null = null

onMounted(() => {
  view = new EditorView({
    doc: props.modelValue,
    parent: host.value!,
    extensions: [
      basicSetup,
      yaml(),
      EditorView.updateListener.of((update) => {
        if (update.docChanged) emit('update:modelValue', update.state.doc.toString())
      }),
    ],
  })
})

watch(
  () => props.modelValue,
  (value) => {
    const current = view?.state.doc.toString() ?? ''
    if (view && value !== current) {
      view.dispatch({ changes: { from: 0, to: current.length, insert: value } })
    }
  },
)

onBeforeUnmount(() => {
  view?.destroy()
  view = null
})
</script>

<template>
  <div ref="host" class="yaml-editor" :style="{ height: props.height }" />
</template>

<style scoped>
.yaml-editor {
  width: 100%;
  overflow: auto;
  border: 1px solid var(--ui-border);
  border-radius: calc(var(--ui-radius) * 1.5);
  background: var(--ui-bg);
  font-size: 13px;
  text-align: left;
}

.yaml-editor :deep(.cm-editor) {
  height: 100%;
}

.yaml-editor :deep(.cm-editor.cm-focused) {
  outline: none;
  border-color: transparent;
}

.yaml-editor:has(:deep(.cm-editor.cm-focused)) {
  border-color: var(--ui-primary);
}

.yaml-editor :deep(.cm-gutters) {
  background: var(--ui-bg-muted);
  border-right: 1px solid var(--ui-border);
  color: var(--ui-text-dimmed);
}

.yaml-editor :deep(.cm-activeLine),
.yaml-editor :deep(.cm-activeLineGutter) {
  background: var(--ui-bg-accented);
}

.yaml-editor :deep(.cm-selectionBackground) {
  background: color-mix(in srgb, var(--ui-primary) 20%, transparent) !important;
}
</style>
