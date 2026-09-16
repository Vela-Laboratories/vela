'use client'

import { useState, useEffect, useCallback } from 'react'
import Link from 'next/link'
import { getBook, type BookLevel } from '@/lib/api'
import { ORDERED_PAIRS, getMarketInfo } from '@/lib/markets'
import { fetchReferenceCandles, fetchReferenceMarkets, type ReferenceMarket } from '@/lib/market-data'
import Skeleton from '@/components/ui/Skeleton'
import TokenIcon from '@/components/TokenIcon'

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

const IN = 'Inter, sans-serif'
const CN = "var(--font-mono), 'IBM Plex Mono', monospace"

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
}: {
  pair: string
  market: ReferenceMarket | undefined
  book: { bids: BookLevel[]; asks: BookLevel[] } | undefined
  sparkline: SparklineBar[]
}) {
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
    <Link
      href={`/markets/${pair}`}
      aria-label={`Open ${info.name} ${info.quoteTicker} testnet market`}
      style={{
        position: 'relative',
        overflow: 'hidden',
        display: 'flex',
        flexDirection: 'column',
        color: 'inherit',
        textDecoration: 'none',
        transition: 'transform 0.16s ease, border-color 0.16s ease, background 0.16s ease',
      }}
      className="market-card p-4 sm:p-[24px] lg:p-[24px]"
    >
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', marginBottom: 12 }}>
        <span style={{ display: 'flex', alignItems: 'center', gap: 9, fontFamily: IN, fontSize: 11, letterSpacing: '0.08em', textTransform: 'uppercase', color: 'rgba(232,234,240,0.55)' }}>
          <TokenIcon ticker={info.ticker} /> {info.ticker} / USDC
        </span>
        <span style={{ fontFamily: IN, fontSize: 7.5, letterSpacing: '0.1em', textTransform: 'uppercase', border: '1px solid rgba(232,234,240,0.12)', padding: '2px 6px', color: 'rgba(232,234,240,0.3)' }}>
          BINANCE SPOT
        </span>
      </div>

      <div style={{ fontFamily: CN, fontWeight: 700, fontSize: priceFontSize, color: '#F6F8FF', lineHeight: 1, marginBottom: 4, letterSpacing: '-0.04em' }}>
        {market ? formatPrice(market.price) : '—'}
      </div>

      <div style={{ display: 'flex', gap: 10, alignItems: 'center', marginBottom: 14 }}>
        <span style={{ fontFamily: IN, fontWeight: 650, fontSize: 11, color: isPositive ? '#21E6A3' : '#FF5C7C' }}>
          {changeStr}
        </span>
        <span style={{ fontFamily: IN, fontSize: 10, color: 'rgba(232,234,240,0.2)' }}>
          Vol {formatVolume(market?.quoteVolume24h)} USDC
        </span>
      </div>

      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8 }}>
        <div>
          <div style={{ fontFamily: IN, fontSize: 8, textTransform: 'uppercase', color: 'rgba(107,138,90,0.7)', letterSpacing: '0.12em', marginBottom: 4 }}>
            DEMO BIDS
          </div>
          {topBids.length === 0 ? (
            <span style={{ fontFamily: CN, fontSize: 10, color: 'rgba(107,138,90,0.3)' }}>—</span>
          ) : (
            topBids.map((lvl, i) => (
              <div key={i} style={{ display: 'flex', justifyContent: 'space-between' }}>
                <span style={{ fontFamily: CN, fontSize: 11, color: '#21E6A3' }}>{fmtBookPrice(lvl.price)}</span>
                <span style={{ fontFamily: CN, fontSize: 10, color: 'rgba(232,234,240,0.35)' }}>{parseFloat(lvl.quantity).toFixed(2)}</span>
              </div>
            ))
          )}
        </div>
        <div>
          <div style={{ fontFamily: IN, fontSize: 8, textTransform: 'uppercase', color: 'rgba(204,51,51,0.7)', letterSpacing: '0.12em', marginBottom: 4 }}>
            DEMO ASKS
          </div>
          {topAsks.length === 0 ? (
            <span style={{ fontFamily: CN, fontSize: 10, color: 'rgba(204,51,51,0.3)' }}>—</span>
          ) : (
            topAsks.map((lvl, i) => (
              <div key={i} style={{ display: 'flex', justifyContent: 'space-between' }}>
                <span style={{ fontFamily: CN, fontSize: 11, color: '#FF5C7C' }}>{fmtBookPrice(lvl.price)}</span>
                <span style={{ fontFamily: CN, fontSize: 10, color: 'rgba(232,234,240,0.35)' }}>{parseFloat(lvl.quantity).toFixed(2)}</span>
              </div>
            ))
          )}
        </div>
      </div>

      <div style={{ marginTop: 14, height: 28, display: 'flex', alignItems: 'flex-end', gap: 2 }} aria-label="24 hour Binance reference trend">
        {bars.length === 0 ? <span style={{ fontFamily: IN, fontSize: 9, color: '#69697A' }}>Market data unavailable</span> : bars.map((bar, i) => (
          <div
            key={i}
            style={{ flex: 1, height: `${bar.height}%`, background: bar.up ? '#21E6A3' : '#FF5C7C', borderRadius: '2px 2px 0 0', opacity: 0.78 }}
          />
        ))}
      </div>
    </Link>
  )
}

