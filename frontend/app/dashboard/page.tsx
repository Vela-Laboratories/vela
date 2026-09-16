'use client'

import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from 'react'
import {
  getBalances,
  getOrders,
  cancelOrder,
  getReferral,
  registerReferral,
  type BalanceResponse,
  type Order,
  type ReferralData,
} from '@/lib/api'
import { signCancel } from '@/lib/signing'
import { velaWs, getWsClient } from '@/lib/ws'
import { useAuth } from '@/lib/auth'
import { Card } from '@/components/ui/Card'
import { Badge } from '@/components/ui/Badge'
import { Button } from '@/components/ui/Button'
import { FullPageSpinner } from '@/components/ui/Spinner'

interface FillEntry {
  key: string
  market: string
  ourSide: 'buy' | 'sell'
  price: string
  qty: string
  fee: string
  pnl: number
  ts: number
}

interface DashState {
  orders: Order[]
  fills: FillEntry[]
  balances: BalanceResponse[]
  cancelingIds: number[]
  cancelSnapshot: Record<number, Order>
  loadingInitial: boolean
}

type DashAction =
  | { type: 'INIT'; orders: Order[]; balances: BalanceResponse[] }
  | { type: 'SET_LOADING'; loading: boolean }
  | { type: 'ORDER_UPDATE'; orderId: number; status: string; filledQty: string }
  | {
      type: 'FILL'
      makerOrderId: number
      takerOrderId: number
      price: string
      qty: string
      takerSide: string
      makerFee: string
      takerFee: string
      ts: number
    }
  | { type: 'BALANCE_UPDATE'; asset: string; available: string; locked: string }
  | { type: 'CANCEL_OPTIMISTIC'; orderId: number }
  | { type: 'CANCEL_ROLLBACK'; order: Order }
  | { type: 'CANCEL_DONE'; orderId: number }

const initState: DashState = {
  orders: [],
  fills: [],
  balances: [],
  cancelingIds: [],
  cancelSnapshot: {},
  loadingInitial: false,
}

function omitKey(map: Record<number, Order>, id: number): Record<number, Order> {
  const next: Record<number, Order> = {}
  for (const [k, v] of Object.entries(map)) {
    if (Number(k) !== id) next[Number(k)] = v
  }
  return next
}

function dashReducer(state: DashState, action: DashAction): DashState {
  switch (action.type) {
    case 'INIT':
      return {
        ...state,
        orders: action.orders,
        balances: action.balances,
        loadingInitial: false,
      }

    case 'SET_LOADING':
      return { ...state, loadingInitial: action.loading }

    case 'ORDER_UPDATE': {
      return {
        ...state,
        orders: state.orders.map((o) =>
          o.id === action.orderId
            ? { ...o, status: action.status, filled_quantity: action.filledQty }
            : o,
        ),
      }
    }

    case 'FILL': {
      const makerOrder = state.orders.find((o) => o.id === action.makerOrderId)
      const takerOrder = state.orders.find((o) => o.id === action.takerOrderId)

      let market: string | null = null
      let ourSide: 'buy' | 'sell' | null = null
      let fee = '0'

      if (makerOrder) {
        market = makerOrder.market
        ourSide = action.takerSide === 'buy' ? 'sell' : 'buy'
        fee = action.makerFee
      } else if (takerOrder) {
        market = takerOrder.market
        ourSide = action.takerSide as 'buy' | 'sell'
        fee = action.takerFee
      }

      if (!market || !ourSide) return state

      const price = parseFloat(action.price)
      const qty = parseFloat(action.qty)
      const feeAmt = parseFloat(fee)
      const pnl =
        ourSide === 'sell'
          ? price * qty - feeAmt
          : -(price * qty) - feeAmt

      const entry: FillEntry = {
        key: `${action.ts}-${action.makerOrderId}-${action.takerOrderId}`,
        market,
        ourSide,
        price: action.price,
        qty: action.qty,
        fee,
        pnl,
        ts: action.ts,
      }

      return { ...state, fills: [entry, ...state.fills].slice(0, 500) }
    }

    case 'BALANCE_UPDATE': {
      const prev = state.balances.filter((b) => b.asset !== action.asset)
      return {
        ...state,
        balances: [
          ...prev,
          {
            asset: action.asset,
            available: action.available,
            locked: action.locked,
            total: (
              parseFloat(action.available) + parseFloat(action.locked)
            ).toFixed(8),
          },
        ],
      }
    }

    case 'CANCEL_OPTIMISTIC': {
      const order = state.orders.find((o) => o.id === action.orderId)
      if (!order) return state
      return {
        ...state,
        orders: state.orders.filter((o) => o.id !== action.orderId),
        cancelingIds: [...state.cancelingIds, action.orderId],
        cancelSnapshot: { ...state.cancelSnapshot, [action.orderId]: order },
      }
    }

    case 'CANCEL_ROLLBACK': {
      return {
        ...state,
        orders: [...state.orders, action.order].sort((a, b) => a.id - b.id),
        cancelingIds: state.cancelingIds.filter((id) => id !== action.order.id),
        cancelSnapshot: omitKey(state.cancelSnapshot, action.order.id),
      }
    }

    case 'CANCEL_DONE': {
      return {
        ...state,
        cancelingIds: state.cancelingIds.filter((id) => id !== action.orderId),
        cancelSnapshot: omitKey(state.cancelSnapshot, action.orderId),
      }
    }

    default:
      return state
  }
}

