'use client'

import {
  useCallback,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
} from 'react'
import {
  getAccountFills,
  getOrders,
  listMarkets,
  type AccountFill,
  type MarketResponse,
  type Order,
} from '@/lib/api'
import { getWsClient } from '@/lib/ws'
import { useAuth } from '@/lib/auth'
import { Badge } from '@/components/ui/Badge'
import { Button } from '@/components/ui/Button'
import { Card } from '@/components/ui/Card'
import { Input } from '@/components/ui/Input'
import { Spinner, FullPageSpinner } from '@/components/ui/Spinner'

const PAGE_SIZE = 25
const MAX_PUBLIC_TRADES = 100

interface FillRecord {
  key: string
  ts: number
  market: string
  side: 'buy' | 'sell'
  price: string
  size: string
  fee: string
  role: 'maker' | 'taker' | 'unknown'
  source: 'historical' | 'live'
}

interface PublicTrade {
  key: string
  ts: number
  price: string
  size: string
  side: 'buy' | 'sell'
}

interface HistState {
  markets: MarketResponse[]
  historicalFills: FillRecord[]
  liveFills: FillRecord[]
  marketByOrderId: Record<number, string>
  loadingInitial: boolean
  visiblePage: number
  filterMarket: string
  filterDateFrom: string
  filterDateTo: string
  publicMarket: string | null
  publicTrades: PublicTrade[]
}

type HistAction =
  | { type: 'INIT_MARKETS'; markets: MarketResponse[] }
  | {
      type: 'INIT'
      fills: FillRecord[]
      marketByOrderId: Record<number, string>
    }
  | { type: 'SET_LOADING_INITIAL'; loading: boolean }
  | { type: 'PREPEND_LIVE'; fill: FillRecord }
  | { type: 'APPEND_HISTORICAL' }
  | {
      type: 'SET_FILTER'
      key: 'filterMarket' | 'filterDateFrom' | 'filterDateTo'
      value: string
    }
  | { type: 'SET_PUBLIC_MARKET'; market: string }
  | { type: 'ADD_PUBLIC_TRADE'; trade: PublicTrade }

const initState: HistState = {
  markets: [],
  historicalFills: [],
  liveFills: [],
  marketByOrderId: {},
  loadingInitial: false,
  visiblePage: 1,
  filterMarket: '',
  filterDateFrom: '',
  filterDateTo: '',
  publicMarket: null,
  publicTrades: [],
}

function histReducer(state: HistState, action: HistAction): HistState {
  switch (action.type) {
    case 'INIT_MARKETS':
      return {
        ...state,
        markets: action.markets,
        publicMarket: state.publicMarket ?? action.markets[0]?.id ?? null,
      }

    case 'SET_LOADING_INITIAL':
      return { ...state, loadingInitial: action.loading }

    case 'INIT':
      return {
        ...state,
        historicalFills: action.fills,
        marketByOrderId: action.marketByOrderId,
        loadingInitial: false,
        visiblePage: 1,
      }

    case 'PREPEND_LIVE':
      return {
        ...state,
        liveFills: [action.fill, ...state.liveFills].slice(0, 500),
      }

    case 'APPEND_HISTORICAL':
      return { ...state, visiblePage: state.visiblePage + 1 }

    case 'SET_FILTER':
      return { ...state, [action.key]: action.value, visiblePage: 1 }

    case 'SET_PUBLIC_MARKET':
      return { ...state, publicMarket: action.market, publicTrades: [] }

    case 'ADD_PUBLIC_TRADE':
      return {
        ...state,
        publicTrades: [action.trade, ...state.publicTrades].slice(
          0,
          MAX_PUBLIC_TRADES,
        ),
      }

    default:
      return state
  }
}

function accountFillToRecord(fill: AccountFill): FillRecord {
  return {
    key: `hist-${fill.id}`,
    ts: fill.timestamp,
    market: fill.market,
    side: fill.side,
    price: fill.price,
    size: fill.quantity,
    fee: '—',
    role: fill.role,
    source: 'historical',
  }
}

