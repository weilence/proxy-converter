<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'
import { computed, ref } from 'vue'
import { api, ApiError } from '../api/client'
import type { TokenInfo, TokenStatus } from '../api/types'

const props = defineProps<{ tokens: TokenInfo[] }>()
const emit = defineEmits<{ edit: [token: TokenInfo]; reload: [] }>()
const toast = useToast()

const STATUS_LABEL: Record<TokenStatus, string> = {
  valid: '有效',
  expired: '已过期',
  disabled: '已停用',
}
const STATUS_COLOR = {
  valid: 'success',
  expired: 'warning',
  disabled: 'neutral',
} as const

function configPreview(config: string): string {
  const flat = config.split(/\s+/).filter(Boolean).join(' ')
  if (!flat) return '-'
  return flat.length > 32 ? `${flat.slice(0, 32)}…` : flat
}

function describe(err: unknown, fallback: string): string {
  if (err instanceof ApiError && err.status === 404) return '令牌不存在'
  return err instanceof Error ? err.message : fallback
}

async function toggleEnabled(row: TokenInfo) {
  const enabled = row.status === 'disabled'
  try {
    await api.setEnabled(row.id, enabled)
    toast.add({ title: enabled ? '已启用' : '已停用', color: 'success' })
  } catch (err) {
    toast.add({ title: describe(err, '操作失败'), color: 'error' })
  }
  emit('reload')
}

async function copyConfigUrl(row: TokenInfo) {
  const url = `${window.location.origin}/config?token=${encodeURIComponent(row.token)}`
  try {
    await navigator.clipboard.writeText(url)
    toast.add({ title: '配置链接已复制', color: 'success' })
  } catch {
    toast.add({ title: '复制失败，请手动复制', color: 'error' })
  }
}

const duplicating = ref<TokenInfo | null>(null)
const duplicatingBusy = ref(false)
const dupOpen = computed({
  get: () => duplicating.value !== null,
  set: (value) => {
    if (!value) duplicating.value = null
  },
})

async function duplicate() {
  const row = duplicating.value
  if (!row || duplicatingBusy.value) return
  duplicatingBusy.value = true
  try {
    const created = await api.duplicateToken(row.id)
    toast.add({ title: `已复制，新令牌：${created.token}`, color: 'success' })
    duplicating.value = null
    emit('reload')
  } catch (err) {
    toast.add({ title: describe(err, '复制失败'), color: 'error' })
  } finally {
    duplicatingBusy.value = false
  }
}

const deleting = ref<TokenInfo | null>(null)
const removing = ref(false)
const deleteOpen = computed({
  get: () => deleting.value !== null,
  set: (value) => {
    if (!value) deleting.value = null
  },
})

const converting = ref<TokenInfo | null>(null)
const geoConverting = ref<TokenInfo | null>(null)

async function remove() {
  const row = deleting.value
  if (!row || removing.value) return
  removing.value = true
  try {
    await api.removeToken(row.id)
    toast.add({ title: '已删除', color: 'success' })
    deleting.value = null
    emit('reload')
  } catch (err) {
    toast.add({ title: describe(err, '删除失败'), color: 'error' })
  } finally {
    removing.value = false
  }
}

const columns: TableColumn<TokenInfo>[] = [
  { accessorKey: 'id', header: 'ID' },
  { accessorKey: 'token', header: '令牌' },
  { accessorKey: 'name', header: '备注' },
  { accessorKey: 'config', header: '配置内容' },
  { accessorKey: 'status', header: '状态' },
  { accessorKey: 'expires_at', header: '过期时间' },
  { accessorKey: 'last_used_at', header: '最近使用' },
  { accessorKey: 'created_at', header: '创建时间' },
  { id: 'actions', header: '操作' },
]
</script>

<template>
  <div class="rounded-xl border border-default bg-default p-5">
    <UTable :data="props.tokens" :columns="columns">
      <template #token-cell="{ row }">
        <span class="font-mono text-xs">{{ row.original.token }}</span>
      </template>
      <template #name-cell="{ row }">
        {{ row.original.name || '-' }}
      </template>
      <template #config-cell="{ row }">
        <span
          class="block max-w-56 truncate font-mono text-xs"
          :title="row.original.config"
        >{{ configPreview(row.original.config) }}</span>
      </template>
      <template #status-cell="{ row }">
        <UBadge :color="STATUS_COLOR[row.original.status]" variant="subtle">
          {{ STATUS_LABEL[row.original.status] }}
        </UBadge>
      </template>
      <template #actions-cell="{ row }">
        <div class="flex gap-1.5">
          <UTooltip text="复制配置链接">
            <UButton
              size="xs"
              color="neutral"
              variant="soft"
              icon="i-lucide-link"
              @click="copyConfigUrl(row.original)"
            />
          </UTooltip>
          <UButton size="xs" color="neutral" variant="soft" @click="emit('edit', row.original)">
            修改
          </UButton>
          <UButton size="xs" color="neutral" variant="soft" @click="converting = row.original">
            转 mrs
          </UButton>
          <UButton size="xs" color="neutral" variant="soft" @click="geoConverting = row.original">
            转 Geo
          </UButton>
          <UButton size="xs" color="neutral" variant="soft" @click="duplicating = row.original">
            复制
          </UButton>
          <UButton size="xs" color="neutral" variant="soft" @click="toggleEnabled(row.original)">
            {{ row.original.status === 'disabled' ? '启用' : '停用' }}
          </UButton>
          <UButton size="xs" color="error" variant="soft" @click="deleting = row.original">
            删除
          </UButton>
        </div>
      </template>
      <template #empty>
        <p class="py-7 text-center text-sm text-muted">还没有令牌，先添加一个吧。</p>
      </template>
    </UTable>

    <UModal
      v-model:open="deleteOpen"
      title="删除令牌"
      :description="`确定删除令牌「${deleting?.token}」吗？删除后立即失效。`"
    >
      <template #footer>
        <div class="flex w-full justify-end gap-2">
          <UButton color="neutral" variant="soft" @click="deleteOpen = false">取消</UButton>
          <UButton color="error" :loading="removing" @click="remove">删除</UButton>
        </div>
      </template>
    </UModal>

    <UModal
      v-model:open="dupOpen"
      title="复制令牌"
      :description="`将为令牌「${duplicating?.token}」生成一个新令牌，复制其配置和已托管的 mrs/geo 文件，配置中的下载链接会改写为新令牌。`"
    >
      <template #footer>
        <div class="flex w-full justify-end gap-2">
          <UButton color="neutral" variant="soft" @click="dupOpen = false">取消</UButton>
          <UButton :loading="duplicatingBusy" @click="duplicate">复制</UButton>
        </div>
      </template>
    </UModal>

    <MrsDialog v-model:token="converting" @saved="emit('reload')" />
    <GeoDialog v-model:token="geoConverting" @saved="emit('reload')" />
  </div>
</template>