function fmtN(n: string | number, dec = 2): string {
  const v = typeof n === 'string' ? parseFloat(n) : n
  return isNaN(v) ? '—' : v.toFixed(dec)
}

function fmtPnl(pnl: number): string {
  return `${pnl >= 0 ? '+' : ''}${pnl.toFixed(4)}`
}

function ConnectGate({ onConnect }: { onConnect: () => Promise<void> }) {
  const [connecting, setConnecting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function handleConnect() {
    setConnecting(true)
    setError(null)
    try {
      await onConnect()
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to connect wallet')
    } finally {
      setConnecting(false)
    }
  }

  return (
    <div className="min-h-[60vh] flex items-center justify-center px-4">
      <div className="w-full max-w-sm text-center bg-canvas border border-border p-8">
        <div className="w-16 h-16 bg-[rgba(107,138,90,0.1)] flex items-center justify-center mx-auto mb-5">
          <svg width="28" height="28" viewBox="0 0 24 24" fill="none">
            <path
              d="M21 7H3C2.44772 7 2 7.44772 2 8V20C2 20.5523 2.44772 21 3 21H21C21.5523 21 22 20.5523 22 20V8C22 7.44772 21.5523 7 21 7Z"
              stroke="#20c98a"
              strokeWidth="1.5"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
            <circle cx="14" cy="14" r="2" fill="#20c98a" />
            <path
              d="M17 7V5C17 3.89543 16.1046 3 15 3H9C7.89543 3 7 3.89543 7 5V7"
              stroke="#20c98a"
              strokeWidth="1.5"
              strokeLinecap="round"
            />
          </svg>
        </div>
        <h2 className="text-xl font-semibold text-ink mb-2">
          Connect your wallet
        </h2>
        <p className="text-sm text-brown mb-6 leading-relaxed">
          Connect a wallet to view balances, open orders, credit utilization,
          and real-time P&amp;L.
        </p>
        {error && (
          <p className="text-xs text-terra mb-4 bg-terra/10 px-3 py-2">
            {error}
          </p>
        )}
        <Button size="lg" className="w-full" loading={connecting} onClick={handleConnect}>
          Connect Wallet
        </Button>
      </div>
    </div>
  )
}

function CreditGauge({
  utilPct,
  quotedValue,
  depositedValue,
}: {
  utilPct: number
  quotedValue: number
  depositedValue: number
}) {
  const R = 38
  const circumference = 2 * Math.PI * R
  const clamped = Math.min(100, Math.max(0, utilPct))
  const dashOffset = circumference - (clamped / 100) * circumference
  const isDanger = clamped >= 95
  const isWarning = clamped >= 80
  const arcColor = isDanger ? '#ef6a5b' : '#20c98a'
  const textFill = '#e9eaf0'

  return (
    <div className="flex flex-col items-center gap-5">
      <svg width="128" height="128" viewBox="0 0 100 100">
        <circle cx="50" cy="50" r={R} stroke="rgba(232,234,240,0.06)" strokeWidth="10" fill="none" />
        <circle
          cx="50"
          cy="50"
          r={R}
          stroke={arcColor}
          strokeWidth="10"
          fill="none"
          strokeDasharray={circumference}
          strokeDashoffset={dashOffset}
          strokeLinecap="round"
          transform="rotate(-90 50 50)"
          style={{ transition: 'stroke-dashoffset 0.5s ease, stroke 0.3s ease' }}
        />
        <text
          x="50"
          y="46"
          textAnchor="middle"
          fontSize="15"
          fontWeight="700"
          fontFamily="Courier New, monospace"
          fill={textFill}
        >
          {clamped.toFixed(0)}%
        </text>
        <text
          x="50"
          y="61"
          textAnchor="middle"
          fontSize="8"
          fill="rgba(232,234,240,0.25)"
          fontFamily="Courier New, monospace"
        >
          utilized
        </text>
      </svg>

      <div className="w-full space-y-2 text-sm">
        <div className="flex items-center justify-between">
          <span className="text-brown uppercase tracking-[0.1em] text-xs">Quoted</span>
          <span className="tabular-nums font-medium text-ink font-mono">
            {quotedValue.toFixed(2)}
          </span>
        </div>
        <div className="flex items-center justify-between">
          <span className="text-brown uppercase tracking-[0.1em] text-xs">Deposited</span>
          <span className="tabular-nums font-medium text-ink font-mono">
            {depositedValue.toFixed(2)}
          </span>
        </div>
        {isWarning && (
          <div
            className={[
              'mt-1 px-3 py-2 text-xs font-medium border-l-[3px]',
              isDanger
                ? 'bg-terra/10 border-terra text-terra'
                : 'bg-ochre/10 border-ochre text-ochre',
            ].join(' ')}
          >
            {isDanger
              ? 'Credit critical — reduce exposure'
              : 'High utilization — consider rebalancing'}
          </div>
        )}
      </div>
    </div>
  )
}

function StatusBadge({ status }: { status: string }) {
  const variantMap: Record<
    string,
    'primary' | 'success' | 'neutral' | 'warning' | 'error'
  > = {
    open:     'primary',
    partial:  'warning',
    filled:   'success',
    canceled: 'neutral',
    rejected: 'error',
  }
  return (
    <Badge variant={variantMap[status] ?? 'neutral'} dot size="sm">
      {status}
    </Badge>
  )
}

function OpenOrdersTable({
  orders,
  cancelingIds,
  onCancel,
}: {
  orders: Order[]
  cancelingIds: number[]
  onCancel: (order: Order) => void
}) {
  const active = useMemo(
    () => orders.filter((o) => o.status === 'open' || o.status === 'partial'),
    [orders],
  )

  return (
    <Card padding="none">
      <div className="px-6 py-4 border-b border-border flex items-center justify-between">
        <span className="text-[0.65rem] uppercase tracking-[0.15em] text-brown font-medium">Open Orders</span>
        <Badge variant={active.length > 0 ? 'primary' : 'neutral'}>{active.length}</Badge>
      </div>

      {active.length === 0 ? (
        <p className="px-6 py-10 text-center text-sm text-brown">No open orders</p>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="text-left border-b border-border bg-canvas">
                <th className="px-4 py-3 text-[0.65rem] font-medium text-brown uppercase tracking-[0.12em]">Market</th>
                <th className="px-4 py-3 text-[0.65rem] font-medium text-brown uppercase tracking-[0.12em]">Side</th>
                <th className="px-4 py-3 text-right text-[0.65rem] font-medium text-brown uppercase tracking-[0.12em]">Price</th>
                <th className="px-4 py-3 text-right text-[0.65rem] font-medium text-brown uppercase tracking-[0.12em]">Size</th>
                <th className="px-4 py-3 text-right text-[0.65rem] font-medium text-brown uppercase tracking-[0.12em]">Filled</th>
                <th className="px-4 py-3 text-[0.65rem] font-medium text-brown uppercase tracking-[0.12em]">TIF</th>
                <th className="px-4 py-3 text-[0.65rem] font-medium text-brown uppercase tracking-[0.12em]">Status</th>
                <th className="px-4 py-3" />
              </tr>
            </thead>
            <tbody>
              {active.map((order) => {
                const isCanceling = cancelingIds.includes(order.id)
                const fillPct =
                  parseFloat(order.quantity) > 0
                    ? (parseFloat(order.filled_quantity) /
                        parseFloat(order.quantity)) *
                      100
                    : 0

                return (
                  <tr
                    key={order.id}
                    className={[
                      'transition-opacity duration-150 border-b border-border last:border-0',
                      isCanceling ? 'opacity-35' : 'hover:bg-[rgba(232,234,240,0.04)]',
                    ].join(' ')}
                  >
                    <td className="px-4 py-3">
                      <span
                        className={[
                          'font-medium text-ink text-xs border-l-2 pl-2',
                          order.side === 'buy' ? 'border-sage' : 'border-terra',
                        ].join(' ')}
                      >
                        {order.market}
                      </span>
                    </td>
                    <td className="px-4 py-3">
                      <Badge
                        variant={order.side === 'buy' ? 'success' : 'error'}
                        size="sm"
                      >
                        {order.side}
                      </Badge>
                    </td>
                    <td className="px-4 py-3 text-right tabular-nums text-xs text-ink font-mono">
                      {fmtN(order.price, 4)}
                    </td>
                    <td className="px-4 py-3 text-right tabular-nums text-xs text-ink font-mono">
                      {fmtN(order.quantity, 4)}
                    </td>
                    <td className="px-4 py-3 text-right">
                      <div className="flex items-center justify-end gap-2">
                        <div className="w-14 h-1 bg-vellum overflow-hidden">
                          <div
                            className="h-full bg-sage transition-[width] duration-300"
                            style={{ width: `${fillPct}%` }}
                          />
                        </div>
                        <span className="text-[11px] tabular-nums text-brown w-7 text-right font-mono">
                          {fillPct.toFixed(0)}%
                        </span>
                      </div>
                    </td>
                    <td className="px-4 py-3">
                      <span className="text-[11px] text-brown uppercase font-medium tracking-[0.08em]">
                        GTC
                      </span>
                    </td>
                    <td className="px-4 py-3">
                      <StatusBadge status={order.status} />
                    </td>
                    <td className="px-4 py-3 text-right">
                      <Button
                        variant="ghost"
                        size="sm"
                        loading={isCanceling}
                        disabled={isCanceling}
                        onClick={() => onCancel(order)}
                        className="text-terra hover:bg-terra/10 text-xs"
                      >
                        Cancel
                      </Button>
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        </div>
      )}
    </Card>
  )
}

function PnlTracker({
  fills,
  pnlByMarket,
  totalPnl,
}: {
  fills: FillEntry[]
  pnlByMarket: Record<string, number>
  totalPnl: number
}) {
  const [expanded, setExpanded] = useState(false)
  const markets = Object.keys(pnlByMarket).sort()

  return (
    <Card padding="none">
      <button
        type="button"
        className="w-full px-6 py-4 border-b border-border flex items-center justify-between hover:bg-[rgba(232,234,240,0.03)] transition-colors"
        onClick={() => setExpanded((e) => !e)}
      >
        <div className="flex items-center gap-3">
          <span className="text-[0.65rem] uppercase tracking-[0.15em] text-brown font-medium">Realized P&amp;L</span>
          <span
            className={[
              'text-xl font-bold tabular-nums font-mono',
              totalPnl >= 0 ? 'text-sage' : 'text-terra',
            ].join(' ')}
          >
            {fmtPnl(totalPnl)}
          </span>
        </div>
        <div className="flex items-center gap-2">
          <Badge variant="neutral" size="sm">
            {fills.length} fills
          </Badge>
          <svg
            width="16"
            height="16"
            viewBox="0 0 16 16"
            fill="none"
            className={[
              'text-brown transition-transform duration-200 shrink-0',
              expanded ? 'rotate-180' : '',
            ].join(' ')}
          >
            <path
              d="M4 6l4 4 4-4"
              stroke="currentColor"
              strokeWidth="1.5"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
        </div>
      </button>

      {expanded && (
        <div>
          {markets.length === 0 ? (
            <p className="px-6 py-8 text-sm text-brown text-center">
              No fills yet
            </p>
          ) : (
            <>
              <div className="grid grid-cols-3 px-6 py-2 text-[0.65rem] font-medium text-brown uppercase tracking-[0.12em] border-b border-border">
                <span>Market</span>
                <span className="text-right">Fills</span>
                <span className="text-right">P&amp;L</span>
              </div>
              {markets.map((mkt) => {
                const mktPnl = pnlByMarket[mkt] ?? 0
                const mktFills = fills.filter((f) => f.market === mkt).length
                return (
                  <div
                    key={mkt}
                    className="grid grid-cols-3 px-6 py-3 text-sm hover:bg-[rgba(232,234,240,0.03)] transition-colors border-b border-border last:border-0"
                  >
                    <span className="font-medium text-ink">{mkt}</span>
                    <span className="text-right tabular-nums text-brown font-mono">
                      {mktFills}
                    </span>
                    <span
                      className={[
                        'text-right tabular-nums font-semibold font-mono',
                        mktPnl >= 0 ? 'text-sage' : 'text-terra',
                      ].join(' ')}
                    >
                      {fmtPnl(mktPnl)}
                    </span>
                  </div>
                )
              })}
            </>
          )}
        </div>
      )}
    </Card>
  )
}

interface MarketSummaryRow {
  market: string
  bidCount: number
  askCount: number
  bestBid: string | null
  bestAsk: string | null
  spread: string | null
  quotedValue: number
}

function MarketSummaryTable({ summaries }: { summaries: MarketSummaryRow[] }) {
  return (
    <Card padding="none">
      <div className="px-6 py-4 border-b border-border">
        <span className="text-[0.65rem] uppercase tracking-[0.15em] text-brown font-medium">Per-Market Quotes</span>
      </div>
      {summaries.length === 0 ? (
        <p className="px-6 py-10 text-sm text-brown text-center">
          No active quotes
        </p>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="text-left border-b border-border bg-canvas">
                <th className="px-4 py-3 text-[0.65rem] font-medium text-brown uppercase tracking-[0.12em]">Market</th>
                <th className="px-4 py-3 text-right text-[0.65rem] font-medium text-brown uppercase tracking-[0.12em]">Bid</th>
                <th className="px-4 py-3 text-right text-[0.65rem] font-medium text-brown uppercase tracking-[0.12em]">Ask</th>
                <th className="px-4 py-3 text-right text-[0.65rem] font-medium text-brown uppercase tracking-[0.12em]">Spread</th>
                <th className="px-4 py-3 text-right text-[0.65rem] font-medium text-brown uppercase tracking-[0.12em]"># Orders</th>
                <th className="px-4 py-3 text-right text-[0.65rem] font-medium text-brown uppercase tracking-[0.12em]">Quoted Value</th>
              </tr>
            </thead>
            <tbody>
              {summaries.map((row) => (
                <tr
                  key={row.market}
                  className="hover:bg-[rgba(232,234,240,0.04)] transition-colors duration-100 border-b border-border last:border-0"
                >
                  <td className="px-4 py-3 font-medium text-ink">
                    {row.market}
                  </td>
                  <td className="px-4 py-3 text-right tabular-nums text-sage text-xs font-mono">
                    {row.bestBid ?? '—'}
                  </td>
                  <td className="px-4 py-3 text-right tabular-nums text-terra text-xs font-mono">
                    {row.bestAsk ?? '—'}
                  </td>
                  <td className="px-4 py-3 text-right tabular-nums text-brown text-xs font-mono">
                    {row.spread ?? '—'}
                  </td>
                  <td className="px-4 py-3 text-right tabular-nums text-xs text-ink font-mono">
                    {row.bidCount + row.askCount}
                    <span className="ml-1 text-brown">
                      ({row.bidCount}b / {row.askCount}a)
                    </span>
                  </td>
                  <td className="px-4 py-3 text-right tabular-nums font-medium text-ink text-xs font-mono">
                    {row.quotedValue.toFixed(2)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </Card>
  )
}

type AccountData = {
  type: string
  asset?: string
  available?: string
  locked?: string
  order_id?: number
  status?: string
  filled_quantity?: string
  maker_order_id?: number
  taker_order_id?: number
  price?: string
  quantity?: string
  side?: string
  maker_fee?: string
  taker_fee?: string
  timestamp?: number
  balances?: { asset: string; available: string; locked: string; total: string }[]
  orders?: unknown[]
}

function truncateAddr(addr: string): string {
  return `${addr.slice(0, 6)}…${addr.slice(-4)}`
}

function ReferralSection({ address }: { address: string }) {
  const [referral, setReferral] = useState<ReferralData | null>(null)
  const [copied, setCopied] = useState(false)
  const [refInput, setRefInput] = useState('')
  const [applying, setApplying] = useState(false)
  const [applyError, setApplyError] = useState<string | null>(null)
  const fullLink = `https://vela.monolithsystematic.com?ref=${address}`

  useEffect(() => {
    getReferral(address).then((res) => {
      if (res.ok && res.data) setReferral(res.data)
    })
  }, [address])

  function handleCopy() {
    navigator.clipboard.writeText(fullLink).then(() => {
      setCopied(true)
      setTimeout(() => setCopied(false), 2000)
    })
  }

  async function handleApply() {
    if (!refInput.trim()) return
    setApplying(true)
    setApplyError(null)
    try {
      const nonce = Date.now()
      const msg = `vela:referral:${address.toLowerCase()}:${refInput.toLowerCase()}:${nonce}`
      const signature = (await window.ethereum!.request({
        method: 'personal_sign',
        params: [msg, address],
      })) as string
      const res = await registerReferral({ user: address, ref: refInput, signature, nonce })
      if (res.ok) {
        const updated = await getReferral(address)
        if (updated.ok && updated.data) setReferral(updated.data)
      } else {
        setApplyError(res.error ?? 'Failed to apply referral')
      }
    } catch {
      setApplyError('Failed to apply referral')
    } finally {
      setApplying(false)
    }
  }

  return (
    <Card padding="none">
      <div className="px-6 py-4 border-b border-border">
        <span className="text-[0.65rem] uppercase tracking-[0.15em] text-brown font-medium">Referral Program</span>
      </div>
      <div className="px-6 py-5 space-y-5">
        <div>
          <div className="text-[10px] text-brown uppercase tracking-[0.1em] mb-2">Your referral link</div>
          <div className="flex items-center gap-3">
            <span className="font-mono text-xs text-ink bg-vellum px-3 py-2 flex-1 truncate">
              {`vela.monolithsystematic.com?ref=${truncateAddr(address)}`}
            </span>
            <button
              type="button"
              onClick={handleCopy}
              style={{ borderRadius: 0 }}
              className="text-[11px] uppercase tracking-[0.1em] border border-border px-4 py-2 text-brown hover:text-ink hover:border-ink transition-colors font-medium shrink-0"
            >
              {copied ? 'COPIED!' : 'COPY LINK'}
            </button>
          </div>
        </div>

        <div className="grid grid-cols-2 gap-4">
          <div>
            <div className="text-[10px] text-brown uppercase tracking-[0.1em] mb-1">Referred Users</div>
            <div className="text-lg font-bold text-ink font-mono tabular-nums">
              {referral?.referred_count ?? 0}
            </div>
          </div>
          <div>
            <div className="text-[10px] text-brown uppercase tracking-[0.1em] mb-1">Total Earned</div>
            <div className="text-lg font-bold text-ink font-mono tabular-nums">
              {referral ? `${parseFloat(referral.total_earnings_usdc).toFixed(6)} USDC` : '0.000000 USDC'}
            </div>
          </div>
        </div>

        {referral && referral.referred_count === 0 && (
          <p className="text-xs text-brown leading-relaxed">
            Share your link to earn 20% of your referrals&apos; taker fees for 90 days.
          </p>
        )}

        <div className="pt-2 border-t border-border">
          <div className="text-[10px] text-brown uppercase tracking-[0.1em] mb-2">Your Referrer</div>
          {referral?.referrer ? (
            <span className="font-mono text-xs text-ink">
              Referred by: {truncateAddr(referral.referrer)}
            </span>
          ) : (
            <div className="flex items-center gap-2">
              <input
                type="text"
                value={refInput}
                onChange={(e) => setRefInput(e.target.value)}
                placeholder="Enter referral address (0x...)"
                style={{ borderRadius: 0 }}
                className="flex-1 text-xs font-mono bg-transparent border border-border px-3 py-2 text-ink placeholder:text-brown focus:outline-none focus:border-ink"
              />
              <button
                type="button"
                onClick={handleApply}
                disabled={applying || !refInput.trim()}
                style={{ borderRadius: 0 }}
                className="text-[11px] uppercase tracking-[0.1em] border border-border px-4 py-2 text-brown hover:text-ink hover:border-ink transition-colors font-medium shrink-0 disabled:opacity-40"
              >
                {applying ? '…' : 'APPLY'}
              </button>
            </div>
          )}
          {applyError && (
            <p className="text-xs text-terra mt-2">{applyError}</p>
          )}
        </div>
      </div>
    </Card>
  )
}

export default function DashboardPage() {
  const { address, isConnected, isAuthenticated, connect, signIn } = useAuth()
  const [state, dispatch] = useReducer(dashReducer, initState)
  const wsRef = useRef(getWsClient())

  const handleConnect = useCallback(async () => {
    const addr = await connect()
    const ws = wsRef.current
    ws.connect()
    try {
      await signIn(ws, addr)
    } catch {
      // Private HTTP reads and account streaming remain unavailable.
    }
  }, [connect, signIn])

  useEffect(() => {
    if (!address || !isAuthenticated) return
    dispatch({ type: 'SET_LOADING', loading: true })
    Promise.all([getBalances(address), getOrders(address)])
      .then(([balRes, ordRes]) => {
        dispatch({
          type: 'INIT',
          orders: balRes.ok && ordRes.ok && ordRes.data ? ordRes.data : [],
          balances: balRes.ok && balRes.data ? balRes.data : [],
        })
      })
      .catch(() => dispatch({ type: 'SET_LOADING', loading: false }))
  }, [address, isAuthenticated])

  useEffect(() => {
    if (!address) return
    velaWs.connect()

    const unsub = velaWs.subscribe(`account:${address}`, (raw) => {
      const data = raw as AccountData
      if (data.type === 'account_snapshot') {
        if (data.balances) {
          data.balances.forEach((b) => {
            dispatch({ type: 'BALANCE_UPDATE', asset: b.asset, available: b.available, locked: b.locked })
          })
        }
        return
      }
      if (data.type === 'balance_update' && data.asset) {
        dispatch({
          type: 'BALANCE_UPDATE',
          asset: data.asset,
          available: data.available ?? '0',
          locked: data.locked ?? '0',
        })
      }
      if (data.type === 'order_update' && data.order_id !== undefined) {
        dispatch({
          type: 'ORDER_UPDATE',
          orderId: data.order_id,
          status: data.status ?? '',
          filledQty: data.filled_quantity ?? '0',
        })
      }
      if (data.type === 'fill' && data.maker_order_id !== undefined) {
        dispatch({
          type: 'FILL',
          makerOrderId: data.maker_order_id,
          takerOrderId: data.taker_order_id ?? 0,
          price: data.price ?? '0',
          qty: data.quantity ?? '0',
          takerSide: data.side ?? 'bid',
          makerFee: data.maker_fee ?? '0',
          takerFee: data.taker_fee ?? '0',
          ts: data.timestamp ?? 0,
        })
      }
    })

    const unsubLegacy = wsRef.current.onMessage((msg) => {
      if (msg.type === 'balance_update') {
        dispatch({ type: 'BALANCE_UPDATE', asset: msg.asset, available: msg.available, locked: msg.locked })
      }
      if (msg.type === 'order_update') {
        dispatch({ type: 'ORDER_UPDATE', orderId: msg.order_id, status: msg.status, filledQty: msg.filled_quantity })
      }
      if (msg.type === 'fill') {
        dispatch({ type: 'FILL', makerOrderId: msg.maker_order_id, takerOrderId: msg.taker_order_id, price: msg.price, qty: msg.quantity, takerSide: msg.side, makerFee: msg.maker_fee, takerFee: msg.taker_fee, ts: msg.timestamp })
      }
    })

    return () => { unsub(); unsubLegacy() }
  }, [address])

  const handleCancel = useCallback(
    async (order: Order) => {
      if (!address) return
      dispatch({ type: 'CANCEL_OPTIMISTIC', orderId: order.id })
      try {
        const nonce = Date.now()
        const sig = await signCancel({ order_id: order.id, nonce, address })
        const res = await cancelOrder({ order_id: order.id, nonce, address, signature: sig })
        if (res.ok) {
          dispatch({ type: 'CANCEL_DONE', orderId: order.id })
        } else {
          dispatch({ type: 'CANCEL_ROLLBACK', order })
        }
      } catch {
        dispatch({ type: 'CANCEL_ROLLBACK', order })
      }
    },
    [address],
  )

  const openOrders = useMemo(
    () => state.orders.filter((o) => o.status === 'open' || o.status === 'partial'),
    [state.orders],
  )

  const depositedValue = useMemo(
    () =>
      state.balances.reduce(
        (sum, b) => sum + parseFloat(b.available) + parseFloat(b.locked),
        0,
      ),
    [state.balances],
  )

  const quotedValue = useMemo(
    () =>
      openOrders.reduce(
        (sum, o) => sum + parseFloat(o.price) * parseFloat(o.quantity),
        0,
      ),
    [openOrders],
  )

  const utilPct = useMemo(
    () =>
      depositedValue > 0
        ? Math.min(100, (quotedValue / depositedValue) * 100)
        : 0,
    [depositedValue, quotedValue],
  )

  const totalPnl = useMemo(
    () => state.fills.reduce((sum, f) => sum + f.pnl, 0),
    [state.fills],
  )

  const pnlByMarket = useMemo(() => {
    const map: Record<string, number> = {}
    for (const f of state.fills) {
      map[f.market] = (map[f.market] ?? 0) + f.pnl
    }
    return map
  }, [state.fills])

  const activeMarkets = useMemo(
    () => [...new Set(openOrders.map((o) => o.market))],
    [openOrders],
  )

  const marketSummaries = useMemo((): MarketSummaryRow[] => {
    return activeMarkets.map((market) => {
      const mktOrders = openOrders.filter((o) => o.market === market)
      const bids = mktOrders.filter((o) => o.side === 'buy')
      const asks = mktOrders.filter((o) => o.side === 'sell')

      const bestBid =
        bids.length > 0
          ? bids.reduce((best, o) =>
              parseFloat(o.price) > parseFloat(best.price) ? o : best,
            ).price
          : null

      const bestAsk =
        asks.length > 0
          ? asks.reduce((best, o) =>
              parseFloat(o.price) < parseFloat(best.price) ? o : best,
            ).price
          : null

      const spread =
        bestBid && bestAsk
          ? (parseFloat(bestAsk) - parseFloat(bestBid)).toFixed(4)
          : null

      const quotedVal = mktOrders.reduce(
        (sum, o) => sum + parseFloat(o.price) * parseFloat(o.quantity),
        0,
      )

      return {
        market,
        bidCount: bids.length,
        askCount: asks.length,
        bestBid: bestBid ? parseFloat(bestBid).toFixed(4) : null,
        bestAsk: bestAsk ? parseFloat(bestAsk).toFixed(4) : null,
        spread,
        quotedValue: quotedVal,
      }
    })
  }, [activeMarkets, openOrders])

  if (!isConnected || !isAuthenticated) {
    return <ConnectGate onConnect={handleConnect} />
  }

  if (state.loadingInitial) {
    return <FullPageSpinner />
  }

  const shortAddr = address
    ? `${address.slice(0, 6)}…${address.slice(-4)}`
    : ''

  return (
    <div className="max-w-7xl mx-auto px-4 sm:px-6 py-8">
      <div className="flex items-start justify-between mb-8">
        <div>
          <h1 className="text-2xl font-bold text-ink">Dashboard</h1>
          <p className="text-sm text-brown mt-1 font-mono">{shortAddr}</p>
        </div>
        <Badge variant={isAuthenticated ? 'success' : 'warning'} dot>
          {isAuthenticated ? 'Authenticated' : 'Wallet connected'}
        </Badge>
      </div>

      <div className="grid grid-cols-2 sm:grid-cols-4 gap-4 mb-8">
        {(
          [
            {
              label: 'Portfolio Value',
              value: depositedValue.toFixed(2),
              suffix: 'USD' as string | undefined,
              pnl: false,
              positive: false,
              fontPF: true,
            },
            {
              label: 'Open Orders',
              value: openOrders.length.toString(),
              suffix: undefined,
              pnl: false,
              positive: false,
              fontPF: false,
            },
            {
              label: 'Active Markets',
              value: activeMarkets.length.toString(),
              suffix: undefined,
              pnl: false,
              positive: false,
              fontPF: false,
            },
            {
              label: 'Realized P&L',
              value: fmtPnl(totalPnl),
              suffix: undefined,
              pnl: true,
              positive: totalPnl >= 0,
              fontPF: false,
            },
          ] satisfies {
            label: string
            value: string
            suffix: string | undefined
            pnl: boolean
            positive: boolean
            fontPF: boolean
          }[]
        ).map(({ label, value, suffix, pnl, positive, fontPF }) => (
          <Card key={label}>
            <div
              className={[
                'font-bold tabular-nums',
                fontPF ? 'text-[2rem]' : 'text-2xl font-mono',
                pnl ? (positive ? 'text-sage' : 'text-terra') : 'text-ink',
              ].join(' ')}
              style={fontPF ? { fontFamily: "var(--font-playfair), 'Playfair Display', serif", fontWeight: 700 } : undefined}
            >
              {value}
              {suffix && (
                <span className="text-sm font-normal text-brown ml-1">
                  {suffix}
                </span>
              )}
            </div>
            <div className="text-[10px] text-brown mt-1 uppercase tracking-[0.12em]">{label}</div>
          </Card>
        ))}
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-[288px_1fr] gap-6 mb-6">
        <Card>
          <div className="text-[0.65rem] font-medium text-brown mb-5 uppercase tracking-[0.15em]">
            Credit Utilization
          </div>
          <CreditGauge
            utilPct={utilPct}
            quotedValue={quotedValue}
            depositedValue={depositedValue}
          />
          <div className="mt-5 pt-4 border-t border-border">
            <div className="flex items-center justify-between mb-2">
              <span className="text-xs text-brown uppercase tracking-[0.1em]">Credit ratio</span>
              <Badge
                variant={
                  utilPct >= 95 ? 'error' : utilPct >= 80 ? 'warning' : 'success'
                }
                size="sm"
              >
                {utilPct.toFixed(1)}% / 100%
              </Badge>
            </div>
            <div className="h-1.5 bg-vellum overflow-hidden">
              <div
                className={[
                  'h-full transition-[width] duration-500',
                  utilPct >= 95
                    ? 'bg-terra'
                    : utilPct >= 80
                    ? 'bg-ochre'
                    : 'bg-sage',
                ].join(' ')}
                style={{ width: `${utilPct}%` }}
              />
            </div>
            <div className="flex justify-between text-[10px] text-brown mt-1.5">
              <span>0%</span>
              <span className="text-ochre font-medium">80% warn</span>
              <span>100%</span>
            </div>
          </div>
        </Card>

        <MarketSummaryTable summaries={marketSummaries} />
      </div>

      {address && (
        <div className="mb-6">
          <ReferralSection address={address} />
        </div>
      )}

      <div className="mb-6">
        <OpenOrdersTable
          orders={state.orders}
          cancelingIds={state.cancelingIds}
          onCancel={handleCancel}
        />
      </div>

      <PnlTracker
        fills={state.fills}
        pnlByMarket={pnlByMarket}
        totalPnl={totalPnl}
      />
    </div>
  )
}