function SkeletonCard() {
  return (
    <div style={{
      background: '#111110',
      border: '1px solid rgba(232,234,240,0.04)',
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
      <div style={{ display: 'flex', flexDirection: 'column' }} className="market-grid-page min-h-[calc(100vh-96px)]">

        <div style={{ flexShrink: 0 }} className="markets-header h-auto lg:h-[64px] px-6 lg:px-8 py-3 lg:py-0 flex items-center justify-between flex-wrap gap-2">
          <div>
            <div style={{ lineHeight: 1 }}>
              <span style={{ fontFamily: IN, fontWeight: 700, fontSize: 20, color: '#F6F8FF' }}>Explore markets</span>
              <span style={{ marginLeft: 8, fontFamily: IN, fontWeight: 650, fontSize: 9, letterSpacing: '0.08em', color: '#66c9b4' }}>· LIVE</span>
            </div>
            <div style={{ fontFamily: IN, fontSize: 9, letterSpacing: '0.12em', color: 'rgba(232,234,240,0.2)', textTransform: 'uppercase', marginTop: 4 }} className="hidden sm:block">
              Binance Spot reference data · Synthetic Vela testnet books · Updating every 2s
            </div>
          </div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
            <span style={{ display: 'inline-block', width: 5, height: 5, borderRadius: '50%', background: '#66c9b4', flexShrink: 0 }} />
            <span style={{ fontFamily: IN, fontSize: 9, letterSpacing: '0.15em', color: 'rgba(232,234,240,0.22)', textTransform: 'uppercase' }}>
              Binance live feed
            </span>
          </div>
        </div>

        <div
          className="market-card-grid flex-1 grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3"
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
                />
              ))}
        </div>

        <div style={{ flexShrink: 0 }} className="markets-footer hidden lg:flex h-[52px] px-8 items-center justify-between">
          <div style={{ display: 'flex', gap: 40 }}>
            {[
              { label: 'TOTAL MARKETS', value: '16' },
              { label: 'REFERENCE DATA', value: 'Binance Spot' },
              { label: 'ORDER BOOKS', value: 'Synthetic demo' },
              { label: 'NETWORK', value: 'Sepolia' },
            ].map(({ label, value }) => (
              <div key={label} style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
                <span style={{ fontFamily: IN, fontSize: 7.5, textTransform: 'uppercase', letterSpacing: '0.18em', color: 'rgba(232,234,240,0.18)' }}>{label}</span>
                <span style={{ fontFamily: CN, fontSize: 12, color: 'rgba(232,234,240,0.55)' }}>{value}</span>
              </div>
            ))}
          </div>
          <span style={{ fontFamily: IN, fontSize: 9, color: 'rgba(232,234,240,0.15)', letterSpacing: '0.1em' }}>
            Choose a testnet market →
          </span>
        </div>

      </div>
    </>
  )
}
