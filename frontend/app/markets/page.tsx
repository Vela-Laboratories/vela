'use client'

import { useState, useEffect, useCallback } from 'react'
import { useRouter } from 'next/navigation'
import { getBook, type BookLevel } from '@/lib/api'
import { ORDERED_PAIRS, getMarketInfo } from '@/lib/markets'
import { fetchReferenceCandles, fetchReferenceMarkets, type ReferenceMarket } from '@/lib/market-data'
import Skeleton from '@/components/ui/Skeleton'

interface SparklineBar {
  height: number
  up: boolean
}

async function fetchSparkline(marketId: string): Promise<SparklineBar[]> {
  try {
    const data = await fetchReferenceCandles(marketId, '1H', 24)
    const candles = data.candles
    if (candles.length < 3) return []
    const closes = candles.map((c) => c.close)
    const minClose = Math.min(...closes)
    const maxClose = Math.max(...closes)
    const range = maxClose - minClose
    return candles.map((c) => ({
      height: range > 0 ? ((c.close - minClose) / range) * 80 + 20 : 50,
      up: c.close >= c.open,
    }))
  } catch {
    return []
  }
}

const PF = "'Playfair Display', serif"
const IN = 'Inter, sans-serif'
const CN = "'Courier New', monospace"

function formatPrice(price: number): string {
  if (price >= 1000) return price.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 })
  if (price >= 1) return price.toFixed(2)
  if (price < 0.01) return price.toFixed(8)
  return price.toFixed(4)
}

function fmtBookPrice(v: string): string {
  const n = parseFloat(v)
  return isNaN(n) ? '—' : formatPrice(n)
}

function formatVolume(value: number | undefined): string {
  if (value === undefined || !Number.isFinite(value)) return '—'
  return new Intl.NumberFormat('en-US', { notation: 'compact', maximumFractionDigits: 2 }).format(value)
}

function MarketCard({
  pair,
  market,
  book,
  sparkline,
  onClick,
}: {
  pair: string
  market: ReferenceMarket | undefined
  book: { bids: BookLevel[]; asks: BookLevel[] } | undefined
  sparkline: SparklineBar[]
  onClick: () => void
}) {
  const [hovered, setHovered] = useState(false)
  const info = getMarketInfo(pair)
  const change = market?.change24h
  const changeStr = change === undefined ? '—' : `${change >= 0 ? '+' : ''}${change.toFixed(2)}%`
  const isPositive = (change ?? 0) >= 0
  const bars = sparkline

  const topBids = book?.bids.slice(0, 3) ?? []
  const topAsks = book?.asks.slice(0, 3) ?? []
  const priceValue = market?.price ?? 0
  const priceFontSize = priceValue > 10000 ? 22 : 28

  return (
    <div
      onClick={onClick}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      style={{
        background: hovered ? '#111110' : '#0C0C0C',
        cursor: 'pointer',
        position: 'relative',
        overflow: 'hidden',
        display: 'flex',
        flexDirection: 'column',
        transition: 'background 0.15s',
      }}
      className="p-4 sm:p-[24px] lg:p-[24px]"
    >
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', marginBottom: 12 }}>
        <span style={{ fontFamily: IN, fontSize: 10, letterSpacing: '0.14em', textTransform: 'uppercase', color: 'rgba(232,228,216,0.3)' }}>
          {info.ticker} / USDC
        </span>
        <span style={{ fontFamily: IN, fontSize: 7.5, letterSpacing: '0.1em', textTransform: 'uppercase', border: '1px solid rgba(232,228,216,0.12)', padding: '2px 6px', color: 'rgba(232,228,216,0.3)' }}>
          BINANCE SPOT
        </span>
      </div>

      <div style={{ fontFamily: PF, fontWeight: 900, fontSize: priceFontSize, color: '#E8E4D8', lineHeight: 1, marginBottom: 4 }}>
        {market ? formatPrice(market.price) : '—'}
      </div>

      <div style={{ display: 'flex', gap: 10, alignItems: 'center', marginBottom: 14 }}>
        <span style={{ fontFamily: IN, fontWeight: 500, fontSize: 11, color: isPositive ? '#6B8A5A' : '#CC3333' }}>
          {changeStr}
        </span>
        <span style={{ fontFamily: IN, fontSize: 10, color: 'rgba(232,228,216,0.2)' }}>
          Vol {formatVolume(market?.quoteVolume24h)} USDC
        </span>
      </div>

      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8 }}>
        <div>
          <div style={{ fontFamily: IN, fontSize: 8, textTransform: 'uppercase', color: 'rgba(107,138,90,0.7)', letterSpacing: '0.12em', marginBottom: 4 }}>
            VELA BIDS
          </div>
          {topBids.length === 0 ? (
            <span style={{ fontFamily: CN, fontSize: 10, color: 'rgba(107,138,90,0.3)' }}>—</span>
          ) : (
            topBids.map((lvl, i) => (
              <div key={i} style={{ display: 'flex', justifyContent: 'space-between' }}>
                <span style={{ fontFamily: CN, fontSize: 11, color: '#6B8A5A' }}>{fmtBookPrice(lvl.price)}</span>
                <span style={{ fontFamily: CN, fontSize: 10, color: 'rgba(232,228,216,0.35)' }}>{parseFloat(lvl.quantity).toFixed(2)}</span>
              </div>
            ))
          )}
        </div>
        <div>
          <div style={{ fontFamily: IN, fontSize: 8, textTransform: 'uppercase', color: 'rgba(204,51,51,0.7)', letterSpacing: '0.12em', marginBottom: 4 }}>
            VELA ASKS
          </div>
          {topAsks.length === 0 ? (
            <span style={{ fontFamily: CN, fontSize: 10, color: 'rgba(204,51,51,0.3)' }}>—</span>
          ) : (
            topAsks.map((lvl, i) => (
              <div key={i} style={{ display: 'flex', justifyContent: 'space-between' }}>
                <span style={{ fontFamily: CN, fontSize: 11, color: '#CC3333' }}>{fmtBookPrice(lvl.price)}</span>
                <span style={{ fontFamily: CN, fontSize: 10, color: 'rgba(232,228,216,0.35)' }}>{parseFloat(lvl.quantity).toFixed(2)}</span>
              </div>
            ))
          )}
        </div>
      </div>

      <div style={{ marginTop: 14, height: 28, display: 'flex', alignItems: 'flex-end', gap: 2 }} aria-label="24 hour Binance reference trend">
        {bars.length === 0 ? <span style={{ fontFamily: IN, fontSize: 9, color: '#69697A' }}>Market data unavailable</span> : bars.map((bar, i) => (
          <div
            key={i}
            style={{ flex: 1, height: `${bar.height}%`, background: bar.up ? '#6B8A5A' : '#CC3333' }}
          />
        ))}
      </div>
    </div>
  )
}

