<script setup lang="ts">
import { ref } from 'vue'
import { api, ApiError } from '../api/client'

const emit = defineEmits<{ added: [] }>()
const toast = useToast()

const token = ref('')
const name = ref('')
const days = ref('')
const config = ref('')
const submitting = ref(false)

async function submit() {
  const value = token.value.trim()
  if (!value || submitting.value) return

  submitting.value = true
  try {
    await api.addToken({
      token: value,
      name: name.value.trim(),
      days: days.value.trim() === '' ? null : Number(days.value),
      config: config.value.trim(),
    })
    toast.add({ title: '令牌已添加', color: 'success' })
    token.value = ''
    name.value = ''
    days.value = ''
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
    <div class="grid items-end gap-3 md:grid-cols-[2fr_2fr_1fr_auto]">
      <UFormField label="令牌" required>
        <UInput v-model="token" placeholder="例如 my-secret-token" class="w-full" />
      </UFormField>
      <UFormField label="备注">
        <UInput v-model="name" placeholder="例如 家里的设备" class="w-full" />
      </UFormField>
      <UFormField label="有效天数">
        <UInput v-model="days" type="number" :min="0" placeholder="留空=永久" class="w-full" />
      </UFormField>
      <UButton type="submit" :loading="submitting">添加令牌</UButton>
      <UFormField label="配置内容" class="md:col-span-4">
        <UTextarea
          v-model="config"
          :rows="5"
          spellcheck="false"
          placeholder="粘贴 YAML 配置全文；留空则用户获取配置时返回空内容"
          class="w-full font-mono"
        />
      </UFormField>
    </div>
  </form>
</template>
