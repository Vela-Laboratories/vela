// ---------------------------------------------------------------------------
// Vela HTTP API client — typed against the Rust API handler
//
// The interfaces below are hand-authored for ergonomics, but the
// generated `api-types.gen.ts` is the ground truth mirror of the
// backend's `/openapi.json`. To regenerate:
//
//     cargo run --quiet --bin dump-openapi > /tmp/vela-openapi.json \
//       && (cd frontend && npm run generate:api-types \
//             && VELA_OPENAPI_URL= npx openapi-typescript \
//                  /tmp/vela-openapi.json -o lib/api-types.gen.ts)
//
// Or point the script at a running dev server:
//
//     (cd frontend && VELA_OPENAPI_URL=http://localhost:3001 \
//        npm run generate:api-types)
//
// TypeScript will complain in this file if the generated schema and a
// hand-authored interface disagree — see the `_check` assertions.
// ---------------------------------------------------------------------------

import type { components } from './api-types.gen'

/** Re-export of the OpenAPI component schemas so downstream code can
 * import `components["schemas"]["X"]` without depending on the
 * generated file directly. */
export type ApiSchemas = components['schemas']

const API_URL =
  process.env.NEXT_PUBLIC_API_URL ?? 'https://vela-engine.fly.dev'

let authSessionToken: string | null = null

export function setAuthSessionToken(token: string | null): void {
  authSessionToken = token
}

// ---------------------------------------------------------------------------
// Response shapes (mirrors api/src/types.rs)
// ---------------------------------------------------------------------------

export interface ApiResponse<T> {
  ok: boolean
  data?: T
  error?: string
}

export interface MarketResponse {
  id: string
  base: string
  quote: string
  best_bid?: string
  best_ask?: string
  spread?: string
}

export interface BookLevel {
  price: string
  quantity: string
}

export interface BookResponse {
  market: string
  bids: BookLevel[]
  asks: BookLevel[]
}

export interface BalanceResponse {
  asset: string
  available: string
  locked: string
  total: string
}

export type DepositStatus = ApiSchemas['DepositStatusResponse']

export interface Order {
  id: number
  market: string
  side: 'buy' | 'sell' | 'bid' | 'ask'
  order_type: 'limit' | 'market'
  price: string
  quantity: string
  filled_quantity: string
  display_quantity?: string | null
  min_quantity?: string | null
  stp?: string
  status: string
  nonce: number
  client_order_id?: string
  timestamp: number
}

// ---------------------------------------------------------------------------
// Request bodies (mirrors api/src/types.rs)
// ---------------------------------------------------------------------------

export interface PostOrderBody {
  market: string
  side: 'Bid' | 'Ask'
  order_type: 'GoodTillCanceled' | 'PostOnly' | 'ImmediateOrCancel' | 'FillOrKill'
  /** Raw integer price (scaled by PRICE_DECIMALS) */
  price: number
  /** Raw integer quantity (scaled by QUANTITY_DECIMALS) */
  quantity: number
  nonce: number
  client_order_id?: string
  signature_version: 2
  stp?: 'None' | 'CancelTaker' | 'CancelMaker' | 'CancelBoth' | 'DecrementAndCancel'
  min_quantity?: number
  display_quantity?: number
  address: string
  signature: string
}

export interface CancelOrderBody {
  order_id?: number
  client_order_id?: string
  nonce: number
  address: string
  signature: string
}

export interface WithdrawBody {
  withdrawal_id: string
  asset: string
  /** Raw integer amount (8 decimals) */
  amount: string
  nonce: number
  address: string
  destination_address: string
  signature: string
}

export type WithdrawalStatus = ApiSchemas['WithdrawalStatusResponse']

// ---------------------------------------------------------------------------
// Fetch helper
// ---------------------------------------------------------------------------

