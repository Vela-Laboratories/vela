'use client'

import { use, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import Link from 'next/link'
import { useRouter } from 'next/navigation'
import Skeleton from '@/components/ui/Skeleton'
// Chart runtime is dynamically imported inside ChartArea's mount effect
// so it doesn't land in the initial route bundle (~300 KB gzipped).
// Types are erased at compile time and add no runtime cost.
import type { IChartApi, ISeriesApi, CandlestickData, Time } from 'lightweight-charts'
import {
  getOrders,
  getBalances,
  postOrder,
  cancelOrder,
  type Order,
  type PostOrderBody,
  type CancelOrderBody,
} from '@/lib/api'
import { signOrder, signCancel } from '@/lib/signing'
import { useAuth } from '@/lib/auth'
import { velaWs } from '@/lib/ws'
import { ORDERED_PAIRS, getMarketInfo, getPriceDecimals } from '@/lib/markets'
import { fetchReferenceCandles, fetchReferenceMarkets, type ReferenceMarket } from '@/lib/market-data'

type OrderSide = 'buy' | 'sell'
type OrderEntryType = 'limit' | 'market' | 'stop'
type Timeframe = '1m' | '5m' | '15m' | '1H' | '4H' | '1D'
type LiveMode = 'reference' | 'unavailable' | null

interface Toast {
  id: number
  message: string
  variant: 'success' | 'error'
}

const IN = 'Inter, sans-serif'
const CN = "'Courier New', monospace"

function fmtPrice(v: string | number | undefined, decimals = 2): string {
  if (v == null || v === '') return '—'
  const n = typeof v === 'string' ? parseFloat(v) : v
  return isNaN(n) ? '—' : n.toFixed(decimals)
}

function fmtSize(v: string | number | undefined, ticker = 'BTC'): string {
  if (v == null || v === '') return '—'
  const n = typeof v === 'string' ? parseFloat(v) : v
  if (isNaN(n)) return '—'
  return ['BTC', 'ETH'].includes(ticker.toUpperCase()) ? n.toFixed(4) : n.toFixed(2)
}

function fmtCompact(value: number | undefined): string {
  if (value === undefined || !Number.isFinite(value)) return '—'
  return new Intl.NumberFormat('en-US', { notation: 'compact', maximumFractionDigits: 2 }).format(value)
}

function fmtOrderTime(ts: number): string {
  const d = new Date(ts < 1e12 ? ts * 1000 : ts)
  return d.toLocaleTimeString('en-US', { hour12: false, hour: '2-digit', minute: '2-digit', second: '2-digit' })
}

function useToasts() {
  const [toasts, setToasts] = useState<Toast[]>([])
  const idRef = useRef(0)
  const addToast = useCallback((message: string, variant: 'success' | 'error') => {
    const id = ++idRef.current
    setToasts((prev) => [...prev, { id, message, variant }])
    setTimeout(() => setToasts((prev) => prev.filter((t) => t.id !== id)), 3000)
  }, [])
  return { toasts, addToast }
}

function ToastContainer({ toasts }: { toasts: Toast[] }) {
  return (
    <div style={{ position: 'fixed', bottom: 24, right: 24, zIndex: 1000, display: 'flex', flexDirection: 'column-reverse', gap: 8, pointerEvents: 'none' }}>
      {toasts.map((t) => (
        <div
          key={t.id}
          style={{
            background: '#111110',
            border: '1px solid rgba(232,228,216,0.08)',
            borderLeft: `3px solid ${t.variant === 'success' ? '#6B8A5A' : '#CC3333'}`,
            padding: '10px 16px',
            fontFamily: IN,
            fontSize: 11,
            color: 'rgba(232,228,216,0.7)',
            minWidth: 220,
            maxWidth: 320,
          }}
        >
          {t.message}
        </div>
      ))}
    </div>
  )
}

function StatGroup({ label, value }: { label: string; value: string }) {
  return (
    <div className="terminal-stat">
      <span>
        {label}
      </span>
      <strong>
        {value}
      </strong>
    </div>
  )
}

function TopBar({ pair, reference, sourceLive }: { pair: string; reference: ReferenceMarket | undefined; sourceLive: boolean | null }) {
  const info = getMarketInfo(pair)
  const decimals = getPriceDecimals(pair)
  const change = reference?.change24h
  const changeText = change === undefined ? '—' : `${change >= 0 ? '+' : ''}${change.toFixed(2)}%`

  return (
    <div className="terminal-market-bar">
      <div className="terminal-market-identity">
        <Link href="/markets" aria-label="All markets">‹</Link>
        <div className="asset-mark">{info.ticker.slice(0, 1)}</div>
        <div>
          <div className="terminal-pair">{info.ticker}<span>/ {info.quoteTicker}</span></div>
          <div className="terminal-product">Binance Spot · USDC reference</div>
        </div>
      </div>

      <div className="terminal-mark-price">
        <span>Reference price</span>
        {reference ? <strong>{reference.price.toFixed(decimals)}</strong> : sourceLive === null ? <Skeleton className="w-24 h-5 inline-block" /> : <strong>—</strong>}
      </div>

      <div className="terminal-stats">
        <StatGroup label="24H change" value={changeText} />
        <StatGroup label="24H high" value={reference ? reference.high24h.toFixed(decimals) : '—'} />
        <StatGroup label="24H volume" value={reference ? `${fmtCompact(reference.quoteVolume24h)} USDC` : '—'} />
      </div>

      <div className="terminal-connection" style={sourceLive === false ? { color: '#CC6B6B' } : undefined}>
        <span style={sourceLive === false ? { background: '#CC6B6B' } : undefined} />
        {sourceLive === false ? 'Source unavailable' : 'Binance live'}
      </div>
    </div>
  )
}

const TIMEFRAMES: Timeframe[] = ['1m', '5m', '15m', '1H', '4H', '1D']

function TimeframeRow({ timeframe, onChange, isLive }: { timeframe: Timeframe; onChange: (tf: Timeframe) => void; isLive: LiveMode }) {
  const liveLabel = isLive === 'reference' ? '● BINANCE LIVE' : isLive === 'unavailable' ? '○ DATA UNAVAILABLE' : null
  const liveColor = isLive === 'reference' ? '#6B8A5A' : '#CC6B6B'

  return (
    <div className="terminal-timeframes">
      {TIMEFRAMES.map((tf) => (
        <button
          key={tf}
          onClick={() => onChange(tf)}
          style={{
            background: 'transparent',
            border: 'none',
            borderBottom: timeframe === tf ? '2px solid #9C9CF6' : '2px solid transparent',
            color: timeframe === tf ? '#F4F4F5' : '#777789',
            fontFamily: IN,
            fontSize: 12,
            letterSpacing: '0.07em',
            padding: '6px 10px',
            cursor: 'pointer',
            borderRadius: 0,
            textTransform: 'uppercase',
          }}
        >
          {tf}
        </button>
      ))}
      {liveLabel !== null && (
        <span style={{
          marginLeft: 'auto',
          marginBottom: 8,
          fontFamily: IN,
          fontSize: 11,
          textTransform: 'uppercase',
          letterSpacing: '0.1em',
          color: liveColor,
        }}>
          {liveLabel}
        </span>
      )}
    </div>
  )
}

async function getChartCandles(pair: string, timeframe: Timeframe, limit: number): Promise<CandlestickData<Time>[]> {
  const snapshot = await fetchReferenceCandles(pair, timeframe, limit)
  return snapshot.candles.map((c) => ({
      time: c.time as Time,
      open: c.open,
      high: c.high,
      low: c.low,
      close: c.close,
    }))
}

function ChartArea({ pair, timeframe, onLiveChange }: { pair: string; timeframe: Timeframe; onLiveChange: (isLive: LiveMode) => void }) {
  const containerRef = useRef<HTMLDivElement>(null)
  const chartRef = useRef<IChartApi | null>(null)
  const seriesRef = useRef<ISeriesApi<'Candlestick', Time> | null>(null)
  const [chartReady, setChartReady] = useState(false)
  const [hasCandles, setHasCandles] = useState(false)

  useEffect(() => {
    const container = containerRef.current
    if (!container) return

    let cancelled = false
    let cleanup: (() => void) | null = null

    ;(async () => {
      const { createChart, CandlestickSeries } = await import('lightweight-charts')
      if (cancelled) return

      const chart = createChart(container, {
        layout: {
          background: { color: '#09090E' },
          textColor: '#7F7F91',
          fontFamily: IN,
          fontSize: 11,
        },
        grid: {
          vertLines: { color: '#15151D' },
          horzLines: { color: '#15151D' },
        },
        crosshair: {
          vertLine: { color: '#4A4A5D', width: 1, style: 3 },
          horzLine: { color: '#4A4A5D', width: 1, style: 3 },
        },
        rightPriceScale: { borderColor: '#242431' },
        timeScale: { borderColor: '#242431', timeVisible: true, secondsVisible: false },
        width: container.clientWidth,
        height: container.clientHeight,
      })

      const series = chart.addSeries(CandlestickSeries, {
        upColor: '#2ED67B',
        downColor: '#FF5C67',
        borderUpColor: '#2ED67B',
        borderDownColor: '#FF5C67',
        wickUpColor: '#2ED67B',
        wickDownColor: '#FF5C67',
      })

      chartRef.current = chart
      seriesRef.current = series
      setChartReady(true)

      const ro = new ResizeObserver(() => {
        if (container && chartRef.current) {
          chartRef.current.applyOptions({ width: container.clientWidth, height: container.clientHeight })
        }
      })
      ro.observe(container)

      cleanup = () => {
        ro.disconnect()
        chart.remove()
        chartRef.current = null
        seriesRef.current = null
        setChartReady(false)
      }
    })()

    return () => {
      cancelled = true
      if (cleanup) cleanup()
    }
  }, [])

  useEffect(() => {
    if (!chartReady || !seriesRef.current) return
    let cancelled = false
    let interval: ReturnType<typeof setInterval> | null = null

    async function loadInitial() {
      try {
        const candles = await getChartCandles(pair, timeframe, 200)
        if (cancelled || candles.length < 2) return
        seriesRef.current?.setData(candles)
        chartRef.current?.timeScale().fitContent()
        setHasCandles(true)
        onLiveChange('reference')
      } catch {
        if (!cancelled) {
          setHasCandles(false)
          seriesRef.current?.setData([])
          onLiveChange('unavailable')
        }
      }
    }

    async function refreshLatest() {
      try {
        const candles = await getChartCandles(pair, timeframe, 2)
        if (cancelled) return
        const latestCandle = candles.at(-1)
        if (latestCandle) {
          seriesRef.current?.update(latestCandle)
          setHasCandles(true)
          onLiveChange('reference')
        }
      } catch {
        if (!cancelled) onLiveChange('unavailable')
      }
    }

    void loadInitial().then(() => {
      if (!cancelled) interval = setInterval(() => void refreshLatest(), 3_000)
    })

    return () => {
      cancelled = true
      if (interval) clearInterval(interval)
    }
  }, [chartReady, timeframe, pair, onLiveChange])

  return (
    <div style={{ flex: 1, position: 'relative', overflow: 'hidden' }}>
      <div ref={containerRef} style={{ position: 'absolute', inset: 0, width: '100%', height: '100%' }} />
      {!hasCandles ? (
        <div style={{ position: 'absolute', inset: 0, display: 'grid', placeItems: 'center', pointerEvents: 'none', color: '#69697A', fontFamily: IN, fontSize: 12 }}>
          Waiting for Binance market data
        </div>
      ) : null}
    </div>
  )
}

function OpenOrdersPanel({ pair, onToast }: { pair: string; onToast: (msg: string, v: 'success' | 'error') => void }) {
  const { address, isConnected, isAuthenticated } = useAuth()
  const [orders, setOrders] = useState<Order[]>([])
  const info = getMarketInfo(pair)

  const fetchOrders = useCallback(() => {
    if (!address || !isAuthenticated) return
    getOrders(address).then((res) => {
      if (res.ok && res.data) setOrders(res.data)
    }).catch(() => {})
  }, [address, isAuthenticated])

  useEffect(() => {
    fetchOrders()
    const interval = setInterval(fetchOrders, 5000)
    return () => clearInterval(interval)
  }, [fetchOrders])

  const openOrders = useMemo(
    () => orders.filter((o) => o.status === 'open' || o.status === 'partial'),
    [orders],
  )

  const handleCancel = useCallback(async (order: Order) => {
    if (!address) return
    const nonce = Date.now()
    try {
      const sig = await signCancel({ order_id: order.id, nonce, address })
      const body: CancelOrderBody = { order_id: order.id, nonce, address, signature: sig }
      const res = await cancelOrder(body)
      if (res.ok) {
        onToast('Order cancelled', 'success')
        fetchOrders()
      } else {
        onToast(res.error ?? 'Cancel failed', 'error')
      }
    } catch (err) {
      if (err instanceof Error && (err.message.includes('rejected') || err.message.includes('denied'))) {
        onToast('Signature rejected', 'error')
      } else {
        onToast(err instanceof Error ? err.message : 'Cancel failed', 'error')
      }
    }
  }, [address, onToast, fetchOrders])

  const showEmpty = !isConnected || !isAuthenticated || openOrders.length === 0
  const emptyMsg = !isConnected || !isAuthenticated
    ? 'Connect and authenticate to view orders'
    : 'No open orders'

  return (
    <div className="terminal-orders" style={{ height: 176, flexShrink: 0, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      <div style={{ padding: '8px 16px', borderBottom: '1px solid rgba(232,228,216,0.05)', flexShrink: 0 }}>
        <span style={{ fontFamily: IN, fontSize: 11, fontWeight: 600, color: '#D6D6DF' }}>
          OPEN ORDERS{isConnected ? ` (${openOrders.length})` : ''}
        </span>
      </div>

      {showEmpty ? (
        <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
          <span style={{ fontFamily: IN, fontSize: 12, color: '#69697A', fontWeight: 400 }}>
            {emptyMsg}
          </span>
        </div>
      ) : (
        <>
          <div style={{ display: 'grid', gridTemplateColumns: '90px 55px 45px 75px 75px 1fr', padding: '5px 16px', flexShrink: 0 }}>
            {['TIME', 'PAIR', 'SIDE', 'PRICE', 'SIZE', 'ACTION'].map((h) => (
              <span key={h} style={{ fontFamily: IN, fontSize: 7.5, textTransform: 'uppercase', color: 'rgba(232,228,216,0.18)', letterSpacing: '0.1em' }}>
                {h}
              </span>
            ))}
          </div>
          <div style={{ overflowY: 'auto', flex: 1 }}>
            {openOrders.map((order) => (
              <div key={order.id} style={{ display: 'grid', gridTemplateColumns: '90px 55px 45px 75px 75px 1fr', padding: '4px 16px', alignItems: 'center' }}>
                <span style={{ fontFamily: CN, fontSize: 10, color: 'rgba(232,228,216,0.3)' }}>{fmtOrderTime(order.timestamp)}</span>
                <span style={{ fontFamily: IN, fontSize: 10, color: '#E8E4D8' }}>{order.market}</span>
                <span style={{ fontFamily: IN, fontSize: 10, textTransform: 'uppercase', color: order.side === 'buy' ? '#6B8A5A' : '#CC3333' }}>{order.side}</span>
                <span style={{ fontFamily: CN, fontSize: 10, color: '#E8E4D8' }}>{fmtPrice(order.price)}</span>
                <span style={{ fontFamily: CN, fontSize: 10, color: '#E8E4D8' }}>{fmtSize(order.quantity, info.ticker)}</span>
                {(order.status === 'open' || order.status === 'partial') ? (
                  <button
                    onClick={() => handleCancel(order)}
                    style={{ background: 'transparent', border: 'none', fontFamily: IN, fontSize: 9, color: 'rgba(232,228,216,0.28)', cursor: 'pointer', padding: 0, borderRadius: 0, textAlign: 'left' }}
                    onMouseEnter={(e) => { (e.currentTarget as HTMLButtonElement).style.color = '#CC3333' }}
                    onMouseLeave={(e) => { (e.currentTarget as HTMLButtonElement).style.color = 'rgba(232,228,216,0.28)' }}
                  >
                    Cancel
                  </button>
                ) : <span />}
              </div>
            ))}
          </div>
        </>
      )}
    </div>
  )
}

function ChartColumn({ pair, onToast }: { pair: string; onToast: (msg: string, v: 'success' | 'error') => void }) {
  const [timeframe, setTimeframe] = useState<Timeframe>('1H')
  const [isLive, setIsLive] = useState<LiveMode>(null)
  const handleLiveChange = useCallback((live: LiveMode) => setIsLive(live), [])

  return (
    <div className="terminal-chart-column" style={{ display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      <TimeframeRow timeframe={timeframe} onChange={setTimeframe} isLive={isLive} />
      <ChartArea pair={pair} timeframe={timeframe} onLiveChange={handleLiveChange} />
      <OpenOrdersPanel pair={pair} onToast={onToast} />
    </div>
  )
}

function OrderBookPanel({
  bids,
  asks,
  pair,
  loading,
}: {
  bids: { price: string; quantity: string }[]
  asks: { price: string; quantity: string }[]
  pair: string
  loading: boolean
}) {
  const info = getMarketInfo(pair)
  const decimals = getPriceDecimals(pair)

  const topAsks = useMemo(() => asks.slice(0, 10).reverse(), [asks])
  const topBids = useMemo(() => bids.slice(0, 10), [bids])

  const maxSize = useMemo(() => {
    const sizes = [
      ...asks.slice(0, 10).map((a) => parseFloat(a.quantity)),
      ...bids.slice(0, 10).map((b) => parseFloat(b.quantity)),
    ]
    return sizes.length ? Math.max(...sizes) : 1
  }, [bids, asks])

  const spread = useMemo(() => {
    if (!bids[0] || !asks[0]) return null
    const s = parseFloat(asks[0].price) - parseFloat(bids[0].price)
    return s > 0 ? s.toFixed(4) : null
  }, [bids, asks])

  return (
    <div className="terminal-orderbook" style={{ display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      <div style={{ padding: '10px 12px', borderBottom: '1px solid rgba(232,228,216,0.05)', flexShrink: 0 }}>
        <span style={{ fontFamily: IN, fontSize: 12, fontWeight: 600, color: '#D6D6DF' }}>
          VELA TESTNET BOOK
        </span>
      </div>

      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', padding: '5px 12px', flexShrink: 0 }}>
        {['PRICE', 'SIZE'].map((h) => (
          <span key={h} style={{ fontFamily: IN, fontSize: 10, textTransform: 'uppercase', color: '#69697A', letterSpacing: '0.08em' }}>
            {h}
          </span>
        ))}
      </div>

      <div style={{ flex: 1, overflowY: 'hidden', display: 'flex', flexDirection: 'column', justifyContent: 'flex-end' }}>
        {loading
          ? Array.from({ length: 10 }).map((_, i) => (
              <div key={i} style={{ padding: '3.5px 12px', display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8 }}>
                <Skeleton className="w-16 h-3" />
                <Skeleton className="w-10 h-3 ml-auto" />
              </div>
            ))
          : topAsks.map((lvl) => {
              const pct = maxSize > 0 ? (parseFloat(lvl.quantity) / maxSize) * 100 : 0
              return (
                <div key={`ask-${lvl.price}`} style={{ position: 'relative', padding: '3.5px 12px', display: 'grid', gridTemplateColumns: '1fr 1fr' }}>
                  <div style={{ position: 'absolute', right: 0, top: 0, bottom: 0, width: `${pct}%`, background: 'rgba(204,51,51,0.08)' }} />
                  <span style={{ fontFamily: CN, fontSize: 11.5, color: '#FF5C67', position: 'relative', zIndex: 1 }}>{fmtPrice(lvl.price, decimals)}</span>
                  <span style={{ fontFamily: CN, fontSize: 11.5, color: '#C4C4CE', textAlign: 'right', position: 'relative', zIndex: 1 }}>{fmtSize(lvl.quantity, info.ticker)}</span>
                </div>
              )
            })
        }
      </div>

      <div style={{ padding: '5px 12px', borderTop: '1px solid rgba(232,228,216,0.04)', borderBottom: '1px solid rgba(232,228,216,0.04)', display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexShrink: 0 }}>
        <span style={{ fontFamily: IN, fontSize: 8, color: 'rgba(232,228,216,0.3)' }}>◆ {spread ?? '—'}</span>
        <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
          <span style={{ width: 5, height: 5, borderRadius: '50%', background: '#6B8A5A', display: 'inline-block', flexShrink: 0 }} />
          <span style={{ fontFamily: IN, fontSize: 7, color: 'rgba(107,138,90,0.6)', letterSpacing: '0.12em' }}>LIVE</span>
        </div>
      </div>

      <div style={{ flex: 1, overflowY: 'hidden' }}>
        {loading
          ? Array.from({ length: 10 }).map((_, i) => (
              <div key={i} style={{ padding: '3.5px 12px', display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8 }}>
                <Skeleton className="w-16 h-3" />
                <Skeleton className="w-10 h-3 ml-auto" />
              </div>
            ))
          : topBids.map((lvl) => {
              const pct = maxSize > 0 ? (parseFloat(lvl.quantity) / maxSize) * 100 : 0
              return (
                <div key={`bid-${lvl.price}`} style={{ position: 'relative', padding: '3.5px 12px', display: 'grid', gridTemplateColumns: '1fr 1fr' }}>
                  <div style={{ position: 'absolute', left: 0, top: 0, bottom: 0, width: `${pct}%`, background: 'rgba(107,138,90,0.08)' }} />
                  <span style={{ fontFamily: CN, fontSize: 11.5, color: '#2ED67B', position: 'relative', zIndex: 1 }}>{fmtPrice(lvl.price, decimals)}</span>
                  <span style={{ fontFamily: CN, fontSize: 11.5, color: '#C4C4CE', textAlign: 'right', position: 'relative', zIndex: 1 }}>{fmtSize(lvl.quantity, info.ticker)}</span>
                </div>
              )
            })
        }
      </div>
    </div>
  )
}

function MarketSelectorColumn({ currentPair, markets }: { currentPair: string; markets: ReferenceMarket[] }) {
  const router = useRouter()

  const marketsById = useMemo(() => Object.fromEntries(markets.map((m) => [m.market, m])), [markets])

  return (
    <div className="terminal-market-list" style={{ display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      <nav className="terminal-rail-tabs" aria-label="Discovery">
        <span className="is-active">Markets</span>
        <Link href="/history">Activity</Link>
        <Link href="/leaderboard">Leaders</Link>
      </nav>
      <div style={{ overflowY: 'auto', flex: 1 }}>
        {ORDERED_PAIRS.map((pair) => {
          const market = marketsById[pair]
          const info = getMarketInfo(pair)
          const isActive = pair === currentPair
          const decimals = getPriceDecimals(pair)
          const change = market?.change24h

          return (
            <div
              key={pair}
              onClick={() => router.push(`/markets/${pair}`)}
              style={{
                padding: '9px 12px',
                borderBottom: '1px solid rgba(232,228,216,0.03)',
                borderLeft: isActive ? '2px solid #9C9CF6' : '2px solid transparent',
                background: isActive ? '#15151F' : 'transparent',
                cursor: 'pointer',
                transition: 'background 0.1s',
                paddingLeft: isActive ? 10 : 12,
              }}
              onMouseEnter={(e) => { if (!isActive) (e.currentTarget as HTMLDivElement).style.background = 'rgba(232,228,216,0.025)' }}
              onMouseLeave={(e) => { if (!isActive) (e.currentTarget as HTMLDivElement).style.background = 'transparent' }}
            >
              <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 2 }}>
                <span style={{ fontFamily: IN, fontWeight: 600, fontSize: 12, color: '#F4F4F5' }}>{info.ticker}</span>
                <span style={{ fontFamily: CN, fontSize: 11, color: '#F4F4F5' }}>
                  {market ? market.price.toFixed(decimals) : '—'}
                </span>
              </div>
              <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                <span style={{ fontFamily: IN, fontSize: 10, color: '#69697A' }}>USDC</span>
                <span style={{ fontFamily: IN, fontSize: 10, color: change !== undefined && change >= 0 ? '#2ED67B' : '#FF5C67' }}>
                  {change === undefined ? '—' : `${change >= 0 ? '+' : ''}${change.toFixed(2)}%`}
                </span>
              </div>
            </div>
          )
        })}
      </div>
    </div>
  )
}

function OrderEntryPanel({
  pair,
  bids,
  asks,
  onToast,
}: {
  pair: string
  bids: { price: string; quantity: string }[]
  asks: { price: string; quantity: string }[]
  onToast: (msg: string, v: 'success' | 'error') => void
}) {
  const { address, isConnected, isAuthenticated, connect } = useAuth()
  const [orderType, setOrderType] = useState<OrderEntryType>('limit')
  const [side, setSide] = useState<OrderSide>('buy')
  const [price, setPrice] = useState('')
  const [size, setSize] = useState('')
  const [postOnly, setPostOnly] = useState(false)
  const [submitting, setSubmitting] = useState(false)
  const [balances, setBalances] = useState<{ base: string; quote: string }>({ base: '0', quote: '0' })

  const info = getMarketInfo(pair)

  const bestBid = bids[0]?.price ?? null
  const bestAsk = asks[0]?.price ?? null
  const midPrice = bestBid && bestAsk
    ? ((parseFloat(bestBid) + parseFloat(bestAsk)) / 2).toFixed(4)
    : null

  useEffect(() => {
    if (!address || !isAuthenticated) return
    const doFetch = () => {
      getBalances(address).then((res) => {
        if (res.ok && res.data) {
          const baseB = res.data.find((b) => b.asset.toUpperCase() === info.ticker.toUpperCase())
          const quoteB = res.data.find((b) => b.asset.toUpperCase() === info.quoteTicker.toUpperCase())
          setBalances({
            base: baseB ? parseFloat(baseB.available).toFixed(4) : '0',
            quote: quoteB ? parseFloat(quoteB.available).toFixed(2) : '0',
          })
        }
      }).catch(() => {})
    }
    doFetch()
    const interval = setInterval(doFetch, 10_000)
    return () => clearInterval(interval)
  }, [address, isAuthenticated, info.ticker, info.quoteTicker])

  const availableBalance = side === 'buy' ? balances.quote : balances.base
  const availableAsset = side === 'buy' ? info.quoteTicker : info.ticker

  const total = useMemo(() => {
    const p = parseFloat(price)
    const s = parseFloat(size)
    if (!isNaN(p) && p > 0 && !isNaN(s) && s > 0) return (p * s).toFixed(4)
    return null
  }, [price, size])

  const handlePctClick = useCallback((pct: number) => {
    const bal = parseFloat(availableBalance)
    if (isNaN(bal) || bal <= 0) return
    if (side === 'buy') {
      const p = parseFloat(price) || parseFloat(midPrice ?? '0')
      if (p > 0) setSize(((bal * pct) / p).toFixed(4))
    } else {
      setSize((bal * pct).toFixed(4))
    }
  }, [availableBalance, side, price, midPrice])

  const handleSubmit = useCallback(async () => {
    if (!address || !isConnected || !isAuthenticated || !size) return
    if ((orderType === 'limit' || orderType === 'stop') && !price) return
    setSubmitting(true)
    const priceRaw = orderType !== 'market' ? Math.round(parseFloat(price) * 1_000_000) : 0
    const sizeRaw = Math.round(parseFloat(size) * 1_000_000)
    const nonce = Date.now()
    try {
      const sig = await signOrder({ market: pair, side, price: priceRaw, quantity: sizeRaw, nonce, address })
      const body: PostOrderBody = {
        market: pair,
        side,
        order_type: orderType === 'stop' ? 'limit' : orderType,
        price: priceRaw,
        quantity: sizeRaw,
        nonce,
        address,
        signature: sig,
      }
      const res = await postOrder(body)
      if (res.ok) {
        onToast('Order placed', 'success')
        setSize('')
        if (orderType !== 'market') setPrice('')
      } else {
        onToast(res.error ?? 'Order failed', 'error')
      }
    } catch (err) {
      if (err instanceof Error && (err.message.includes('rejected') || err.message.includes('denied') || err.message.includes('cancelled'))) {
        onToast('Signature rejected', 'error')
      } else {
        onToast(err instanceof Error ? err.message : 'Signing failed', 'error')
      }
    } finally {
      setSubmitting(false)
    }
  }, [address, isAuthenticated, isConnected, pair, orderType, price, size, side, onToast])

  return (
    <div className="terminal-order-ticket" style={{ display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', borderBottom: '1px solid rgba(232,228,216,0.07)', flexShrink: 0 }}>
        {(['limit', 'market', 'stop'] as OrderEntryType[]).map((t) => (
          <button
            key={t}
            onClick={() => setOrderType(t)}
            style={{
              padding: '11px 0',
              background: 'transparent',
              border: 'none',
              borderBottom: orderType === t ? '2px solid #E8E4D8' : '2px solid transparent',
              color: orderType === t ? '#F4F4F5' : '#69697A',
              fontFamily: IN,
              fontSize: 11,
              textTransform: 'uppercase',
              letterSpacing: '0.1em',
              cursor: 'pointer',
              textAlign: 'center',
              borderRadius: 0,
            }}
          >
            {t}
          </button>
        ))}
      </div>

      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 1, margin: '12px 12px 0', flexShrink: 0 }}>
        {(['buy', 'sell'] as OrderSide[]).map((s) => (
          <button
            key={s}
            onClick={() => setSide(s)}
            style={{
              padding: 11,
              textAlign: 'center',
              fontFamily: IN,
              fontWeight: 600,
              fontSize: 12,
              textTransform: 'uppercase',
              letterSpacing: '0.1em',
              cursor: 'pointer',
              borderRadius: 0,
              border: side === s
                ? s === 'buy' ? '1px solid rgba(46,214,123,0.45)' : '1px solid rgba(255,92,103,0.45)'
                : '1px solid #242431',
              background: side === s
                ? s === 'buy' ? 'rgba(46,214,123,0.14)' : 'rgba(255,92,103,0.14)'
                : '#101018',
              color: side === s
                ? s === 'buy' ? '#2ED67B' : '#FF5C67'
                : '#777789',
            }}
          >
            {s}
          </button>
        ))}
      </div>

      <div style={{ padding: 14, flex: 1, display: 'flex', flexDirection: 'column', gap: 14, overflowY: 'auto' }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', paddingBottom: 8, borderBottom: '1px solid rgba(232,228,216,0.05)' }}>
          <span style={{ fontFamily: IN, fontSize: 10, textTransform: 'uppercase', letterSpacing: '0.12em', color: '#69697A' }}>Available</span>
          <span style={{ fontFamily: CN, fontSize: 12, color: '#D6D6DF' }}>{availableBalance} {availableAsset}</span>
        </div>

        {(orderType === 'limit' || orderType === 'stop') && (
          <div>
            <div style={{ marginBottom: 4 }}>
              <span style={{ fontFamily: IN, fontSize: 10, textTransform: 'uppercase', letterSpacing: '0.1em', color: '#777789' }}>
                PRICE (USDC)
              </span>
            </div>
            <input
              type="number"
              value={price}
              onChange={(e) => setPrice(e.target.value)}
              placeholder="0.00"
              style={{ width: '100%', background: '#101018', border: '1px solid #2B2B38', color: '#F4F4F5', fontFamily: CN, fontSize: 14, padding: '11px 12px', borderRadius: 7, outline: 'none', boxSizing: 'border-box' }}
              onFocus={(e) => { e.currentTarget.style.borderColor = 'rgba(232,228,216,0.2)' }}
              onBlur={(e) => { e.currentTarget.style.borderColor = 'rgba(232,228,216,0.08)' }}
            />
            <div style={{ display: 'flex', gap: 3, marginTop: 4 }}>
              {[
                { label: 'BID', val: bestBid },
                { label: 'MID', val: midPrice },
                { label: 'ASK', val: bestAsk },
              ].map(({ label, val }) => (
                <button
                  key={label}
                  onClick={() => val && setPrice(parseFloat(val).toFixed(4))}
                  style={{ flex: 1, fontFamily: IN, fontSize: 10, color: '#8B8B9C', padding: '5px 7px', border: '1px solid #242431', background: '#0D0D13', cursor: 'pointer', borderRadius: 5 }}
                  onMouseEnter={(e) => { e.currentTarget.style.color = '#E8E4D8'; e.currentTarget.style.borderColor = 'rgba(232,228,216,0.15)' }}
                  onMouseLeave={(e) => { e.currentTarget.style.color = 'rgba(232,228,216,0.25)'; e.currentTarget.style.borderColor = 'rgba(232,228,216,0.06)' }}
                >
                  {label}
                </button>
              ))}
            </div>
          </div>
        )}

        <div>
          <span style={{ fontFamily: IN, fontSize: 10, textTransform: 'uppercase', letterSpacing: '0.1em', color: '#777789', display: 'block', marginBottom: 5 }}>
            SIZE ({info.ticker})
          </span>
          <input
            type="number"
            value={size}
            onChange={(e) => setSize(e.target.value)}
            placeholder="0.0000"
            style={{ width: '100%', background: '#101018', border: '1px solid #2B2B38', color: '#F4F4F5', fontFamily: CN, fontSize: 14, padding: '11px 12px', borderRadius: 7, outline: 'none', boxSizing: 'border-box' }}
            onFocus={(e) => { e.currentTarget.style.borderColor = 'rgba(232,228,216,0.2)' }}
            onBlur={(e) => { e.currentTarget.style.borderColor = 'rgba(232,228,216,0.08)' }}
          />
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr 1fr', gap: 3, marginTop: 4 }}>
            {[{ label: '25%', pct: 0.25 }, { label: '50%', pct: 0.5 }, { label: '75%', pct: 0.75 }, { label: 'MAX', pct: 1 }].map(({ label, pct }) => (
              <button
                key={label}
                onClick={() => handlePctClick(pct)}
                style={{ fontFamily: IN, fontSize: 10, padding: 6, border: '1px solid #242431', color: '#8B8B9C', background: '#0D0D13', textAlign: 'center', cursor: 'pointer', borderRadius: 5 }}
                onMouseEnter={(e) => { e.currentTarget.style.color = '#E8E4D8'; e.currentTarget.style.borderColor = 'rgba(232,228,216,0.15)' }}
                onMouseLeave={(e) => { e.currentTarget.style.color = 'rgba(232,228,216,0.25)'; e.currentTarget.style.borderColor = 'rgba(232,228,216,0.06)' }}
              >
                {label}
              </button>
            ))}
          </div>
        </div>

        {orderType === 'limit' && (
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <div
              role="checkbox"
              aria-checked={postOnly}
              tabIndex={0}
              onClick={() => setPostOnly(!postOnly)}
              onKeyDown={(event) => { if (event.key === 'Enter' || event.key === ' ') setPostOnly(!postOnly) }}
              style={{ width: 18, height: 18, border: '1px solid #3A3A49', borderRadius: 4, background: postOnly ? '#9C9CF6' : 'transparent', display: 'flex', alignItems: 'center', justifyContent: 'center', cursor: 'pointer', flexShrink: 0 }}
            >
              {postOnly && <span style={{ color: '#E8E4D8', fontSize: 10, lineHeight: 1 }}>✓</span>}
            </div>
            <span style={{ fontFamily: IN, fontSize: 12, color: '#A1A1AE' }}>Post-only</span>
          </div>
        )}

        <div style={{ marginTop: 'auto', paddingTop: 10, borderTop: '1px solid rgba(232,228,216,0.06)', display: 'flex', flexDirection: 'column', gap: 5 }}>
          {[
            { label: 'TOTAL', value: total ? `${total} ${info.quoteTicker}` : '—' },
            { label: 'FEE', value: '0.00 USDC' },
          ].map(({ label, value }) => (
            <div key={label} style={{ display: 'flex', justifyContent: 'space-between' }}>
              <span style={{ fontFamily: IN, fontSize: 10, textTransform: 'uppercase', letterSpacing: '0.1em', color: '#69697A' }}>{label}</span>
              <span style={{ fontFamily: CN, fontSize: 12, color: '#D6D6DF' }}>{value}</span>
            </div>
          ))}
        </div>
      </div>

      <button
        onClick={isConnected && isAuthenticated ? handleSubmit : connect}
        disabled={isConnected && isAuthenticated && submitting}
        style={{
          margin: '0 14px 14px',
          padding: 13,
          fontFamily: IN,
          fontWeight: 600,
          fontSize: 12,
          textTransform: 'uppercase',
          letterSpacing: '0.14em',
          borderRadius: 7,
          border: 'none',
          cursor: 'pointer',
          background: !isConnected || !isAuthenticated
            ? '#20202A'
            : side === 'buy'
            ? '#159A5A'
            : '#D84752',
          color: !isConnected || !isAuthenticated ? '#777789' : 'white',
          flexShrink: 0,
          opacity: isConnected && isAuthenticated && submitting ? 0.6 : 1,
        }}
      >
        {!isConnected
          ? 'CONNECT WALLET'
          : !isAuthenticated
            ? 'AUTHENTICATE WALLET'
            : submitting
              ? '...'
              : side === 'buy'
                ? `BUY ${info.ticker}`
                : `SELL ${info.ticker}`}
      </button>
    </div>
  )
}

function MobileTerminal({
  pair,
  reference,
  sourceLive,
  bids,
  asks,
  loading,
  onToast,
}: {
  pair: string
  reference: ReferenceMarket | undefined
  sourceLive: boolean | null
  bids: { price: string; quantity: string }[]
  asks: { price: string; quantity: string }[]
  loading: boolean
  onToast: (msg: string, v: 'success' | 'error') => void
}) {
  const [panel, setPanel] = useState<'trade' | 'book'>('trade')

  return (
    <div className="mobile-terminal">
      <TopBar pair={pair} reference={reference} sourceLive={sourceLive} />
      <div className="mobile-terminal__chart">
        <ChartColumn pair={pair} onToast={onToast} />
      </div>
      <div className="mobile-terminal__tabs" role="tablist" aria-label="Trading panel">
        <button type="button" role="tab" aria-selected={panel === 'trade'} onClick={() => setPanel('trade')}>Trade</button>
        <button type="button" role="tab" aria-selected={panel === 'book'} onClick={() => setPanel('book')}>Order book</button>
      </div>
      <div className="mobile-terminal__panel">
        {panel === 'trade'
          ? <OrderEntryPanel pair={pair} bids={bids} asks={asks} onToast={onToast} />
          : <OrderBookPanel bids={bids} asks={asks} pair={pair} loading={loading} />}
      </div>
    </div>
  )
}

export default function TradingPage(props: { params: Promise<{ pair: string }> }) {
  const params = use(props.params);
  const pair = decodeURIComponent(params.pair)
  const { toasts, addToast } = useToasts()
  const [isMobile, setIsMobile] = useState(false)

  const [bids, setBids] = useState<{ price: string; quantity: string }[]>([])
  const [asks, setAsks] = useState<{ price: string; quantity: string }[]>([])
  const [bookLoaded, setBookLoaded] = useState(false)
  const [referenceMarkets, setReferenceMarkets] = useState<ReferenceMarket[]>([])
  const [referenceSourceLive, setReferenceSourceLive] = useState<boolean | null>(null)

  const reference = useMemo(() => referenceMarkets.find((market) => market.market === pair), [referenceMarkets, pair])

  useEffect(() => {
    velaWs.connect()
    const unsub = velaWs.subscribe(`orderbook:${pair}`, (data) => {
      const d = data as { bids?: [string, string][]; asks?: [string, string][] }
      if (d.bids !== undefined && d.asks !== undefined) {
        setBids(d.bids.map(([price, quantity]) => ({ price, quantity })))
        setAsks(d.asks.map(([price, quantity]) => ({ price, quantity })))
        setBookLoaded(true)
      }
    })
    return () => unsub()
  }, [pair])

  useEffect(() => {
    let cancelled = false
    const refreshReferenceMarkets = async () => {
      try {
        const snapshot = await fetchReferenceMarkets()
        if (!cancelled) {
          setReferenceMarkets(snapshot.markets)
          setReferenceSourceLive(true)
        }
      } catch {
        if (!cancelled) setReferenceSourceLive(false)
      }
    }
    void refreshReferenceMarkets()
    const interval = setInterval(() => void refreshReferenceMarkets(), 2_000)
    return () => {
      cancelled = true
      clearInterval(interval)
    }
  }, [])

  useEffect(() => {
    const check = () => setIsMobile(window.innerWidth < 700)
    check()
    window.addEventListener('resize', check)
    return () => window.removeEventListener('resize', check)
  }, [])

  if (isMobile) {
    return (
      <>
        <MobileTerminal
          pair={pair}
          reference={reference}
          sourceLive={referenceSourceLive}
          bids={bids}
          asks={asks}
          loading={!bookLoaded}
          onToast={addToast}
        />
        <ToastContainer toasts={toasts} />
      </>
    )
  }

  return (
    <>
      <style>{`
        * { box-sizing: border-box; }
        input[type=number]::-webkit-inner-spin-button,
        input[type=number]::-webkit-outer-spin-button { -webkit-appearance: none; }
        input[type=number] { -moz-appearance: textfield; }
        ::-webkit-scrollbar { width: 4px; }
        ::-webkit-scrollbar-track { background: transparent; }
        ::-webkit-scrollbar-thumb { background: rgba(232,228,216,0.1); }
      `}</style>

      <div className="trade-terminal" style={{ display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
        <TopBar pair={pair} reference={reference} sourceLive={referenceSourceLive} />

        <div className="trade-terminal__grid">
          <MarketSelectorColumn currentPair={pair} markets={referenceMarkets} />
          <ChartColumn pair={pair} onToast={addToast} />
          <OrderBookPanel bids={bids} asks={asks} pair={pair} loading={!bookLoaded} />
          <OrderEntryPanel pair={pair} bids={bids} asks={asks} onToast={addToast} />
        </div>
      </div>

      <ToastContainer toasts={toasts} />
    </>
  )
}
