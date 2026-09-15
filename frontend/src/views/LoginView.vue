<script setup lang="ts">
import { ref } from 'vue'
import { api } from '../api/client'

const emit = defineEmits<{ 'logged-in': [] }>()

const password = ref('')
const error = ref('')
const submitting = ref(false)

async function submit() {
  if (submitting.value) return
  submitting.value = true
  error.value = ''
  try {
    await api.login(password.value)
    password.value = ''
    emit('logged-in')
  } catch (err) {
    error.value = err instanceof Error ? err.message : '登录失败'
  } finally {
    submitting.value = false
  }
}
</script>

<template>
  <div class="flex min-h-screen items-center justify-center p-6">
    <form
      class="w-full max-w-sm rounded-xl border border-default bg-default p-8 shadow-sm"
      @submit.prevent="submit"
    >
      <h1 class="text-xl font-semibold">proxy-converter</h1>
      <p class="mt-1 mb-6 text-sm text-muted">管理后台 · 请输入管理员密码</p>
      <UFormField label="密码">
        <UInput
          v-model="password"
          type="password"
          autocomplete="current-password"
          autofocus
          class="w-full"
        />
      </UFormField>
      <UButton type="submit" block :loading="submitting" class="mt-5">登 录</UButton>
      <p class="mt-3 min-h-5 text-sm text-error">{{ error }}</p>
    </form>
  </div>
</template>