async function apiFetch<T>(
  path: string,
  init?: RequestInit,
): Promise<ApiResponse<T>> {
  const headers = new Headers(init?.headers)
  if (!headers.has('Content-Type')) headers.set('Content-Type', 'application/json')
  if (authSessionToken) headers.set('Authorization', `Bearer ${authSessionToken}`)
  const res = await fetch(`${API_URL}${path}`, {
    ...init,
    headers,
  })
  if (!res.ok) {
    if (res.status === 401 && authSessionToken) {
      authSessionToken = null
      if (typeof window !== 'undefined') {
        window.dispatchEvent(new Event('vela:auth-expired'))
      }
    }
    const text = await res.text().catch(() => res.statusText)
    return { ok: false, error: text }
  }
  return res.json() as Promise<ApiResponse<T>>
}

// ---------------------------------------------------------------------------
// Public endpoints
// ---------------------------------------------------------------------------

/** GET /markets */
export async function listMarkets(): Promise<ApiResponse<MarketResponse[]>> {
  const apiUrl = process.env.NEXT_PUBLIC_API_URL ?? 'https://vela-engine.fly.dev'
  try {
    const res = await fetch(`${apiUrl}/markets`, {
      cache: 'no-store',
      next: { revalidate: 0 },
    })
    if (!res.ok) {
      const text = await res.text().catch(() => res.statusText)
      return { ok: false, error: text }
    }
    return res.json() as Promise<ApiResponse<MarketResponse[]>>
  } catch (err) {
    console.error(err)
    return { ok: true, data: [] }
  }
}

/** GET /markets/:market/book */
export function getBook(market: string): Promise<ApiResponse<BookResponse>> {
  return apiFetch(`/markets/${encodeURIComponent(market)}/book`)
}

export interface PublicTrade {
  id: string
  market_id: string
  price: number
  quantity: number
  timestamp: number
  side: string
  synthetic: boolean
}

/** GET /trades/:market — newest public fills for one market. */
export function getPublicTrades(market: string): Promise<ApiResponse<PublicTrade[]>> {
  return apiFetch(`/trades/${encodeURIComponent(market)}`, { cache: 'no-store' })
}

export interface FeeTier {
  tier: number
  min_30d_volume_usdc: string
  maker_bps: number
  taker_bps: number
}

export interface FeeSchedule {
  window_days: number
  tiers: FeeTier[]
  notes: string
}

/** GET /fees/schedule — public, authoritative fee tiers. */
export function getFeeSchedule(): Promise<ApiResponse<FeeSchedule>> {
  return apiFetch('/fees/schedule', { cache: 'no-store' })
}

export interface AccountFeeTier {
  address: string
  computed_tier: number
  cached_tier: number
  volume_30d_usdc: string
  maker_bps: number
  taker_bps: number
  next_tier_threshold_usdc: string | null
  note: string
}

/** GET /fees/tier/:address — the authenticated user's effective schedule. */
export function getAccountFeeTier(address: string): Promise<ApiResponse<AccountFeeTier>> {
  return apiFetch(`/fees/tier/${encodeURIComponent(address)}`, { cache: 'no-store' })
}

// ---------------------------------------------------------------------------
// Authenticated endpoints
// ---------------------------------------------------------------------------

export interface AuthChallenge {
  address: string
  nonce: string
  message: string
  expires_at_ms: number
}

export interface AuthSession {
  address: string
  token: string
  expires_at_ms: number
}

export function requestAuthChallenge(
  address: string,
): Promise<ApiResponse<AuthChallenge>> {
  return apiFetch('/auth/challenge', {
    method: 'POST',
    body: JSON.stringify({ address }),
  })
}

export function createAuthSession(
  address: string,
  nonce: string,
  signature: string,
): Promise<ApiResponse<AuthSession>> {
  return apiFetch('/auth/session', {
    method: 'POST',
    body: JSON.stringify({ address, nonce, signature }),
  })
}

export function revokeAuthSession(): Promise<ApiResponse<{ revoked: boolean }>> {
  return apiFetch('/auth/session/revoke', { method: 'POST' })
}

/** GET /account/:address/balances */
export function getBalances(
  address: string,
): Promise<ApiResponse<BalanceResponse[]>> {
  return apiFetch(`/account/${encodeURIComponent(address)}/balances`)
}

