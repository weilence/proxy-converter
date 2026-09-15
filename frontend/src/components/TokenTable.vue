<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'
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

async function remove(row: TokenInfo) {
  if (!window.confirm(`确定删除令牌「${row.token}」吗？删除后立即失效。`)) return
  try {
    await api.removeToken(row.id)
    toast.add({ title: '已删除', color: 'success' })
  } catch (err) {
    toast.add({ title: describe(err, '删除失败'), color: 'error' })
  }
  emit('reload')
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
          <UButton size="xs" color="neutral" variant="soft" @click="emit('edit', row.original)">
            配置
          </UButton>
          <UButton size="xs" color="neutral" variant="soft" @click="toggleEnabled(row.original)">
            {{ row.original.status === 'disabled' ? '启用' : '停用' }}
          </UButton>
          <UButton size="xs" color="error" variant="soft" @click="remove(row.original)">
            删除
          </UButton>
        </div>
      </template>
      <template #empty>
        <p class="py-7 text-center text-sm text-muted">还没有令牌，先添加一个吧。</p>
      </template>
    </UTable>
  </div>
</template>
