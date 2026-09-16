import type { MrsConvertResult, TokenInfo } from './types'

export class ApiError extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message)
    this.name = 'ApiError'
  }
}

async function request(url: string, init?: RequestInit): Promise<Response> {
  const res = await fetch(url, {
    headers: { 'Content-Type': 'application/json' },
    ...init,
  })
  if (res.status === 401) {
    throw new ApiError(401, '未登录或会话已过期')
  }
  return res
}

async function errorText(res: Response, fallback: string): Promise<string> {
  const text = (await res.text().catch(() => '')).trim()
  return (text || fallback).slice(0, 120)
}

export const api = {
  /** Probe session validity by hitting an authed endpoint. */
  async loggedIn(): Promise<boolean> {
    try {
      return (await fetch('/admin/api/tokens')).ok
    } catch {
      return false
    }
  },

  async login(password: string): Promise<void> {
    const res = await fetch('/admin/api/login', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ password }),
    })
    if (res.status === 401) throw new ApiError(401, '密码错误')
    if (!res.ok) throw new ApiError(res.status, `登录失败 (${res.status})`)
  },

  async logout(): Promise<void> {
    try {
      await fetch('/admin/api/logout', { method: 'POST' })
    } catch {
      // The session cookie expires with its TTL anyway.
    }
  },

  async listTokens(): Promise<TokenInfo[]> {
    const res = await request('/admin/api/tokens')
    if (!res.ok) throw new ApiError(res.status, await errorText(res, '加载令牌失败'))
    return res.json()
  },

  async addToken(input: {
    token: string
    name: string
    days: number | null
    config: string
  }): Promise<void> {
    const res = await request('/admin/api/tokens', {
      method: 'POST',
      body: JSON.stringify(input),
    })
    if (!res.ok) throw new ApiError(res.status, await errorText(res, `添加失败 (${res.status})`))
  },

  async setConfig(id: number, config: string): Promise<void> {
    const res = await request(`/admin/api/tokens/${id}/config`, {
      method: 'POST',
      body: JSON.stringify({ config }),
    })
    if (!res.ok) throw new ApiError(res.status, await errorText(res, '保存失败'))
  },

  async setName(id: number, name: string): Promise<void> {
    const res = await request(`/admin/api/tokens/${id}/name`, {
      method: 'POST',
      body: JSON.stringify({ name }),
    })
    if (!res.ok) throw new ApiError(res.status, await errorText(res, '保存失败'))
  },

  async convertMrs(id: number, baseUrl: string): Promise<MrsConvertResult> {
    const res = await request(`/admin/api/tokens/${id}/convert-mrs`, {
      method: 'POST',
      body: JSON.stringify({ base_url: baseUrl }),
    })
    if (!res.ok) throw new ApiError(res.status, await errorText(res, '转换失败'))
    return res.json()
  },

  async setEnabled(id: number, enabled: boolean): Promise<void> {
    const res = await request(`/admin/api/tokens/${id}/${enabled ? 'enable' : 'disable'}`, {
      method: 'POST',
    })
    if (!res.ok) throw new ApiError(res.status, await errorText(res, '操作失败'))
  },

  async removeToken(id: number): Promise<void> {
    const res = await request(`/admin/api/tokens/${id}`, { method: 'DELETE' })
    if (!res.ok) throw new ApiError(res.status, await errorText(res, '删除失败'))
  },
}