/** GET /account/:address/deposits */
export function getDeposits(
  address: string,
  transactionHash?: string,
): Promise<ApiResponse<DepositStatus[]>> {
  const query = new URLSearchParams()
  if (transactionHash) query.set('transaction_hash', transactionHash)
  const suffix = query.size > 0 ? `?${query.toString()}` : ''
  return apiFetch(
    `/account/${encodeURIComponent(address)}/deposits${suffix}`,
    { cache: 'no-store' },
  )
}

/** GET /account/:address/orders */
export function getOrders(address: string): Promise<ApiResponse<Order[]>> {
  return apiFetch(`/account/${encodeURIComponent(address)}/orders`)
}

export interface AccountFill {
  id: string
  market: string
  price: string
  quantity: string
  order_id: number
  counterparty_order_id: number
  role: 'maker' | 'taker'
  side: 'buy' | 'sell'
  timestamp: number
}

/** GET /account/:address/fills */
export function getAccountFills(
  address: string,
): Promise<ApiResponse<AccountFill[]>> {
  return apiFetch(`/account/${encodeURIComponent(address)}/fills`)
}

/** POST /orders */
export function postOrder(
  body: PostOrderBody,
): Promise<ApiResponse<unknown>> {
  return apiFetch('/orders', {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

/** POST /orders/cancel */
export function cancelOrder(
  body: CancelOrderBody,
): Promise<ApiResponse<unknown>> {
  return apiFetch('/orders/cancel', {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

/** POST /withdrawals */
export function initiateWithdrawal(
  body: WithdrawBody,
): Promise<ApiResponse<WithdrawalStatus>> {
  return apiFetch('/withdrawals', {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

/** GET /account/:address/withdrawals */
export function getWithdrawals(
  address: string,
): Promise<ApiResponse<WithdrawalStatus[]>> {
  return apiFetch(`/account/${encodeURIComponent(address)}/withdrawals`, {
    cache: 'no-store',
  })
}

/** POST /withdrawals/:id/cancel */
export function cancelWithdrawal(
  withdrawalId: string,
  body: { nonce: number; address: string; signature: string },
): Promise<ApiResponse<WithdrawalStatus>> {
  return apiFetch(`/withdrawals/${encodeURIComponent(withdrawalId)}/cancel`, {
      method: 'POST',
      body: JSON.stringify(body),
    })
}

export interface OHLCVCandle {
  time: number
  open: number
  high: number
  low: number
  close: number
  volume: number
}

export async function fetchOHLCV(
  marketId: string,
  timeframe: string = '1H',
  limit: number = 200,
): Promise<{ candles: OHLCVCandle[]; hasRealData: boolean; hasLivePrices: boolean }> {
  const apiUrl = process.env.NEXT_PUBLIC_API_URL ?? 'https://vela-engine.fly.dev'
  try {
    const res = await fetch(
      `${apiUrl}/ohlcv/${marketId}?timeframe=${timeframe}&limit=${limit}`,
      { cache: 'no-store' },
    )
    const data = await res.json()
    if (!data.ok) return { candles: [], hasRealData: false, hasLivePrices: false }
    return {
      candles: data.data.candles,
      hasRealData: data.data.has_real_data,
      hasLivePrices: data.data.has_live_prices ?? false,
    }
  } catch {
    return { candles: [], hasRealData: false, hasLivePrices: false }
  }
}

export interface ReferralData {
  address: string
  referrer: string | null
  referred_count: number
  total_earnings_usdc: string
  referred_users: string[]
}

export interface LeaderboardTrader {
  address: string
  volume_usdc: string
  fill_count: number
  maker_count: number
  taker_count: number
}

export interface LeaderboardReferrer {
  address: string
  referred_count: number
  earnings_usdc: string
}

export interface LeaderboardData {
  top_traders: LeaderboardTrader[]
  top_referrers: LeaderboardReferrer[]
  period: string
}

export function getReferral(address: string): Promise<ApiResponse<ReferralData>> {
  return apiFetch(`/referral/${encodeURIComponent(address)}`)
}

export function registerReferral(body: {
  user: string
  ref: string
  signature: string
  nonce: number
}): Promise<ApiResponse<{ registered: boolean }>> {
  return apiFetch('/referral/register', {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export function getLeaderboard(): Promise<ApiResponse<LeaderboardData>> {
  return apiFetch('/leaderboard')
}

/** POST /deposit */
export interface DemoDepositData {
  credit_type: 'demo_unverified'
  verified_onchain: false
  balances: BalanceResponse[]
}

export async function deposit(
  user: string,
  asset: string,
  amount: string,
): Promise<ApiResponse<DemoDepositData>> {
  const apiUrl = process.env.NEXT_PUBLIC_API_URL ?? 'https://vela-engine.fly.dev'
  try {
    const res = await fetch(`${apiUrl}/deposit`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ user, asset, amount }),
      cache: 'no-store',
    })
    return res.json()
  } catch {
    return { ok: false, error: 'Network error' }
  }
}

// ---------------------------------------------------------------------------
// Portfolio / PnL
// ---------------------------------------------------------------------------

// Sourced from the OpenAPI spec so the field set stays honest — if the
// backend adds a field, tsc yells here first.
export type PortfolioLot = ApiSchemas['PortfolioLot']
export type PortfolioResponse = ApiSchemas['PortfolioResponse']

/** GET /portfolio/:address */
export function getPortfolio(
  address: string,
  method: 'FIFO' | 'HIFO' = 'FIFO',
): Promise<ApiResponse<PortfolioResponse>> {
  return apiFetch(
    `/portfolio/${encodeURIComponent(address)}?method=${method.toLowerCase()}`,
  )
}

/** GET /portfolio/:address/csv (returns raw text — not JSON). */
export async function getPortfolioCsv(
  address: string,
  method: 'FIFO' | 'HIFO' = 'FIFO',
): Promise<{ ok: boolean; csv?: string; error?: string }> {
  try {
    const headers = new Headers()
    if (authSessionToken) headers.set('Authorization', `Bearer ${authSessionToken}`)
    const res = await fetch(
      `${API_URL}/portfolio/${encodeURIComponent(address)}/csv?method=${method.toLowerCase()}`,
      { cache: 'no-store', headers },
    )
    if (res.status === 401 && authSessionToken) {
      authSessionToken = null
      if (typeof window !== 'undefined') {
        window.dispatchEvent(new Event('vela:auth-expired'))
      }
    }
    if (!res.ok) {
      const body = await res.text()
      let message = body
      try {
        const parsed: unknown = JSON.parse(body)
        if (parsed && typeof parsed === 'object' && 'error' in parsed && typeof parsed.error === 'string') {
          message = parsed.error
        }
      } catch {
        // Preserve plain-text API errors.
      }
      return { ok: false, error: message || res.statusText || 'CSV export failed' }
    }
    const csv = await res.text()
    return { ok: true, csv }
  } catch (e) {
    return { ok: false, error: String(e) }
  }
}

// ---------------------------------------------------------------------------
// Analytics / batches / proofs / TEE / decisions
// ---------------------------------------------------------------------------

export interface AnalyticsSummary {
  timeframe: string
  markets: {
    market: string
    volume_micro_usdc: string
    trades: number
    spread_bps: number
    depth_micro_usdc: string
    slippage_bps: number
  }[]
}

export function getAnalytics(
  timeframe: '5m' | '1h' | '24h' = '1h',
): Promise<ApiResponse<AnalyticsSummary>> {
  return apiFetch(`/analytics?timeframe=${timeframe}`)
}

export type ProofStats = ApiSchemas['ProofStats']

export function getProofStats(): Promise<ApiResponse<ProofStats>> {
  return apiFetch('/proofs/stats')
}

export function getProofs(limit = 50): Promise<ApiResponse<unknown[]>> {
  return apiFetch(`/proofs?limit=${limit}`)
}

export type TeeStats = ApiSchemas['TeeStats']

export function getTeeStats(): Promise<ApiResponse<TeeStats>> {
  return apiFetch('/tee/stats')
}

export function getAttestations(limit = 50): Promise<ApiResponse<unknown[]>> {
  return apiFetch(`/attestations?limit=${limit}`)
}

export interface DecisionSummary {
  id: number
  title: string
  body: string
  status: 'PENDING' | 'ENACTED' | 'REJECTED'
  proposed_at: number
  enacted_at?: number
}

export function getDecisions(): Promise<ApiResponse<DecisionSummary[]>> {
  return apiFetch('/decisions')
}

// ---------------------------------------------------------------------------
// Perps
// ---------------------------------------------------------------------------

export type PerpMarket = ApiSchemas['PerpMarket']
export type PerpPosition = ApiSchemas['PerpPosition']

export function getPerpMarkets(): Promise<ApiResponse<PerpMarket[]>> {
  return apiFetch('/perp/markets')
}

export function getPerpAccount(
  address: string,
): Promise<ApiResponse<{ user: string; positions: PerpPosition[] }>> {
  return apiFetch(`/perp/account/${encodeURIComponent(address)}`)
}

export type PerpLiquidationCandidate = ApiSchemas['PerpLiquidationCandidate']

export function getLiquidatablePositions(): Promise<
  ApiResponse<PerpLiquidationCandidate[]>
> {
  return apiFetch('/perp/liquidatable')
}

// ---------------------------------------------------------------------------
// Algos (TWAP) / RFQ / sub-accounts / borrow-lend / vaults
// ---------------------------------------------------------------------------

export interface AlgoStatus {
  parent_id: string
  market: string
  side: 'buy' | 'sell'
  total_quantity: number
  filled_quantity: number
  status: 'active' | 'complete' | 'canceled'
  slices: { at: number; quantity: number; status: string }[]
}

export function getAlgoStatus(parentId: string): Promise<ApiResponse<AlgoStatus>> {
  return apiFetch(`/orders/algo/${encodeURIComponent(parentId)}`)
}

export interface RfqRequest {
  id: string
  market: string
  side: 'buy' | 'sell'
  size_micro: number
  requester: string
  created_at: number
  expires_at: number
}

export interface RfqQuote {
  id: string
  rfq_id: string
  market: string
  side: 'buy' | 'sell'
  size_micro: number
  price_micro_usdc: number
  maker: string
  expires_at: number
}

/** GET /rfq/requests — all open RFQ requests. */
export function listRfqRequests(): Promise<ApiResponse<RfqRequest[]>> {
  return apiFetch('/rfq/requests')
}

/** GET /rfq/quotes/:rfq_id — quotes offered for a specific RFQ. */
export function getRfqQuotes(rfqId: string): Promise<ApiResponse<RfqQuote[]>> {
  return apiFetch(`/rfq/quotes/${encodeURIComponent(rfqId)}`)
}

export type BorrowLendMarket = ApiSchemas['BorrowLendMarket']

export function getBorrowLendMarkets(): Promise<ApiResponse<BorrowLendMarket[]>> {
  return apiFetch('/borrow-lend/markets')
}

export type BorrowLendAccount = ApiSchemas['BorrowLendAccount']

export function getBorrowLendAccount(
  address: string,
): Promise<ApiResponse<BorrowLendAccount>> {
  return apiFetch(`/borrow-lend/account/${encodeURIComponent(address)}`)
}

export interface VaultSummary {
  vault_id: string
  operator: string
  total_shares: string
  nav_micro_usdc: string
  drawdown_bps: number
}

export function listVaults(): Promise<ApiResponse<VaultSummary[]>> {
  return apiFetch('/vaults')
}

// ---------------------------------------------------------------------------
// Agent-tier badge
// ---------------------------------------------------------------------------

export interface AgentTier {
  address: string
  tier: 'green' | 'amber' | 'red'
  score: number
  cleared_until_ms?: number
}

export function getAgentTier(address: string): Promise<ApiResponse<AgentTier>> {
  return apiFetch(`/agents/${encodeURIComponent(address)}/tier`)
}

export interface ReputationScore {
  address: string
  fill_quality: number
  toxicity_avg: number
  uptime_bps: number
  score: number
}

export function getReputation(
  address: string,
): Promise<ApiResponse<ReputationScore>> {
  return apiFetch(`/reputation/${encodeURIComponent(address)}`)
}
