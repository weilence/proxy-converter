<script setup lang="ts">
import { ref } from 'vue'
import { api, ApiError } from '../api/client'

const emit = defineEmits<{ added: [] }>()
const toast = useToast()

const name = ref('')
const days = ref<number | null>(null)
const config = ref('')
const submitting = ref(false)

/** Random URL-safe token, 6 bits of entropy per character. */
function generateToken(length = 16): string {
  const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_'
  const bytes = crypto.getRandomValues(new Uint8Array(length))
  return [...bytes].map((byte) => alphabet[byte & 63]).join('')
}

async function submit() {
  if (submitting.value) return

  submitting.value = true
  try {
    await api.addToken({
      token: generateToken(),
      name: name.value.trim(),
      days: days.value,
      config: config.value.trim(),
    })
    toast.add({ title: '令牌已添加', color: 'success' })
    name.value = ''
    days.value = null
    config.value = ''
    emit('added')
  } catch (err) {
    const message
      = err instanceof ApiError && err.status === 409
        ? '令牌已存在'
        : err instanceof Error
          ? err.message
          : '添加失败'
    toast.add({ title: message, color: 'error' })
  } finally {
    submitting.value = false
  }
}
</script>

<template>
  <form class="rounded-xl border border-default bg-default p-5" @submit.prevent="submit">
    <div class="grid items-end gap-3 md:grid-cols-[2fr_1fr_auto]">
      <UFormField label="备注">
        <UInput v-model="name" placeholder="例如 家里的设备" class="w-full" />
      </UFormField>
      <UFormField label="有效天数">
        <UInputNumber
          v-model="days"
          :min="0"
          :step-size="1"
          placeholder="留空=永久"
          class="w-full"
        />
      </UFormField>
      <UButton type="submit" :loading="submitting">添加令牌</UButton>
      <UFormField label="配置内容" class="md:col-span-3">
        <YamlEditor v-model="config" height="12rem" />
      </UFormField>
    </div>
  </form>
</template>
