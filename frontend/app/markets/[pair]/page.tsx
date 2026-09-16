'use client'

import { use, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import Link from 'next/link'
import Skeleton from '@/components/ui/Skeleton'
import TokenIcon from '@/components/TokenIcon'
import type { IChartApi, ISeriesApi, CandlestickData, HistogramData, Time } from 'lightweight-charts'
import { cancelOrder, getAccountFeeTier, getAccountFills, getBalances, getFeeSchedule, getOrders, getPublicTrades, postOrder, type AccountFill, type CancelOrderBody, type Order, type PostOrderBody, type PublicTrade } from '@/lib/api'
import { signCancel, signOrder } from '@/lib/signing'
import { useAuth } from '@/lib/auth'
import { velaWs } from '@/lib/ws'
import { ORDERED_PAIRS, getMarketInfo, getPriceDecimals } from '@/lib/markets'
import { fetchReferenceCandles, fetchReferenceMarkets, type ReferenceCandle, type ReferenceMarket } from '@/lib/market-data'

type OrderSide = 'buy' | 'sell'
type OrderEntryType = 'limit' | 'market'
type Timeframe = '1m' | '5m' | '15m' | '1H' | '4H' | '1D'
type Range = '1D' | '1W' | '1M' | '3M' | '1Y'
type DockTab = 'orders' | 'trades' | 'fills' | 'book'
type LiveMode = 'reference' | 'unavailable' | null
type BookLevel = { price: string; quantity: string }
type Toast = { id: number; message: string; variant: 'success' | 'error' }
type FeeState = { makerBps: number; takerBps: number; tier: number }

const TIMEFRAMES: Timeframe[] = ['1m', '5m', '15m', '1H', '4H', '1D']
const RANGES: Range[] = ['1D', '1W', '1M', '3M', '1Y']
const RANGE_LIMITS: Record<Range, number> = { '1D': 96, '1W': 168, '1M': 240, '3M': 360, '1Y': 500 }

function fmtPrice(value: string | number | undefined, decimals = 2) {
  if (value === undefined || value === '') return '—'
  const number = typeof value === 'string' ? Number.parseFloat(value) : value
  return Number.isFinite(number) ? number.toFixed(decimals) : '—'
}

function fmtSize(value: string | number | undefined, ticker = 'BTC') {
  if (value === undefined || value === '') return '—'
  const number = typeof value === 'string' ? Number.parseFloat(value) : value
  if (!Number.isFinite(number)) return '—'
  return ['BTC', 'ETH'].includes(ticker.toUpperCase()) ? number.toFixed(4) : number.toFixed(2)
}

function fmtCompact(value: number | undefined) {
  return value === undefined || !Number.isFinite(value) ? '—' : new Intl.NumberFormat('en-US', { notation: 'compact', maximumFractionDigits: 2 }).format(value)
}

function fmtTime(timestamp: number) {
  return new Date(timestamp < 1e12 ? timestamp * 1000 : timestamp).toLocaleTimeString('en-US', { hour12: false, hour: '2-digit', minute: '2-digit', second: '2-digit' })
}

function Icon({ name, size = 16 }: { name: 'search' | 'star' | 'collapse' | 'fit' | 'camera' | 'expand' | 'settings' | 'info'; size?: number }) {
  const paths = {
    search: <><circle cx="11" cy="11" r="6" /><path d="m16 16 4 4" /></>,
    star: <path d="m12 3 2.7 5.5 6.1.9-4.4 4.3 1 6.1-5.4-2.9-5.4 2.9 1-6.1L3.2 9.4l6.1-.9Z" />,
    collapse: <path d="m14.5 5-7 7 7 7" />,
    fit: <><path d="M8 4H4v4M16 4h4v4M8 20H4v-4M16 20h4v-4" /><path d="m4 8 5-5m11 5-5-5M4 16l5 5m11-5-5 5" /></>,
    camera: <><path d="M4 8h3l1.5-2h7L17 8h3v11H4Z" /><circle cx="12" cy="13" r="3" /></>,
    expand: <><path d="M8 3H3v5M16 3h5v5M8 21H3v-5M16 21h5v-5" /></>,
    settings: <><circle cx="12" cy="12" r="3" /><path d="M19 12a7 7 0 0 0-.1-1l2-1.5-2-3.4-2.4 1a8 8 0 0 0-1.8-1L14.4 3h-4.8l-.4 3a8 8 0 0 0-1.8 1L5 6.1 3 9.5 5.1 11a7 7 0 0 0 0 2L3 14.5 5 18l2.4-1a8 8 0 0 0 1.8 1l.4 3h4.8l.4-3a8 8 0 0 0 1.8-1l2.4 1 2-3.5-2.1-1.5a7 7 0 0 0 .1-1Z" /></>,
    info: <><circle cx="12" cy="12" r="9" /><path d="M12 11v6m0-10v.1" /></>,
  }
  return <svg aria-hidden="true" width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">{paths[name]}</svg>
}

function useToasts() {
  const [toasts, setToasts] = useState<Toast[]>([])
  const id = useRef(0)
  const addToast = useCallback((message: string, variant: Toast['variant']) => {
    const next = ++id.current
    setToasts((current) => [...current, { id: next, message, variant }])
    window.setTimeout(() => setToasts((current) => current.filter((toast) => toast.id !== next)), 3000)
  }, [])
  return { toasts, addToast }
}

function ToastContainer({ toasts }: { toasts: Toast[] }) {
  return <div className="terminal-toasts" aria-live="polite">{toasts.map((toast) => <div key={toast.id} className={`terminal-toast terminal-toast--${toast.variant}`}>{toast.message}</div>)}</div>
}

function MarketHeader({ pair, reference, sourceLive, bids, asks }: { pair: string; reference?: ReferenceMarket; sourceLive: boolean | null; bids: BookLevel[]; asks: BookLevel[] }) {
  const info = getMarketInfo(pair)
  const decimals = getPriceDecimals(pair)
  const change = reference?.change24h
  const bid = bids[0] ? Number(bids[0].price) : undefined
  const ask = asks[0] ? Number(asks[0].price) : undefined
  const spread = bid !== undefined && ask !== undefined ? ask - bid : undefined
  const stats = [
    ['24h change', change === undefined ? '—' : `${change >= 0 ? '+' : ''}${change.toFixed(2)}%`, change !== undefined && change >= 0 ? 'positive' : 'negative'],
    ['24h high', reference ? reference.high24h.toFixed(decimals) : '—', ''],
    ['24h low', reference ? reference.low24h.toFixed(decimals) : '—', ''],
    ['24h volume', reference ? `${fmtCompact(reference.quoteVolume24h)} ${info.quoteTicker}` : '—', ''],
    ['Vela spread', spread !== undefined && spread >= 0 ? spread.toFixed(decimals) : '—', ''],
  ]
  return <header className="terminal-market-header">
    <div className="terminal-market-identity"><TokenIcon ticker={info.ticker} size="lg" /><div><div className="terminal-pair">{info.ticker}<span>/ {info.quoteTicker}</span></div><div className="terminal-product">Spot · Vela Sepolia testnet</div></div></div>
    <div className="terminal-reference-price"><span>Binance Spot reference</span>{reference ? <strong>{reference.price.toFixed(decimals)}</strong> : sourceLive === null ? <Skeleton className="w-28 h-6" /> : <strong>—</strong>}</div>
    <div className="terminal-header-stats">{stats.map(([label, value, tone]) => <div key={label} className="terminal-header-stat"><span>{label}</span><strong className={tone}>{value}</strong></div>)}</div>
    <div className={`terminal-source-state ${sourceLive === false ? 'is-offline' : ''}`}><i />{sourceLive === false ? 'Reference unavailable' : 'Reference live'}</div>
  </header>
}

function MarketRail({ currentPair, markets, onCollapse }: { currentPair: string; markets: ReferenceMarket[]; onCollapse: () => void }) {
  const [query, setQuery] = useState('')
  const [filter, setFilter] = useState<'all' | 'movers' | 'volume'>('all')
  const [favorites, setFavorites] = useState(['BTC-USDC', 'ETH-USDC'])
  const byId = useMemo(() => Object.fromEntries(markets.map((market) => [market.market, market])), [markets])
  const pairs = useMemo(() => {
    const normalized = query.trim().toUpperCase()
    const visible = ORDERED_PAIRS.filter((pair) => !normalized || pair.includes(normalized))
    if (filter === 'movers') return [...visible].sort((a, b) => Math.abs(byId[b]?.change24h ?? 0) - Math.abs(byId[a]?.change24h ?? 0))
    if (filter === 'volume') return [...visible].sort((a, b) => (byId[b]?.quoteVolume24h ?? 0) - (byId[a]?.quoteVolume24h ?? 0))
    return visible
  }, [byId, filter, query])
  const toggleFavorite = (event: React.MouseEvent, pair: string) => {
    event.preventDefault(); event.stopPropagation()
    setFavorites((current) => current.includes(pair) ? current.filter((item) => item !== pair) : [...current, pair])
  }
  return <aside className="terminal-market-rail">
    <div className="terminal-rail-heading"><strong>Discover</strong><button type="button" aria-label="Collapse market rail" onClick={onCollapse}><Icon name="collapse" /></button></div>
    <nav className="terminal-rail-tabs" aria-label="Discovery"><span className="is-active">Markets</span><Link href="/history">Activity</Link><Link href="/leaderboard">Leaders</Link></nav>
    <label className="terminal-rail-search"><Icon name="search" /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search assets" /></label>
    <div className="terminal-rail-filters">{(['all', 'movers', 'volume'] as const).map((item) => <button key={item} type="button" className={filter === item ? 'is-active' : ''} onClick={() => setFilter(item)}>{item === 'all' ? 'Watchlist' : item}</button>)}</div>
    <div className="terminal-market-list" role="list">
      {pairs.map((pair) => {
        const market = byId[pair]; const info = getMarketInfo(pair); const change = market?.change24h; const favorite = favorites.includes(pair)
        return <div key={pair} className={`terminal-market-row${pair === currentPair ? ' is-active' : ''}`}>
          <button type="button" aria-label={`${favorite ? 'Remove' : 'Add'} ${info.ticker} ${favorite ? 'from' : 'to'} watchlist`} className={favorite ? 'is-favorite' : ''} onClick={(event) => toggleFavorite(event, pair)}><Icon name="star" size={13} /></button>
          <Link href={`/markets/${pair}`} aria-current={pair === currentPair ? 'page' : undefined} aria-label={`${info.ticker} ${info.quoteTicker}, ${market ? market.price.toFixed(getPriceDecimals(pair)) : 'price unavailable'}, ${change === undefined ? 'change unavailable' : `${change.toFixed(2)} percent`}`}>
            <TokenIcon ticker={info.ticker} size="sm" />
            <span className="terminal-market-name"><strong>{info.ticker}</strong><small>{info.quoteTicker}</small></span>
            <span className="terminal-market-values"><strong>{market ? market.price.toFixed(getPriceDecimals(pair)) : '—'}</strong><small className={change !== undefined && change >= 0 ? 'positive' : 'negative'}>{change === undefined ? '—' : `${change >= 0 ? '+' : ''}${change.toFixed(2)}%`}</small></span>
          </Link>
        </div>
      })}
      {pairs.length === 0 ? <p className="terminal-rail-empty">No matching markets</p> : null}
    </div>
  </aside>
}

function ChartWorkspace({ pair, reference, onLiveChange }: { pair: string; reference?: ReferenceMarket; onLiveChange: (mode: LiveMode) => void }) {
  const container = useRef<HTMLDivElement>(null); const shell = useRef<HTMLElement>(null); const chart = useRef<IChartApi | null>(null)
  const candleSeries = useRef<ISeriesApi<'Candlestick', Time> | null>(null); const volumeSeries = useRef<ISeriesApi<'Histogram', Time> | null>(null)
  const [timeframe, setTimeframe] = useState<Timeframe>('1H'); const [range, setRange] = useState<Range>('1W'); const [logarithmic, setLogarithmic] = useState(false)
  const [ready, setReady] = useState(false); const [hasCandles, setHasCandles] = useState(false); const [ohlc, setOhlc] = useState<ReferenceCandle | null>(null); const [observedAt, setObservedAt] = useState<number | null>(null)

  useEffect(() => {
    if (!container.current) return
    const node = container.current; let cancelled = false; let dispose: (() => void) | undefined
    void import('lightweight-charts').then(({ createChart, CandlestickSeries, HistogramSeries }) => {
      if (cancelled) return
      const instance = createChart(node, { layout: { background: { color: '#090b11' }, textColor: '#777f91', fontFamily: 'var(--font-inter-sans)', fontSize: 11 }, grid: { vertLines: { color: 'rgba(131,142,166,.07)' }, horzLines: { color: 'rgba(131,142,166,.07)' } }, crosshair: { vertLine: { color: '#4b556b', width: 1, style: 3 }, horzLine: { color: '#4b556b', width: 1, style: 3 } }, rightPriceScale: { borderVisible: false, scaleMargins: { top: .08, bottom: .23 } }, timeScale: { borderVisible: false, timeVisible: true, secondsVisible: false, rightOffset: 3 }, width: node.clientWidth, height: node.clientHeight })
      const candles = instance.addSeries(CandlestickSeries, { upColor: '#20c98a', downColor: '#ef6a5b', borderVisible: false, wickUpColor: '#20c98a', wickDownColor: '#ef6a5b', priceLineColor: '#d4a24c' })
      const volume = instance.addSeries(HistogramSeries, { priceFormat: { type: 'volume' }, priceScaleId: 'volume' })
      instance.priceScale('volume').applyOptions({ scaleMargins: { top: .82, bottom: 0 } })
      instance.subscribeCrosshairMove((event) => { const point = event.seriesData.get(candles) as CandlestickData<Time> | undefined; if (point) setOhlc({ ...point, time: Number(point.time), volume: 0, closeTime: Number(point.time) }) })
      chart.current = instance; candleSeries.current = candles; volumeSeries.current = volume; setReady(true)
      const observer = new ResizeObserver(() => instance.applyOptions({ width: node.clientWidth, height: node.clientHeight })); observer.observe(node)
      dispose = () => { observer.disconnect(); instance.remove(); chart.current = null; candleSeries.current = null; volumeSeries.current = null }
    })
    return () => { cancelled = true; dispose?.() }
  }, [])

  useEffect(() => {
    if (!ready) return
    let cancelled = false; let timer: number | undefined
    const load = async (initial: boolean) => {
      try {
        const snapshot = await fetchReferenceCandles(pair, timeframe, initial ? RANGE_LIMITS[range] : 2)
        if (cancelled || snapshot.candles.length === 0) return
        const candles: CandlestickData<Time>[] = snapshot.candles.map((item) => ({ time: item.time as Time, open: item.open, high: item.high, low: item.low, close: item.close }))
        const volumes: HistogramData<Time>[] = snapshot.candles.map((item) => ({ time: item.time as Time, value: item.volume, color: item.close >= item.open ? 'rgba(32,201,138,.28)' : 'rgba(239,106,91,.28)' }))
        if (initial) { candleSeries.current?.setData(candles); volumeSeries.current?.setData(volumes); chart.current?.timeScale().fitContent() } else { candleSeries.current?.update(candles.at(-1)!); volumeSeries.current?.update(volumes.at(-1)!) }
        setOhlc(snapshot.candles.at(-1) ?? null); setObservedAt(snapshot.observedAt); setHasCandles(true); onLiveChange('reference')
      } catch { if (!cancelled) { setHasCandles(false); onLiveChange('unavailable') } }
    }
    void load(true).then(() => { if (!cancelled) timer = window.setInterval(() => void load(false), 3000) })
    return () => { cancelled = true; if (timer) window.clearInterval(timer) }
  }, [onLiveChange, pair, range, ready, timeframe])

  const toggleScale = async () => { const { PriceScaleMode } = await import('lightweight-charts'); const next = !logarithmic; setLogarithmic(next); chart.current?.priceScale('right').applyOptions({ mode: next ? PriceScaleMode.Logarithmic : PriceScaleMode.Normal }) }
  const takeScreenshot = () => { const canvas = chart.current?.takeScreenshot(); if (!canvas) return; const anchor = document.createElement('a'); anchor.download = `vela-${pair}-${timeframe}.png`; anchor.href = canvas.toDataURL('image/png'); anchor.click() }
  const toggleFullscreen = () => document.fullscreenElement ? void document.exitFullscreen() : void shell.current?.requestFullscreen()
  return <section className="terminal-chart-workspace" ref={shell}>
    <div className="terminal-chart-toolbar"><div className="terminal-toolbar-group"><span>Interval</span>{TIMEFRAMES.map((item) => <button type="button" key={item} className={timeframe === item ? 'is-active' : ''} onClick={() => setTimeframe(item)}>{item}</button>)}</div><div className="terminal-toolbar-group terminal-toolbar-ranges"><span>Range</span>{RANGES.map((item) => <button type="button" key={item} className={range === item ? 'is-active' : ''} onClick={() => setRange(item)}>{item}</button>)}</div><div className="terminal-chart-actions"><button type="button" onClick={() => chart.current?.timeScale().fitContent()} aria-label="Fit chart"><Icon name="fit" /></button><button type="button" className={logarithmic ? 'is-active' : ''} onClick={() => void toggleScale()} aria-label="Toggle logarithmic scale">log</button><button type="button" onClick={takeScreenshot} aria-label="Download chart screenshot"><Icon name="camera" /></button><button type="button" onClick={toggleFullscreen} aria-label="Toggle fullscreen chart"><Icon name="expand" /></button></div></div>
    <div className="terminal-chart-legend"><strong>{pair.replace('-', ' / ')}</strong><span>Binance Spot reference</span>{ohlc ? <><i>O {fmtPrice(ohlc.open, getPriceDecimals(pair))}</i><i>H {fmtPrice(ohlc.high, getPriceDecimals(pair))}</i><i>L {fmtPrice(ohlc.low, getPriceDecimals(pair))}</i><i>C {fmtPrice(ohlc.close, getPriceDecimals(pair))}</i><i>Vol {fmtCompact(ohlc.volume)}</i></> : null}<time>{observedAt ? `Updated ${fmtTime(observedAt)}` : 'Connecting…'}</time></div>
    <div className="terminal-chart-canvas"><div ref={container} />{!hasCandles ? <div className="terminal-chart-empty">Waiting for Binance reference candles</div> : null}</div>
    <div className="terminal-chart-foot"><span><i className="reference-dot" />Reference price only</span><span>Execution occurs on the synthetic Vela testnet book</span><strong>{reference ? fmtPrice(reference.price, getPriceDecimals(pair)) : '—'}</strong></div>
  </section>
}

function OrderBook({ bids, asks, pair, loading }: { bids: BookLevel[]; asks: BookLevel[]; pair: string; loading: boolean }) {
  const info = getMarketInfo(pair); const decimals = getPriceDecimals(pair); const topAsks = useMemo(() => asks.slice(0, 8).reverse(), [asks]); const topBids = useMemo(() => bids.slice(0, 8), [bids])
  const max = useMemo(() => Math.max(1, ...[...topAsks, ...topBids].map((level) => Number(level.quantity))), [topAsks, topBids]); const spread = bids[0] && asks[0] ? Number(asks[0].price) - Number(bids[0].price) : undefined
  const rows = (levels: BookLevel[], side: 'ask' | 'bid') => levels.map((level) => <div key={`${side}-${level.price}`} className={`terminal-book-row terminal-book-row--${side}`}><i style={{ width: `${Math.min(100, Number(level.quantity) / max * 100)}%` }} /><span>{fmtPrice(level.price, decimals)}</span><span>{fmtSize(level.quantity, info.ticker)}</span></div>)
  return <div className="terminal-book"><div className="terminal-table-head"><span>Price ({info.quoteTicker})</span><span>Size ({info.ticker})</span></div>{loading ? <div className="terminal-book-loading">Loading Vela book…</div> : <>{rows(topAsks, 'ask')}<div className="terminal-book-spread"><span>Spread</span><strong>{spread !== undefined && spread >= 0 ? spread.toFixed(decimals) : '—'}</strong><em>Synthetic testnet</em></div>{rows(topBids, 'bid')}</>}</div>
}

function EmptyState({ message }: { message: string }) { return <div className="terminal-empty"><Icon name="info" /><span>{message}</span></div> }

function ActivityDock({ pair, bids, asks, bookLoading, onToast }: { pair: string; bids: BookLevel[]; asks: BookLevel[]; bookLoading: boolean; onToast: (message: string, variant: Toast['variant']) => void }) {
  const { address, isConnected, isAuthenticated } = useAuth(); const [tab, setTab] = useState<DockTab>('trades'); const [orders, setOrders] = useState<Order[]>([]); const [trades, setTrades] = useState<PublicTrade[]>([]); const [fills, setFills] = useState<AccountFill[]>([]); const info = getMarketInfo(pair)
  const refresh = useCallback(() => {
    void getPublicTrades(pair).then((response) => { if (response.ok && response.data) setTrades(response.data.slice(0, 25)) }).catch(() => {})
    if (address && isAuthenticated) void Promise.all([getOrders(address), getAccountFills(address)]).then(([ordersResponse, fillsResponse]) => { if (ordersResponse.ok && ordersResponse.data) setOrders(ordersResponse.data); if (fillsResponse.ok && fillsResponse.data) setFills(fillsResponse.data.filter((fill) => fill.market === pair)) }).catch(() => {})
  }, [address, isAuthenticated, pair])
  useEffect(() => { refresh(); const timer = window.setInterval(refresh, 5000); return () => window.clearInterval(timer) }, [refresh])
  // Wall-clock nonces are generated only in this user-triggered event handler.
  // eslint-disable-next-line react-hooks/purity
  const handleCancel = async (order: Order) => { if (!address) return; try { const nonce = Date.now(); const signature = await signCancel({ order_id: order.id, nonce, address }); const body: CancelOrderBody = { order_id: order.id, nonce, address, signature }; const response = await cancelOrder(body); if (response.ok) { onToast('Order cancelled', 'success'); refresh() } else onToast(response.error ?? 'Cancel failed', 'error') } catch (error) { onToast(error instanceof Error ? error.message : 'Cancel failed', 'error') } }
  const openOrders = orders.filter((order) => (order.status === 'open' || order.status === 'partial') && order.market === pair); const privateEmpty = !isConnected || !isAuthenticated ? 'Connect and authenticate to view private account activity.' : undefined
  return <section className="terminal-activity-dock"><div className="terminal-dock-tabs" role="tablist" aria-label="Market activity">{([['orders', `Open orders${isAuthenticated ? ` (${openOrders.length})` : ''}`], ['trades', 'Recent trades'], ['fills', 'My fills'], ['book', 'Order book']] as [DockTab, string][]).map(([id, label]) => <button type="button" role="tab" aria-selected={tab === id} key={id} onClick={() => setTab(id)}>{label}</button>)}</div><div className="terminal-dock-content">
    {tab === 'book' ? <OrderBook bids={bids} asks={asks} pair={pair} loading={bookLoading} /> : null}
    {tab === 'orders' ? privateEmpty ? <EmptyState message={privateEmpty} /> : openOrders.length === 0 ? <EmptyState message="No open orders for this market." /> : <div className="terminal-table"><div className="terminal-table-head terminal-table-head--orders"><span>Time</span><span>Side</span><span>Type</span><span>Price</span><span>Size</span><span>Filled</span><span /></div>{openOrders.map((order) => <div className="terminal-table-row terminal-table-row--orders" key={order.id}><span>{fmtTime(order.timestamp)}</span><span className={order.side === 'buy' ? 'positive' : 'negative'}>{order.side}</span><span>{order.order_type}</span><span>{fmtPrice(order.price, getPriceDecimals(pair))}</span><span>{fmtSize(order.quantity, info.ticker)}</span><span>{fmtSize(order.filled_quantity, info.ticker)}</span><button type="button" onClick={() => void handleCancel(order)}>Cancel</button></div>)}</div> : null}
    {tab === 'trades' ? trades.length === 0 ? <EmptyState message="No Vela testnet fills have printed for this market yet." /> : <div className="terminal-table"><div className="terminal-table-head terminal-table-head--trades"><span>Time</span><span>Price</span><span>Size</span><span>Liquidity</span></div>{trades.map((trade) => <div className="terminal-table-row terminal-table-row--trades" key={trade.id}><span>{fmtTime(trade.timestamp)}</span><span className={trade.side.toLowerCase() === 'buy' ? 'positive' : 'negative'}>{fmtPrice(trade.price / 1_000_000, getPriceDecimals(pair))}</span><span>{fmtSize(trade.quantity / 1_000_000, info.ticker)}</span><span>{trade.synthetic ? 'Synthetic' : 'Participant'}</span></div>)}</div> : null}
    {tab === 'fills' ? privateEmpty ? <EmptyState message={privateEmpty} /> : fills.length === 0 ? <EmptyState message="No account fills for this market." /> : <div className="terminal-table"><div className="terminal-table-head terminal-table-head--fills"><span>Time</span><span>Side</span><span>Role</span><span>Price</span><span>Size</span></div>{fills.map((fill) => <div className="terminal-table-row terminal-table-row--fills" key={fill.id}><span>{fmtTime(fill.timestamp)}</span><span className={fill.side === 'buy' ? 'positive' : 'negative'}>{fill.side}</span><span>{fill.role}</span><span>{fmtPrice(fill.price, getPriceDecimals(pair))}</span><span>{fmtSize(fill.quantity, info.ticker)}</span></div>)}</div> : null}
  </div></section>
}

function OrderTicket({ pair, bids, asks, fee, onToast }: { pair: string; bids: BookLevel[]; asks: BookLevel[]; fee: FeeState; onToast: (message: string, variant: Toast['variant']) => void }) {
  const { address, isConnected, isAuthenticated, connect } = useAuth(); const [side, setSide] = useState<OrderSide>('buy'); const [orderType, setOrderType] = useState<OrderEntryType>('limit'); const [price, setPrice] = useState(''); const [size, setSize] = useState(''); const [advanced, setAdvanced] = useState(false); const [submitting, setSubmitting] = useState(false); const [balances, setBalances] = useState({ base: '0', quote: '0' })
  const info = getMarketInfo(pair); const bestBid = bids[0]?.price; const bestAsk = asks[0]?.price; const mid = bestBid && bestAsk ? ((Number(bestBid) + Number(bestAsk)) / 2).toString() : undefined
  useEffect(() => { if (!address || !isAuthenticated) return; const refresh = () => void getBalances(address).then((response) => { if (response.ok && response.data) setBalances({ base: response.data.find((item) => item.asset.toUpperCase() === info.ticker)?.available ?? '0', quote: response.data.find((item) => item.asset.toUpperCase() === info.quoteTicker)?.available ?? '0' }) }).catch(() => {}); refresh(); const timer = window.setInterval(refresh, 10000); return () => window.clearInterval(timer) }, [address, info.quoteTicker, info.ticker, isAuthenticated])
  const executablePrice = orderType === 'market' ? (side === 'buy' ? bestAsk : bestBid) : price; const total = Number(executablePrice) * Number(size); const feeBps = orderType === 'market' ? fee.takerBps : fee.makerBps; const feeAmount = Number.isFinite(total) ? total * feeBps / 10000 : 0; const available = side === 'buy' ? balances.quote : balances.base; const availableAsset = side === 'buy' ? info.quoteTicker : info.ticker
  const setPercentage = (percentage: number) => { const balance = Number(available); if (!(balance > 0)) return; if (side === 'buy') { const px = Number(executablePrice); if (px > 0) setSize((balance * percentage / px).toFixed(6)) } else setSize((balance * percentage).toFixed(6)) }
  const submit = async () => { if (!address || !isConnected || !isAuthenticated || !(Number(size) > 0) || (orderType === 'limit' && !(Number(price) > 0))) return; setSubmitting(true); try { const priceRaw = orderType === 'market' ? 0 : Math.round(Number(price) * 1_000_000); const quantity = Math.round(Number(size) * 1_000_000); const nonce = Date.now(); const signature = await signOrder({ market: pair, side, price: priceRaw, quantity, nonce, address }); const body: PostOrderBody = { market: pair, side, order_type: orderType, price: priceRaw, quantity, nonce, address, signature }; const response = await postOrder(body); if (response.ok) { onToast('Order placed', 'success'); setSize(''); if (orderType === 'limit') setPrice('') } else onToast(response.error ?? 'Order rejected', 'error') } catch (error) { onToast(error instanceof Error ? error.message : 'Signing failed', 'error') } finally { setSubmitting(false) } }
  const actionLabel = !isConnected ? 'Connect wallet' : !isAuthenticated ? 'Authenticate wallet' : submitting ? 'Submitting…' : `${side === 'buy' ? 'Buy' : 'Sell'} ${info.ticker}`
  return <section className="terminal-order-ticket"><div className="terminal-side-switch">{(['buy', 'sell'] as const).map((item) => <button type="button" key={item} className={`${item}${side === item ? ' is-active' : ''}`} onClick={() => setSide(item)}>{item}</button>)}</div><div className="terminal-order-types" role="tablist" aria-label="Order type">{(['limit', 'market'] as const).map((item) => <button type="button" role="tab" key={item} aria-selected={orderType === item} onClick={() => setOrderType(item)}>{item}</button>)}</div><div className="terminal-ticket-body">
    <div className="terminal-available"><span>Available</span><strong>{fmtSize(available, availableAsset)} {availableAsset}</strong></div>
    {orderType === 'limit' ? <label className="terminal-input"><span>Price <i>{info.quoteTicker}</i></span><input type="number" inputMode="decimal" value={price} onChange={(event) => setPrice(event.target.value)} placeholder="0.00" /><div>{([['Bid', bestBid], ['Mid', mid], ['Ask', bestAsk]] as [string, string | undefined][]).map(([label, value]) => <button type="button" key={label} disabled={!value} onClick={() => value && setPrice(value)}>{label}</button>)}</div></label> : <div className="terminal-market-execution"><Icon name="info" /><span>Market orders execute against the synthetic Vela testnet book. The preview uses the current best {side === 'buy' ? 'ask' : 'bid'}.</span></div>}
    <label className="terminal-input terminal-input--size"><span>Amount <i>{info.ticker}</i></span><input type="number" inputMode="decimal" value={size} onChange={(event) => setSize(event.target.value)} placeholder="0.000000" /></label>
    <div className="terminal-percentages">{([[.1, '10%'], [.25, '25%'], [.5, '50%'], [1, 'Max']] as [number, string][]).map(([value, label]) => <button type="button" key={label} onClick={() => setPercentage(value)}>{label}</button>)}</div>
    <button type="button" className="terminal-advanced-toggle" aria-expanded={advanced} onClick={() => setAdvanced(!advanced)}><span><Icon name="settings" />Advanced</span><i>{advanced ? '−' : '+'}</i></button>
    {advanced ? <div className="terminal-advanced-note"><strong>Time in force: Good ’til cancelled</strong><span>Post-only, stop triggers, and custom slippage remain hidden until those constraints are bound and enforced by the order API.</span></div> : null}
    <dl className="terminal-order-preview"><div><dt>Estimated total</dt><dd>{Number.isFinite(total) && total > 0 ? `${total.toFixed(4)} ${info.quoteTicker}` : '—'}</dd></div><div><dt>Fee · tier {fee.tier} ({feeBps} bps)</dt><dd>{Number.isFinite(feeAmount) && total > 0 ? `${feeAmount.toFixed(4)} ${info.quoteTicker}` : '—'}</dd></div><div><dt>Estimated debit</dt><dd>{Number.isFinite(total) && total > 0 ? `${(total + Math.max(0, feeAmount)).toFixed(4)} ${info.quoteTicker}` : '—'}</dd></div></dl>
    <button type="button" className={`terminal-submit terminal-submit--${side}`} disabled={isConnected && isAuthenticated && submitting} onClick={isConnected && isAuthenticated ? () => void submit() : connect}>{actionLabel}</button>
  </div></section>
}

function ContextCards({ pair, reference, sourceLive, bids, asks, fee }: { pair: string; reference?: ReferenceMarket; sourceLive: boolean | null; bids: BookLevel[]; asks: BookLevel[]; fee: FeeState }) {
  const { address, isAuthenticated } = useAuth(); const info = getMarketInfo(pair); const decimals = getPriceDecimals(pair); const bid = bids[0] ? Number(bids[0].price) : undefined; const ask = asks[0] ? Number(asks[0].price) : undefined; const spread = bid !== undefined && ask !== undefined ? ask - bid : undefined
  return <div className="terminal-context-cards"><article><header><span>Market context</span><small className={sourceLive === false ? 'is-offline' : ''}><i />{sourceLive === false ? 'Unavailable' : 'Live reference'}</small></header><p>{info.ticker}/{info.quoteTicker} uses Binance Spot for charting and reference only. Orders execute on Vela’s separate synthetic Sepolia testnet book.</p><dl><div><dt>Reference</dt><dd>{reference ? fmtPrice(reference.price, decimals) : '—'}</dd></div><div><dt>Observed</dt><dd>{reference ? fmtTime(reference.observedAt) : '—'}</dd></div><div><dt>Vela best bid</dt><dd>{bid !== undefined ? fmtPrice(bid, decimals) : '—'}</dd></div><div><dt>Vela best ask</dt><dd>{ask !== undefined ? fmtPrice(ask, decimals) : '—'}</dd></div><div><dt>Spread</dt><dd>{spread !== undefined && spread >= 0 ? fmtPrice(spread, decimals) : '—'}</dd></div><div><dt>Network</dt><dd>Sepolia</dd></div></dl></article><article><header><span>Fee schedule</span><Link href="/transparency">Details</Link></header><p>{isAuthenticated && address ? `Your current fee tier is ${fee.tier}.` : 'Connect and authenticate to load your account tier.'} Limit previews use maker fees; market previews use taker fees.</p><dl><div><dt>Maker</dt><dd>{fee.makerBps} bps</dd></div><div><dt>Taker</dt><dd>{fee.takerBps} bps</dd></div></dl></article></div>
}

function ContextRail(props: { pair: string; reference?: ReferenceMarket; sourceLive: boolean | null; bids: BookLevel[]; asks: BookLevel[]; fee: FeeState; onToast: (message: string, variant: Toast['variant']) => void }) {
  return <aside className="terminal-context-rail"><OrderTicket pair={props.pair} bids={props.bids} asks={props.asks} fee={props.fee} onToast={props.onToast} /><ContextCards pair={props.pair} reference={props.reference} sourceLive={props.sourceLive} bids={props.bids} asks={props.asks} fee={props.fee} /></aside>
}

function TerminalFooter({ markets, sourceLive }: { markets: ReferenceMarket[]; sourceLive: boolean | null }) {
  return <footer className="terminal-footer"><div>{markets.slice(0, 4).map((market) => <span key={market.market}><TokenIcon ticker={getMarketInfo(market.market).ticker} size="sm" /><strong>{market.price.toFixed(getPriceDecimals(market.market))}</strong><i className={market.change24h >= 0 ? 'positive' : 'negative'}>{market.change24h >= 0 ? '+' : ''}{market.change24h.toFixed(2)}%</i></span>)}</div><nav><span className={sourceLive === false ? 'is-offline' : ''}><i />{sourceLive === false ? 'Reference degraded' : 'Binance reference live'}</span><b>Sepolia testnet</b><Link href="/transparency">Transparency</Link><a href="https://monolithsystematicllc.mintlify.app/introduction/what-is-vela" target="_blank" rel="noreferrer">Help</a></nav></footer>
}

function MobileTerminal(props: { pair: string; reference?: ReferenceMarket; sourceLive: boolean | null; bids: BookLevel[]; asks: BookLevel[]; loading: boolean; fee: FeeState; onToast: (message: string, variant: Toast['variant']) => void }) {
  const [panel, setPanel] = useState<'chart' | 'trade' | 'book'>('chart'); const ignoreLive = useCallback(() => {}, [])
  return <div className="mobile-terminal"><MarketHeader pair={props.pair} reference={props.reference} sourceLive={props.sourceLive} bids={props.bids} asks={props.asks} /><div className="mobile-terminal__tabs" role="tablist">{(['chart', 'trade', 'book'] as const).map((item) => <button type="button" role="tab" aria-selected={panel === item} key={item} onClick={() => setPanel(item)}>{item}</button>)}</div><div className="mobile-terminal__panel">{panel === 'chart' ? <ChartWorkspace pair={props.pair} reference={props.reference} onLiveChange={ignoreLive} /> : panel === 'trade' ? <OrderTicket pair={props.pair} bids={props.bids} asks={props.asks} fee={props.fee} onToast={props.onToast} /> : <OrderBook bids={props.bids} asks={props.asks} pair={props.pair} loading={props.loading} />}</div></div>
}

export default function TradingPage({ params }: { params: Promise<{ pair: string }> }) {
  const { pair: rawPair } = use(params); const pair = decodeURIComponent(rawPair); const { toasts, addToast } = useToasts(); const { address, isAuthenticated } = useAuth()
  const [bids, setBids] = useState<BookLevel[]>([]); const [asks, setAsks] = useState<BookLevel[]>([]); const [bookLoaded, setBookLoaded] = useState(false); const [markets, setMarkets] = useState<ReferenceMarket[]>([]); const [sourceLive, setSourceLive] = useState<boolean | null>(null); const [chartLive, setChartLive] = useState<LiveMode>(null); const [fee, setFee] = useState<FeeState>({ makerBps: 0, takerBps: 10, tier: 0 }); const [isMobile, setIsMobile] = useState(false); const [railCollapsed, setRailCollapsed] = useState(false)
  const reference = useMemo(() => markets.find((market) => market.market === pair), [markets, pair])
  useEffect(() => { velaWs.connect(); return velaWs.subscribe(`orderbook:${pair}`, (payload) => { const data = payload as { bids?: [string, string][]; asks?: [string, string][] }; if (data.bids && data.asks) { setBids(data.bids.map(([price, quantity]) => ({ price, quantity }))); setAsks(data.asks.map(([price, quantity]) => ({ price, quantity }))); setBookLoaded(true) } }) }, [pair])
  useEffect(() => { let cancelled = false; const refresh = async () => { try { const snapshot = await fetchReferenceMarkets(); if (!cancelled) { setMarkets(snapshot.markets); setSourceLive(true) } } catch { if (!cancelled) setSourceLive(false) } }; void refresh(); const timer = window.setInterval(() => void refresh(), 5000); return () => { cancelled = true; window.clearInterval(timer) } }, [])
  useEffect(() => { const refresh = () => setIsMobile(window.innerWidth < 760); refresh(); window.addEventListener('resize', refresh); return () => window.removeEventListener('resize', refresh) }, [])
  useEffect(() => { const request = address && isAuthenticated ? getAccountFeeTier(address).then((response) => response.ok && response.data ? { makerBps: response.data.maker_bps, takerBps: response.data.taker_bps, tier: response.data.cached_tier } : null) : getFeeSchedule().then((response) => { const tier = response.data?.tiers[0]; return response.ok && tier ? { makerBps: tier.maker_bps, takerBps: tier.taker_bps, tier: tier.tier } : null }); void request.then((next) => { if (next) setFee(next) }).catch(() => {}) }, [address, isAuthenticated])
  const handleChartLive = useCallback((mode: LiveMode) => setChartLive(mode), []); const effectiveLive = sourceLive === false || chartLive === 'unavailable' ? false : sourceLive
  if (isMobile) return <><MobileTerminal pair={pair} reference={reference} sourceLive={effectiveLive} bids={bids} asks={asks} loading={!bookLoaded} fee={fee} onToast={addToast} /><ToastContainer toasts={toasts} /></>
  return <><div className="trade-terminal"><MarketHeader pair={pair} reference={reference} sourceLive={effectiveLive} bids={bids} asks={asks} /><div className={`trade-terminal__grid${railCollapsed ? ' is-rail-collapsed' : ''}`}>{railCollapsed ? <button type="button" className="terminal-rail-restore" onClick={() => setRailCollapsed(false)} aria-label="Open market discovery"><Icon name="collapse" /></button> : <MarketRail currentPair={pair} markets={markets} onCollapse={() => setRailCollapsed(true)} />}<main className="terminal-workspace"><ChartWorkspace pair={pair} reference={reference} onLiveChange={handleChartLive} /><ActivityDock pair={pair} bids={bids} asks={asks} bookLoading={!bookLoaded} onToast={addToast} /></main><ContextRail pair={pair} reference={reference} sourceLive={effectiveLive} bids={bids} asks={asks} fee={fee} onToast={addToast} /></div><TerminalFooter markets={markets} sourceLive={effectiveLive} /></div><ToastContainer toasts={toasts} /></>
}
