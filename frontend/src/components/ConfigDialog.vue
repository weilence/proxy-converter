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

// Snapshot of the token being edited; survives the close animation.
const target = ref<TokenInfo | null>(null)
const name = ref('')
const config = ref('')
watch(
  () => props.token,
  (token) => {
    if (!token) return
    target.value = token
    name.value = token.name
    config.value = token.config
  },
)

const saving = ref(false)

async function save() {
  const token = target.value
  if (!token || saving.value) return
  saving.value = true
  try {
    await api.setName(token.id, name.value.trim())
    await api.setConfig(token.id, config.value.trim())
    toast.add({ title: '令牌已更新', color: 'success' })
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
    :dismissible="false"
    title="修改令牌"
    :description="target ? `令牌「${target.token}」的备注与配置内容。` : ''"
  >
    <template #body>
      <div class="flex w-full flex-col gap-4">
        <UFormField label="备注">
          <UInput v-model="name" placeholder="例如 家里的设备" class="w-full" />
        </UFormField>
        <UFormField
          label="配置内容"
          description="YAML 全文；留空表示不绑定，用户获取配置时将返回空内容。"
        >
          <YamlEditor v-model="config" />
        </UFormField>
      </div>
    </template>
    <template #footer>
      <div class="flex w-full justify-end gap-2">
        <UButton color="neutral" variant="soft" @click="open = false">取消</UButton>
        <UButton :loading="saving" @click="save">保存</UButton>
      </div>
    </template>
  </UModal>
</template>
