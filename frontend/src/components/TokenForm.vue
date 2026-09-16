<script setup lang="ts">
import { ref } from 'vue'
import { api } from '../api/client'

const emit = defineEmits<{ added: [] }>()
const toast = useToast()

const name = ref('')
const days = ref<number | null>(null)
const config = ref('')
const submitting = ref(false)

async function submit() {
  if (submitting.value) return

  submitting.value = true
  try {
    const created = await api.addToken({
      name: name.value.trim(),
      days: days.value,
      config: config.value.trim(),
    })
    toast.add({ title: `令牌已添加：${created.token}`, color: 'success' })
    name.value = ''
    days.value = null
    config.value = ''
    emit('added')
  } catch (err) {
    toast.add({ title: err instanceof Error ? err.message : '添加失败', color: 'error' })
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
