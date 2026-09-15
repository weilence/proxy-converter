<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { api, ApiError } from '../api/client'
import type { TokenInfo } from '../api/types'

const props = defineProps<{ token: TokenInfo | null }>()
const emit = defineEmits<{ 'update:token': [token: TokenInfo | null]; saved: [] }>()
const toast = useToast()

const open = computed({
  get: () => props.token !== null,
  set: (value) => {
    if (!value) emit('update:token', null)
  },
})

const config = ref('')
watch(
  () => props.token,
  (token) => {
    config.value = token?.config ?? ''
  },
)

const saving = ref(false)

async function save() {
  if (!props.token || saving.value) return
  saving.value = true
  try {
    await api.setConfig(props.token.id, config.value.trim())
    toast.add({ title: '配置内容已更新', color: 'success' })
    emit('update:token', null)
    emit('saved')
  } catch (err) {
    const message
      = err instanceof ApiError && err.status === 404
        ? '令牌不存在'
        : err instanceof Error
          ? err.message
          : '保存失败'
    toast.add({ title: message, color: 'error' })
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <UModal
    v-model:open="open"
    title="编辑配置内容"
    description="YAML 全文；留空表示不绑定，用户获取配置时将返回空内容。"
  >
    <template #body>
      <UTextarea
        v-model="config"
        :rows="14"
        spellcheck="false"
        class="w-full font-mono"
        @keydown.meta.enter.prevent="save"
        @keydown.ctrl.enter.prevent="save"
      />
    </template>
    <template #footer>
      <div class="flex w-full justify-end gap-2">
        <UButton color="neutral" variant="soft" @click="open = false">取消</UButton>
        <UButton :loading="saving" @click="save">保存</UButton>
      </div>
    </template>
  </UModal>
</template>
