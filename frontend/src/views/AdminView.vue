<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { api, ApiError } from '../api/client'
import type { TokenInfo } from '../api/types'
import ConfigDialog from '../components/ConfigDialog.vue'
import TokenForm from '../components/TokenForm.vue'
import TokenTable from '../components/TokenTable.vue'

const emit = defineEmits<{ logout: [] }>()

const tokens = ref<TokenInfo[]>([])
const editing = ref<TokenInfo | null>(null)

async function load() {
  try {
    tokens.value = await api.listTokens()
  } catch (err) {
    if (err instanceof ApiError && err.status === 401) emit('logout')
  }
}

async function logout() {
  await api.logout()
  emit('logout')
}

onMounted(load)
</script>

<template>
  <div class="mx-auto max-w-6xl px-6 py-8">
    <div class="mb-5 flex items-center justify-between">
      <h1 class="text-xl font-semibold">令牌管理</h1>
      <UButton color="neutral" variant="soft" @click="logout">退出登录</UButton>
    </div>

    <TokenForm class="mb-5" @added="load" />
    <TokenTable :tokens="tokens" @edit="editing = $event" @reload="load" />
    <ConfigDialog v-model:token="editing" @saved="load" />
  </div>
</template>
