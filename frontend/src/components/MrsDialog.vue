<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { api, ApiError } from '../api/client'
import type { MrsConvertResult, MrsProviderResult, TokenInfo } from '../api/types'

const props = defineProps<{ token: TokenInfo | null }>()
const emit = defineEmits<{ 'update:token': [token: TokenInfo | null]; saved: [] }>()
const toast = useToast()

const open = computed({
  get: () => props.token !== null,
  set: (value) => {
    if (!value) emit('update:token', null)
  },
})

// Snapshot of the token being converted; survives the close animation.
const target = ref<TokenInfo | null>(null)
const baseUrl = ref('')
const running = ref(false)
const result = ref<MrsConvertResult | null>(null)
const applying = ref(false)

watch(
  () => props.token,
  (token) => {
    if (!token) return
    target.value = token
    baseUrl.value = window.location.origin
    result.value = null
    run()
  },
)

async function run() {
  const token = target.value
  if (!token || running.value) return
  running.value = true
  try {
    result.value = await api.convertMrs(token.id, baseUrl.value.trim() || window.location.origin)
  } catch (err) {
    const message
      = err instanceof ApiError && err.status === 400
        ? err.message
        : err instanceof Error
          ? err.message
          : '转换失败'
    toast.add({ title: message, color: 'error' })
  } finally {
    running.value = false
  }
}

const REASON_LABEL: Record<string, string> = {
  classical: 'classical 规则不支持 mrs',
  'already-mrs': '已经是 mrs 格式',
  'unsupported-behavior': '不支持的 behavior',
  'unsupported-format': '不支持的 format',
  'unsupported-type': '仅支持 http 类型的 provider',
}

const STATUS_BADGE = {
  converted: { label: '已转换', color: 'success' },
  skipped: { label: '跳过', color: 'neutral' },
  failed: { label: '失败', color: 'error' },
} as const

function sizeText(size: number | undefined): string {
  if (size === undefined) return ''
  return size >= 1024 * 1024
    ? ` (${(size / 1024 / 1024).toFixed(1)} MB)`
    : ` (${(size / 1024).toFixed(1)} KB)`
}

function reasonText(item: MrsProviderResult): string {
  if (item.status === 'skipped') {
    return item.reason ? REASON_LABEL[item.reason] ?? item.reason : ''
  }
  return item.error ?? ''
}

async function copy(text: string | undefined, title: string) {
  if (!text) return
  try {
    await navigator.clipboard.writeText(text)
    toast.add({ title, color: 'success' })
  } catch {
    toast.add({ title: '复制失败，请手动复制', color: 'error' })
  }
}

async function apply() {
  const token = target.value
  if (!token || !result.value || applying.value) return
  applying.value = true
  try {
    await api.setConfig(token.id, result.value.config.trim())
    toast.add({ title: '已应用到令牌', color: 'success' })
    emit('update:token', null)
    emit('saved')
  } catch (err) {
    const message = err instanceof Error ? err.message : '应用失败'
    toast.add({ title: message, color: 'error' })
  } finally {
    applying.value = false
  }
}
</script>

<template>
  <UModal
    v-model:open="open"
    :dismissible="false"
    title="转换 mrs"
    :description="
      target
        ? `下载令牌「${target.token}」配置中的 rule-providers，转换为 mrs 后由本服务托管。`
        : ''
    "
  >
    <template #body>
      <div class="flex w-full flex-col gap-4">
        <UFormField
          label="基础地址"
          description="客户端实际访问本服务的地址；改写后的配置会用它拼出 mrs 下载链接。"
        >
          <div class="flex w-full gap-2">
            <UInput v-model="baseUrl" class="flex-1" placeholder="https://sub.example.com" />
            <UButton color="neutral" variant="soft" :loading="running" @click="run">
              {{ result ? '重新转换' : '开始转换' }}
            </UButton>
          </div>
        </UFormField>

        <div v-if="result" class="flex flex-col gap-1.5">
          <div
            v-for="item in result.results"
            :key="item.name"
            class="flex items-start justify-between gap-2 rounded-lg border border-default px-3 py-2"
          >
            <div class="min-w-0">
              <p class="truncate font-mono text-xs" :title="item.name">{{ item.name }}</p>
              <p class="truncate text-xs text-muted">
                {{ item.behavior }}{{ sizeText(item.size) }}
                <span v-if="item.status !== 'converted'" class="text-error">
                  {{ reasonText(item) }}
                </span>
              </p>
            </div>
            <div class="flex shrink-0 items-center gap-1">
              <UButton
                v-if="item.url"
                size="xs"
                color="neutral"
                variant="ghost"
                icon="i-lucide-copy"
                @click="copy(item.url, '链接已复制')"
              />
              <UBadge :color="STATUS_BADGE[item.status].color" variant="subtle">
                {{ STATUS_BADGE[item.status].label }}
              </UBadge>
            </div>
          </div>
        </div>

        <UFormField v-if="result" label="改写后的配置预览">
          <pre
            class="max-h-60 overflow-auto rounded-lg border border-default bg-elevated p-3 font-mono text-xs whitespace-pre-wrap"
          >{{ result.config }}</pre>
        </UFormField>
      </div>
    </template>
    <template #footer>
      <div class="flex w-full justify-between gap-2">
        <UButton
          v-if="result"
          color="neutral"
          variant="soft"
          icon="i-lucide-copy"
          @click="copy(result.config, '配置已复制')"
        >
          复制配置
        </UButton>
        <div v-else />
        <div class="flex gap-2">
          <UButton color="neutral" variant="soft" @click="open = false">关闭</UButton>
          <UButton
            v-if="result"
            :loading="applying"
            :disabled="running"
            @click="apply"
          >
            应用到令牌
          </UButton>
        </div>
      </div>
    </template>
  </UModal>
</template>
