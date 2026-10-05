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

export class VelaApiError extends Error {
  readonly status: number | undefined
  readonly retryAfterSeconds: number | undefined
  constructor(message: string, status?: number, retryAfterSeconds?: number) {
    super(message)
    this.name = 'VelaApiError'
    this.status = status
    this.retryAfterSeconds = retryAfterSeconds
  }
}

export interface ClientOptions {
  baseUrl?: string
  bearerToken?: string
  fetch?: typeof fetch
  timeoutMs?: number
}

/** Read-only and signed-write client for the stable Vela HTTP v1 surface. */
export class VelaClient {
  private readonly baseUrl: string
  private readonly bearerToken?: string
  private readonly transport: typeof fetch
  private readonly timeoutMs: number

  constructor(options: ClientOptions = {}) {
    this.baseUrl = (options.baseUrl ?? 'https://vela-engine.fly.dev').replace(/\/$/, '')
    const parsed = new URL(this.baseUrl)
    if (!['http:', 'https:'].includes(parsed.protocol) || parsed.username || parsed.password || parsed.search || parsed.hash) throw new TypeError('baseUrl must use HTTP without credentials, query or fragment')
    this.bearerToken = options.bearerToken
    this.transport = options.fetch ?? fetch
    this.timeoutMs = options.timeoutMs ?? 10_000
    if (!Number.isFinite(this.timeoutMs) || this.timeoutMs <= 0 || this.timeoutMs > 2_147_483_647) throw new RangeError('timeoutMs must be positive and at most 2147483647')
  }

  private async request<T>(path: string, body?: object): Promise<T> {
    const controller = new AbortController()
    const timer = setTimeout(() => controller.abort(), this.timeoutMs)
    try {
    const response = await this.transport(`${this.baseUrl}/v1${path}`, {
      method: body ? 'POST' : 'GET',
      signal: controller.signal,
      headers: {
        ...(this.bearerToken ? { Authorization: `Bearer ${this.bearerToken}` } : {}),
        ...(body ? { 'Content-Type': 'application/json' } : {}),
      },
      body: body ? JSON.stringify(body) : undefined,
    })
    const retryHeader = response.headers.get('Retry-After')
    const retryAfter = retryHeader !== null && /^\d+$/.test(retryHeader) ? Number(retryHeader) : undefined
    let envelope: unknown
    try { envelope = await response.json() } catch {
      throw new VelaApiError('Vela API returned an invalid JSON response', response.status, retryAfter)
    }
    const errorText = envelope && typeof envelope === 'object' && 'error' in envelope && typeof envelope.error === 'string' ? envelope.error.slice(0, 512) : undefined
    if (!response.ok) throw new VelaApiError(errorText || `Vela API returned HTTP ${response.status}`, response.status, retryAfter)
    if (!envelope || typeof envelope !== 'object' || !('ok' in envelope) || envelope.ok !== true || !('data' in envelope)) {
      throw new VelaApiError(errorText || 'Vela API returned an unsuccessful or malformed response', response.status, retryAfter)
    }
    return envelope.data as T
    } catch (error) {
      if (error instanceof VelaApiError) throw error
      throw new VelaApiError(controller.signal.aborted ? 'Vela request timed out; write outcome may be indeterminate' : 'Vela network request failed')
    } finally { clearTimeout(timer) }
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

  orders(address: string): Promise<OpenOrder[]> {
    return this.request(`/account/${encodeURIComponent(address)}/orders`)
  }

  fills(address: string): Promise<AccountFill[]> {
    return this.request(`/account/${encodeURIComponent(address)}/fills`)
  }

  portfolio(address: string, method: 'fifo' | 'hifo' = 'fifo'): Promise<Portfolio> {
    return this.request(`/portfolio/${encodeURIComponent(address)}?method=${method}`)
  }

  agents(master: string): Promise<{ master: string; agents: AgentStatus[] }> {
    return this.request(`/agents/${encodeURIComponent(master)}`)
  }

  algo(parentId: number): Promise<AlgoSnapshot> {
    assertSafeId(parentId)
    return this.request(`/orders/algo/${parentId}`)
  }

  algos(address: string): Promise<AlgoSnapshot[]> {
    return this.request(`/account/${encodeURIComponent(address)}/algos`)
  }

  cancelAlgo(body: { parent_id: number; address: string; nonce: number; signature: string }): Promise<{ parent_id: number; canceled: boolean }> {
    algoCancelSigningMessage(body.parent_id, body.nonce)
    return this.request('/orders/algo/cancel', body)
  }

  /** Sign twapSigningMessage(input) externally, then submit its signature. */
  placeTwap(input: TwapInput, signature: string): Promise<{ parent_id: number; status: AlgoStatus; slice_count: number }> {
    twapSigningMessage(input)
    return this.request('/orders/algo/twap', { ...input, slices: input.slices ?? 12, signature, signature_version: 2 })
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

export interface TwapInput {
  address: string
  market: string
  side: 'Bid' | 'Ask'
  quantity: number
  duration_secs: number
  price_limit: number
  slices?: number
  nonce: number
}

/** EIP-191 message bytes must be signed as this exact UTF-8 string. */
export function twapSigningMessage(input: TwapInput): string {
  const slices = input.slices ?? 12
  for (const value of [input.quantity, input.duration_secs, input.price_limit, slices, input.nonce]) {
    if (!Number.isSafeInteger(value) || value < 0) throw new RangeError('TWAP numbers must be nonnegative safe integers')
  }
  if (slices < 1 || slices > 240 || input.quantity < slices || input.duration_secs < 1 || input.duration_secs > 86400 || input.price_limit < 1 || !Number.isSafeInteger(input.nonce + slices)) {
    throw new RangeError('Invalid TWAP schedule or nonce range')
  }
  if (!/^0x[0-9a-fA-F]{40}$/.test(input.address) || !input.market || !['Bid', 'Ask'].includes(input.side)) throw new TypeError('Invalid TWAP address, market or side')
  return JSON.stringify(['vela:algo:twap:v2', input.address.toLowerCase(), input.market, input.side, input.quantity, input.duration_secs, input.price_limit, slices, input.nonce])
}

export type AlgoStatus = 'Running' | 'Completed' | 'Incomplete' | 'Canceled' | 'Failed'
export interface AlgoSnapshot {
  parent_id: number
  user: string
  market: string
  side: string
  total_quantity: number
  filled_quantity: number
  remaining_quantity: number
  price_limit: number | null
  duration_secs: number
  slices: number
  started_at_ms: number
  status: AlgoStatus
}

function assertSafeId(value: number): void {
  if (!Number.isSafeInteger(value) || value < 0) throw new RangeError('IDs and nonces must be nonnegative safe integers')
}

export function algoCancelSigningMessage(parentId: number, nonce: number): string {
  assertSafeId(parentId)
  assertSafeId(nonce)
  return `vela:algo:cancel:${parentId}:${nonce}`
}

export type AccountFill = components['schemas']['AccountFillResponse']
export interface OpenOrder {
  id: number
  market: string
  side: string
  order_type: string
  price: string
  quantity: string
  filled_quantity: string
  display_quantity: string | null
  min_quantity: string | null
  stp: string
}