function buildOrderMaps(orders: Order[]): {
  marketByOrderId: Record<number, string>
} {
  const marketByOrderId: Record<number, string> = {}
  for (const o of orders) {
    marketByOrderId[o.id] = o.market
  }
  return { marketByOrderId }
}

function fmtTs(ts: number): string {
  return new Date(ts).toLocaleString('en-US', {
    month: 'short',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    hour12: false,
  })
}

function fmtTime(ts: number): string {
  return new Date(ts).toLocaleTimeString('en-US', {
    hour12: false,
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  })
}

function exportCSV(rows: FillRecord[], filename: string) {
  const headers = ['Time (UTC)', 'Market', 'Side', 'Price', 'Size', 'Fee', 'Role']
  const lines = rows.map((r) =>
    [
      new Date(r.ts).toISOString(),
      r.market,
      r.side,
      r.price,
      r.size,
      r.fee,
      r.role,
    ].join(','),
  )
  const csv = [headers.join(','), ...lines].join('\n')
  const blob = new Blob([csv], { type: 'text/csv;charset=utf-8;' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = filename
  document.body.appendChild(a)
  a.click()
  document.body.removeChild(a)
  URL.revokeObjectURL(url)
}

function ConnectGate({ onConnect }: { onConnect: () => Promise<void> }) {
  const [connecting, setConnecting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function handle() {
    setConnecting(true)
    setError(null)
    try {
      await onConnect()
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Connection failed')
    } finally {
      setConnecting(false)
    }
  }

  return (
    <Card className="text-center py-10">
      <div className="w-14 h-14 bg-[rgba(107,138,90,0.1)] flex items-center justify-center mx-auto mb-4">
        <svg width="24" height="24" viewBox="0 0 24 24" fill="none">
          <rect
            x="2" y="7" width="20" height="14"
            stroke="#6B8A5A" strokeWidth="1.5"
          />
          <circle cx="14" cy="14" r="2" fill="#6B8A5A" />
          <path
            d="M17 7V5a2 2 0 0 0-2-2H9a2 2 0 0 0-2 2v2"
            stroke="#6B8A5A" strokeWidth="1.5" strokeLinecap="round"
          />
        </svg>
      </div>
      <h3 className="text-lg font-semibold text-ink mb-2">
        Connect wallet to view trade history
      </h3>
      <p className="text-sm text-brown mb-6 max-w-xs mx-auto leading-relaxed">
        Trade history is private. Connect and authenticate to see your fills,
        fees, and maker/taker role.
      </p>
      {error && (
        <p className="text-xs text-terra bg-terra/10 px-3 py-2 mb-4 max-w-xs mx-auto">
          {error}
        </p>
      )}
      <Button size="lg" loading={connecting} onClick={handle} className="px-8">
        Connect Wallet
      </Button>
    </Card>
  )
}

function RoleBadge({ role }: { role: FillRecord['role'] }) {
  if (role === 'maker') return <Badge variant="secondary" size="sm">Maker</Badge>
  if (role === 'taker') return <Badge variant="primary" size="sm">Taker</Badge>
  return <Badge variant="neutral" size="sm">—</Badge>
}

function SourceDot({ source }: { source: FillRecord['source'] }) {
  if (source === 'live') {
    return (
      <span className="inline-flex items-center gap-1 text-[10px] font-medium text-sage">
        <span className="w-1.5 h-1.5 rounded-full bg-sage animate-pulse inline-block" />
        Live
      </span>
    )
  }
  return null
}

function Filters({
  markets,
  filterMarket,
  filterDateFrom,
  filterDateTo,
  onFilter,
}: {
  markets: MarketResponse[]
  filterMarket: string
  filterDateFrom: string
  filterDateTo: string
  onFilter: (key: HistAction & { type: 'SET_FILTER' }) => void
}) {
  return (
    <div className="flex flex-wrap items-end gap-3">
      <div className="flex flex-col gap-1.5">
        <label className="text-[0.7rem] font-medium text-brown uppercase tracking-[0.12em]">Market</label>
        <select
          value={filterMarket}
          onChange={(e) =>
            onFilter({
              type: 'SET_FILTER',
              key: 'filterMarket',
              value: e.target.value,
            })
          }
          className="h-10 px-3 pr-8 bg-canvas border border-border text-sm text-ink outline-none focus:border-ink cursor-pointer appearance-none min-w-[140px]"
        >
          <option value="">All markets</option>
          {markets.map((m) => (
            <option key={m.id} value={m.id}>
              {m.base}/{m.quote}
            </option>
          ))}
        </select>
      </div>

      <Input
        label="From"
        type="date"
        value={filterDateFrom}
        onChange={(e) =>
          onFilter({
            type: 'SET_FILTER',
            key: 'filterDateFrom',
            value: e.target.value,
          })
        }
        className="text-xs"
      />

      <Input
        label="To"
        type="date"
        value={filterDateTo}
        onChange={(e) =>
          onFilter({
            type: 'SET_FILTER',
            key: 'filterDateTo',
            value: e.target.value,
          })
        }
        className="text-xs"
      />

      {(filterMarket || filterDateFrom || filterDateTo) && (
        <Button
          variant="ghost"
          size="sm"
          onClick={() => {
            onFilter({ type: 'SET_FILTER', key: 'filterMarket', value: '' })
            onFilter({ type: 'SET_FILTER', key: 'filterDateFrom', value: '' })
            onFilter({ type: 'SET_FILTER', key: 'filterDateTo', value: '' })
          }}
          className="mb-0.5"
        >
          Clear
        </Button>
      )}
    </div>
  )
}

function FillHistoryTable({
  fills,
  hasMore,
  loadingInitial,
  onLoadMore,
  onExport,
}: {
  fills: FillRecord[]
  hasMore: boolean
  loadingInitial: boolean
  onLoadMore: () => void
  onExport: () => void
}) {
  return (
    <Card padding="none">
      <div className="px-6 py-4 border-b border-border flex items-center justify-between flex-wrap gap-3">
        <div className="flex items-center gap-3">
          <span className="text-[0.65rem] uppercase tracking-[0.15em] text-brown font-medium">Fill History</span>
          <Badge variant="neutral">{fills.length}</Badge>
        </div>
        <Button
          variant="secondary"
          size="sm"
          disabled={fills.length === 0}
          onClick={onExport}
          icon={
            <svg width="13" height="13" viewBox="0 0 14 14" fill="none">
              <path
                d="M7 1v8M4 6l3 3 3-3M2 11h10"
                stroke="currentColor"
                strokeWidth="1.5"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
            </svg>
          }
        >
          Export CSV
        </Button>
      </div>

      {loadingInitial ? (
        <div className="py-12 flex justify-center">
          <Spinner size="lg" className="text-ochre" />
        </div>
      ) : fills.length === 0 ? (
        <p className="px-6 py-12 text-center text-sm text-brown">
          No fills match the current filters.
        </p>
      ) : (
        <>
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="text-left border-b border-border bg-canvas">
                  <th className="px-5 py-3 text-[0.65rem] font-medium text-brown uppercase tracking-[0.12em]">Time</th>
                  <th className="px-5 py-3 text-[0.65rem] font-medium text-brown uppercase tracking-[0.12em]">Market</th>
                  <th className="px-5 py-3 text-[0.65rem] font-medium text-brown uppercase tracking-[0.12em]">Side</th>
                  <th className="px-5 py-3 text-right text-[0.65rem] font-medium text-brown uppercase tracking-[0.12em]">Price</th>
                  <th className="px-5 py-3 text-right text-[0.65rem] font-medium text-brown uppercase tracking-[0.12em]">Size</th>
                  <th className="px-5 py-3 text-right text-[0.65rem] font-medium text-brown uppercase tracking-[0.12em]">Fee</th>
                  <th className="px-5 py-3 text-[0.65rem] font-medium text-brown uppercase tracking-[0.12em]">Role</th>
                </tr>
              </thead>
              <tbody>
                {fills.map((f, idx) => (
                  <tr
                    key={f.key}
                    className={[
                      'hover:bg-[rgba(232,228,216,0.04)] transition-colors duration-75 border-b border-border last:border-0',
                      idx % 2 === 0 ? 'bg-parchment' : 'bg-canvas',
                    ].join(' ')}
                  >
                    <td className="px-5 py-2.5">
                      <div className="text-xs text-ink tabular-nums font-mono">
                        {fmtTs(f.ts)}
                      </div>
                      <SourceDot source={f.source} />
                    </td>
                    <td className="px-5 py-2.5 text-xs font-medium text-ink">
                      {f.market}
                    </td>
                    <td className="px-5 py-2.5">
                      <Badge
                        variant={f.side === 'buy' ? 'success' : 'error'}
                        size="sm"
                      >
                        {f.side}
                      </Badge>
                    </td>
                    <td className="px-5 py-2.5 text-right tabular-nums text-xs text-ink font-mono">
                      {parseFloat(f.price).toFixed(4)}
                    </td>
                    <td className="px-5 py-2.5 text-right tabular-nums text-xs text-ink font-mono">
                      {parseFloat(f.size).toFixed(4)}
                    </td>
                    <td className="px-5 py-2.5 text-right tabular-nums text-xs text-brown font-mono">
                      {f.fee}
                    </td>
                    <td className="px-5 py-2.5">
                      <RoleBadge role={f.role} />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          {hasMore && (
            <div className="px-6 py-4 border-t border-border flex justify-center">
              <Button variant="secondary" size="sm" onClick={onLoadMore}>
                Load more
              </Button>
            </div>
          )}
        </>
      )}
    </Card>
  )
}

function PublicFeed({
  markets,
  publicMarket,
  trades,
  onSelectMarket,
}: {
  markets: MarketResponse[]
  publicMarket: string | null
  trades: PublicTrade[]
  onSelectMarket: (market: string) => void
}) {
  return (
    <Card padding="none">
      <div className="px-6 py-4 border-b border-border flex items-center justify-between flex-wrap gap-3">
        <div className="flex items-center gap-3">
          <span className="text-[0.65rem] uppercase tracking-[0.15em] text-brown font-medium">Public Trade Feed</span>
          <Badge variant="success" dot size="sm">
            Public
          </Badge>
        </div>
        <div className="flex items-center gap-1.5 flex-wrap">
          {markets.map((m) => (
            <button
              key={m.id}
              type="button"
              onClick={() => onSelectMarket(m.id)}
              className={[
                'px-2.5 py-1 text-xs font-medium transition-colors duration-150 uppercase tracking-[0.08em]',
                publicMarket === m.id
                  ? 'bg-[#E8E4D8] text-[#0C0C0C]'
                  : 'border border-border text-brown hover:bg-canvas',
              ].join(' ')}
            >
              {m.base}/{m.quote}
            </button>
          ))}
        </div>
      </div>

      {trades.length === 0 ? (
        <p className="px-6 py-10 text-center text-sm text-brown">
          {publicMarket
            ? 'Waiting for trades…'
            : 'Select a market to see the live feed.'}
        </p>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="text-left border-b border-border bg-canvas">
                <th className="px-5 py-2.5 text-[0.65rem] font-medium text-brown uppercase tracking-[0.12em]">Time</th>
                <th className="px-5 py-2.5 text-[0.65rem] font-medium text-brown uppercase tracking-[0.12em]">Side</th>
                <th className="px-5 py-2.5 text-right text-[0.65rem] font-medium text-brown uppercase tracking-[0.12em]">Price</th>
                <th className="px-5 py-2.5 text-right text-[0.65rem] font-medium text-brown uppercase tracking-[0.12em]">Size</th>
              </tr>
            </thead>
            <tbody>
              {trades.map((t) => (
                <tr
                  key={t.key}
                  className="hover:bg-[rgba(232,228,216,0.04)] transition-colors duration-75 border-b border-border last:border-0"
                >
                  <td className="px-5 py-[3px] text-[11px] tabular-nums text-brown font-mono">
                    {fmtTime(t.ts)}
                  </td>
                  <td className="px-5 py-[3px]">
                    <span
                      className={[
                        'text-[11px] font-semibold',
                        t.side === 'buy' ? 'text-sage' : 'text-terra',
                      ].join(' ')}
                    >
                      {t.side}
                    </span>
                  </td>
                  <td className="px-5 py-[3px] text-right tabular-nums text-[11px] font-medium text-ink font-mono">
                    {parseFloat(t.price).toFixed(4)}
                  </td>
                  <td className="px-5 py-[3px] text-right tabular-nums text-[11px] text-brown font-mono">
                    {parseFloat(t.size).toFixed(4)}
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

export default function HistoryPage() {
  const { address, isConnected, isAuthenticated, connect, signIn } = useAuth()
  const [state, dispatch] = useReducer(histReducer, initState)
  const wsRef = useRef(getWsClient())

  const handleConnect = useCallback(async () => {
    const addr = await connect()
    const ws = wsRef.current
    ws.connect()
    try {
      await signIn(ws, addr)
    } catch {
      // wallet connected; WS auth best-effort
    }
  }, [connect, signIn])

  useEffect(() => {
    listMarkets().then((res) => {
      if (res.ok && res.data) {
        dispatch({ type: 'INIT_MARKETS', markets: res.data })
      }
    })
  }, [])

  useEffect(() => {
    if (!address || !isAuthenticated) return
    dispatch({ type: 'SET_LOADING_INITIAL', loading: true })
    Promise.all([getOrders(address), getAccountFills(address)])
      .then(([ordersResponse, fillsResponse]) => {
        const orders = ordersResponse.ok && ordersResponse.data ? ordersResponse.data : []
        const fills = fillsResponse.ok && fillsResponse.data
          ? fillsResponse.data.map(accountFillToRecord)
          : []
        const { marketByOrderId } = buildOrderMaps(orders)
        dispatch({ type: 'INIT', fills, marketByOrderId })
      })
      .catch(() => dispatch({ type: 'SET_LOADING_INITIAL', loading: false }))
  }, [address, isAuthenticated])

  const { marketByOrderId } = state

  useEffect(() => {
    if (!address || !isAuthenticated) return
    const ws = wsRef.current
    ws.connect()

    const unsub = ws.onMessage((msg) => {
      if (msg.type === 'fill') {
        const isMaker = msg.maker_order_id in marketByOrderId
        const isTaker = msg.taker_order_id in marketByOrderId

        if (!isMaker && !isTaker) return

        const role: FillRecord['role'] = isMaker ? 'maker' : 'taker'
        const orderId = isMaker ? msg.maker_order_id : msg.taker_order_id
        const market = marketByOrderId[orderId] ?? ''
        const fee = isMaker ? msg.maker_fee : msg.taker_fee
        const takerSide = msg.side as 'buy' | 'sell'
        const ourSide: 'buy' | 'sell' = isMaker
          ? takerSide === 'buy'
            ? 'sell'
            : 'buy'
          : takerSide

        const fill: FillRecord = {
          key: `live-${msg.timestamp}-${msg.maker_order_id}-${msg.taker_order_id}`,
          ts: msg.timestamp,
          market,
          side: ourSide,
          price: msg.price,
          size: msg.quantity,
          fee,
          role,
          source: 'live',
        }
        dispatch({ type: 'PREPEND_LIVE', fill })
      }
    })

    return () => unsub()
  }, [address, isAuthenticated, marketByOrderId])

  useEffect(() => {
    if (!state.publicMarket) return
    const ws = wsRef.current
    ws.connect()
    const channel = `trades:${state.publicMarket}`
    ws.subscribeChannels([channel])

    const unsub = ws.onMessage((msg) => {
      if (msg.type === 'trade' && msg.market === state.publicMarket) {
        dispatch({
          type: 'ADD_PUBLIC_TRADE',
          trade: {
            key: `pub-${msg.timestamp}-${msg.price}-${msg.quantity}`,
            ts: msg.timestamp,
            price: msg.price,
            size: msg.quantity,
            side: msg.side as 'buy' | 'sell',
          },
        })
      }
    })

    return () => {
      unsub()
      ws.unsubscribeChannels([channel])
    }
  }, [state.publicMarket])

  const allFillRecords = useMemo(() => {
    return [...state.liveFills, ...state.historicalFills]
      .filter((fill, index, fills) => fills.findIndex((item) => item.key === fill.key) === index)
      .sort((a, b) => b.ts - a.ts)
  }, [state.historicalFills, state.liveFills])

  const hasMore = useMemo(
    () => allFillRecords.length > state.visiblePage * PAGE_SIZE,
    [allFillRecords.length, state.visiblePage],
  )

  const filteredFills = useMemo(() => {
    const visible = allFillRecords.slice(0, state.visiblePage * PAGE_SIZE)
    return visible.filter((f) => {
      if (state.filterMarket && f.market !== state.filterMarket) return false
      if (state.filterDateFrom) {
        const from = new Date(state.filterDateFrom).getTime()
        if (f.ts < from) return false
      }
      if (state.filterDateTo) {
        const to = new Date(state.filterDateTo + 'T23:59:59.999').getTime()
        if (f.ts > to) return false
      }
      return true
    })
  }, [
    allFillRecords,
    state.visiblePage,
    state.filterMarket,
    state.filterDateFrom,
    state.filterDateTo,
  ])

  const handleExport = useCallback(() => {
    const ts = new Date().toISOString().slice(0, 10)
    const mkt = state.filterMarket || 'all'
    exportCSV(filteredFills, `vela-fills-${mkt}-${ts}.csv`)
  }, [filteredFills, state.filterMarket])

  const handleFilter = useCallback((action: HistAction & { type: 'SET_FILTER' }) => {
    dispatch(action)
  }, [])

  const uniqueMarkets = useMemo(() => {
    const seen = new Set(allFillRecords.map((f) => f.market))
    return state.markets.filter((m) => seen.has(m.id))
  }, [allFillRecords, state.markets])

  return (
    <div className="max-w-7xl mx-auto px-4 sm:px-6 py-8">
      <div className="mb-8">
        <h1 className="text-2xl font-bold text-ink">Trade History</h1>
        <p className="text-sm text-brown mt-1">
          Private fill history and live anonymized trade feed.
        </p>
      </div>

      {!isConnected || !isAuthenticated ? (
        <div className="mb-8">
          <ConnectGate onConnect={handleConnect} />
        </div>
      ) : (
        <div className="mb-8 space-y-5">
          <div className="flex flex-wrap items-center justify-between gap-4">
            <Filters
              markets={uniqueMarkets.length > 0 ? uniqueMarkets : state.markets}
              filterMarket={state.filterMarket}
              filterDateFrom={state.filterDateFrom}
              filterDateTo={state.filterDateTo}
              onFilter={handleFilter}
            />
          </div>

          <FillHistoryTable
            fills={filteredFills}
            hasMore={hasMore}
            loadingInitial={state.loadingInitial}
            onLoadMore={() => dispatch({ type: 'APPEND_HISTORICAL' })}
            onExport={handleExport}
          />
        </div>
      )}

      <div className="border-t border-border pt-8">
        <div className="mb-4">
          <h2 className="text-lg font-semibold text-ink">
            Aggregate Trade Feed
          </h2>
          <p className="text-sm text-brown mt-0.5">
            Public, anonymized — no auth required.
          </p>
        </div>

        {state.markets.length === 0 ? (
          <Card>
            <p className="text-sm text-brown text-center py-4">
              Loading markets…
            </p>
          </Card>
        ) : (
          <PublicFeed
            markets={state.markets}
            publicMarket={state.publicMarket}
            trades={state.publicTrades}
            onSelectMarket={(m) =>
              dispatch({ type: 'SET_PUBLIC_MARKET', market: m })
            }
          />
        )}
      </div>
    </div>
  )
}