function SkeletonCard() {
  return (
    <div style={{
      background: '#111110',
      border: '1px solid rgba(232,228,216,0.04)',
      display: 'flex',
      flexDirection: 'column',
      padding: '24px 28px',
    }}>
      <Skeleton className="w-24 h-3" />
      <Skeleton className="w-32 h-8 mt-3" />
      <Skeleton className="w-16 h-2 mt-2" />
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8, marginTop: 12 }}>
        <Skeleton className="w-full h-16" />
        <Skeleton className="w-full h-16" />
      </div>
      <Skeleton className="w-full h-5 mt-3" />
    </div>
  )
}

export default function MarketsPage() {
  const router = useRouter()
  const [markets, setMarkets] = useState<ReferenceMarket[] | null>(null)
  const [books, setBooks] = useState<Record<string, { bids: BookLevel[]; asks: BookLevel[] }>>({})
  const [sparklines, setSparklines] = useState<Record<string, SparklineBar[]>>({})

  const fetchMarkets = useCallback(async () => {
    try {
      const snapshot = await fetchReferenceMarkets()
      setMarkets(snapshot.markets)
    } catch {}
  }, [])

  const fetchBooks = useCallback(async () => {
    try {
      const results = await Promise.all(
        ORDERED_PAIRS.map((pair) => getBook(pair).then((res) => ({ pair, res })).catch(() => null))
      )
      const next: Record<string, { bids: BookLevel[]; asks: BookLevel[] }> = {}
      for (const item of results) {
        if (item && item.res.ok && item.res.data) {
          next[item.pair] = { bids: item.res.data.bids, asks: item.res.data.asks }
        }
      }
      setBooks(next)
    } catch {}
  }, [])

  const fetchSparklines = useCallback(async () => {
    try {
      const results = await Promise.all(ORDERED_PAIRS.map((pair) => fetchSparkline(pair)))
      const next: Record<string, SparklineBar[]> = {}
      ORDERED_PAIRS.forEach((pair, i) => { next[pair] = results[i] })
      setSparklines(next)
    } catch {}
  }, [])

  useEffect(() => {
    const initial = window.setTimeout(() => {
      void fetchMarkets()
      void fetchBooks()
      void fetchSparklines()
    }, 0)
    const mi = setInterval(fetchMarkets, 2_000)
    const bi = setInterval(fetchBooks, 5000)
    const si = setInterval(fetchSparklines, 5 * 60 * 1000)
    return () => {
      clearTimeout(initial)
      clearInterval(mi)
      clearInterval(bi)
      clearInterval(si)
    }
  }, [fetchMarkets, fetchBooks, fetchSparklines])

  const marketsById = Object.fromEntries((markets ?? []).map((m) => [m.market, m]))

  return (
    <>
      <style>{`* { box-sizing: border-box; } ::-webkit-scrollbar { display: none; }`}</style>
      <div style={{ background: '#0C0C0C', display: 'flex', flexDirection: 'column' }} className="min-h-[calc(100vh-96px)] lg:h-[calc(100vh-96px)] lg:overflow-hidden">

        <div style={{ flexShrink: 0, borderBottom: '1px solid rgba(232,228,216,0.07)' }} className="h-auto lg:h-[52px] px-6 lg:px-10 py-3 lg:py-0 flex items-center justify-between flex-wrap gap-2">
          <div>
            <div style={{ lineHeight: 1 }}>
              <span style={{ fontFamily: PF, fontWeight: 900, fontSize: 22, color: '#E8E4D8' }}>Markets</span>
              <span style={{ fontFamily: PF, fontStyle: 'italic', fontWeight: 400, fontSize: 22, color: 'rgba(232,228,216,0.25)' }}>{' / '}</span>
              <span style={{ fontFamily: PF, fontStyle: 'italic', fontWeight: 400, fontSize: 22, color: 'rgba(232,228,216,0.25)' }}>Live</span>
            </div>
            <div style={{ fontFamily: IN, fontSize: 9, letterSpacing: '0.12em', color: 'rgba(232,228,216,0.2)', textTransform: 'uppercase', marginTop: 4 }} className="hidden sm:block">
              16 Binance Spot references · Vela execution on Ethereum Sepolia · Updating every 2s
            </div>
          </div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
            <span style={{ display: 'inline-block', width: 5, height: 5, borderRadius: '50%', background: '#6B8A5A', flexShrink: 0 }} />
            <span style={{ fontFamily: IN, fontSize: 9, letterSpacing: '0.15em', color: 'rgba(232,228,216,0.22)', textTransform: 'uppercase' }}>
              Binance live feed
            </span>
          </div>
        </div>

        <div
          style={{ gap: 1, background: 'rgba(232,228,216,0.05)' }}
          className="flex-1 grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 lg:overflow-hidden"
        >
          {markets === null
            ? ORDERED_PAIRS.map((pair) => <SkeletonCard key={pair} />)
            : ORDERED_PAIRS.map((pair) => (
                <MarketCard
                  key={pair}
                  pair={pair}
                  market={marketsById[pair]}
                  book={books[pair]}
                  sparkline={sparklines[pair] ?? []}
                  onClick={() => router.push(`/markets/${pair}`)}
                />
              ))}
          <div style={{ background: '#0C0C0C' }} />
        </div>

        <div style={{ flexShrink: 0, borderTop: '1px solid rgba(232,228,216,0.06)' }} className="hidden lg:flex h-[44px] px-10 items-center justify-between">
          <div style={{ display: 'flex', gap: 40 }}>
            {[
              { label: 'TOTAL MARKETS', value: '16' },
              { label: 'ENGINE LATENCY', value: '0.68 μs' },
              { label: 'OPS / SEC', value: '1,430,000' },
              { label: 'NETWORK', value: 'Sepolia' },
            ].map(({ label, value }) => (
              <div key={label} style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
                <span style={{ fontFamily: IN, fontSize: 7.5, textTransform: 'uppercase', letterSpacing: '0.18em', color: 'rgba(232,228,216,0.18)' }}>{label}</span>
                <span style={{ fontFamily: CN, fontSize: 12, color: 'rgba(232,228,216,0.55)' }}>{value}</span>
              </div>
            ))}
          </div>
          <span style={{ fontFamily: IN, fontSize: 9, color: 'rgba(232,228,216,0.15)', letterSpacing: '0.1em' }}>
            Click any market to trade →
          </span>
        </div>

      </div>
    </>
  )
}
