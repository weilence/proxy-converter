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
