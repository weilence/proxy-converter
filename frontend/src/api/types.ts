export type TokenStatus = 'valid' | 'expired' | 'disabled'

// Mirrors `TokenJson` in src/admin.rs.
export interface TokenInfo {
  id: number
  token: string
  name: string
  config: string
  status: TokenStatus
  expires_at: string
  last_used_at: string
  created_at: string
}

export type MrsStatus = 'converted' | 'skipped' | 'failed'

// Mirrors `MrsProviderResult` / `MrsConvertResponse` in src/admin.rs.
export interface MrsProviderResult {
  name: string
  behavior: string
  status: MrsStatus
  size?: number
  url?: string
  reason?: string
  error?: string
}

export interface MrsConvertResult {
  results: MrsProviderResult[]
  config: string
}
