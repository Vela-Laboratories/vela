import type { components, paths } from './schema'

export type Market = components['schemas']['MarketResponse']
export interface Book {
  market: string
  bids: { price: string; quantity: string }[]
  asks: { price: string; quantity: string }[]
}
export type Balance = components['schemas']['BalanceResponse']
export type Portfolio = components['schemas']['PortfolioResponse']

export interface AgentScope {
  allowed_markets: string[] | null
  allowed_order_types: string[] | null
  allowed_sides: ('Bid' | 'Ask')[] | null
  max_notional_per_hour: number | null
  max_notional_per_day: number | null
}

export interface AgentStatus {
  agent: string
  expires_at_ms: number
  max_notional_per_order: number
  revoked: boolean
  active: boolean
  nonce: number
  scope: AgentScope
}

type MarketsPath = paths['/v1/markets']['get']
type MarketsData = NonNullable<MarketsPath['responses'][200]['content']['application/json']['data']>

export interface ClientOptions {
  baseUrl?: string
  bearerToken?: string
  fetch?: typeof fetch
}

/** Read-only and signed-write client for the stable Vela HTTP v1 surface. */
export class VelaClient {
  private readonly baseUrl: string
  private readonly bearerToken?: string
  private readonly transport: typeof fetch

  constructor(options: ClientOptions = {}) {
    this.baseUrl = (options.baseUrl ?? 'https://vela-engine.fly.dev').replace(/\/$/, '')
    this.bearerToken = options.bearerToken
    this.transport = options.fetch ?? fetch
  }

  private async request<T>(path: string, body?: object): Promise<T> {
    const response = await this.transport(`${this.baseUrl}/v1${path}`, {
      method: body ? 'POST' : 'GET',
      headers: {
        ...(this.bearerToken ? { Authorization: `Bearer ${this.bearerToken}` } : {}),
        ...(body ? { 'Content-Type': 'application/json' } : {}),
      },
      body: body ? JSON.stringify(body) : undefined,
    })
    if (!response.ok) throw new Error(`Vela API returned HTTP ${response.status}`)
    const envelope: unknown = await response.json()
    if (!envelope || typeof envelope !== 'object' || !('ok' in envelope) || envelope.ok !== true || !('data' in envelope)) {
      throw new Error('Vela API returned an unsuccessful or malformed response')
    }
    return envelope.data as T
  }

  markets(): Promise<MarketsData> {
    return this.request('/markets')
  }

  book(market: string): Promise<Book> {
    return this.request(`/markets/${encodeURIComponent(market)}/book`)
  }

  balances(address: string): Promise<Balance[]> {
    return this.request(`/account/${encodeURIComponent(address)}/balances`)
  }

  portfolio(address: string, method: 'fifo' | 'hifo' = 'fifo'): Promise<Portfolio> {
    return this.request(`/portfolio/${encodeURIComponent(address)}?method=${method}`)
  }

  agents(master: string): Promise<{ master: string; agents: AgentStatus[] }> {
    return this.request(`/agents/${encodeURIComponent(master)}`)
  }

  /** Supply an EIP-191 signature from the master wallet; this client never handles private keys. */
  registerAgent(body: {
    master: string
    agent: string
    expires_at_ms: number
    max_notional_per_order: number
    nonce: number
    scope: Partial<AgentScope>
    signature: string
  }): Promise<{ master: string; agent: string; expires_at_ms: number }> {
    return this.request('/agents/register', body)
  }

  /** The caller signs the canonical revoke message before submitting it. */
  revokeAgent(body: { master: string; agent: string; nonce: number; signature: string }): Promise<{ agent: string; revoked: boolean }> {
    return this.request('/agents/revoke', body)
  }
}
