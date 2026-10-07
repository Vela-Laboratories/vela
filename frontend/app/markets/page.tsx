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
  const topBids = book?.bids.slice(0, 3) ?? []
  const topAsks = book?.asks.slice(0, 3) ?? []

  return (
    <Link
      href={`/markets/${pair}`}
      aria-label={`Open ${info.name} ${info.quoteTicker} testnet market`}
      className="market-card dippi-discovery-card"
    >
      <div className="dippi-discovery-card__top">
        <span><TokenIcon ticker={info.ticker} />{info.name}</span>
        <i aria-hidden="true">↗</i>
      </div>
      <div className="dippi-discovery-card__pair">{info.ticker}<span> / USDC</span></div>
      <div className="dippi-discovery-card__price">
        <strong>{market ? formatPrice(market.price) : '—'}</strong>
        <span className={change === undefined ? '' : change >= 0 ? 'positive' : 'negative'}>{changeStr}</span>
      </div>
      <div className="dippi-discovery-card__meta">
        <span>24h volume</span><strong>{formatVolume(market?.quoteVolume24h)} USDC</strong>
      </div>
      <div className="dippi-discovery-card__book">
        <div><span>DEMO BIDS</span>{topBids.length === 0 ? <small>—</small> : topBids.map((level, index) => <small key={index}><b>{fmtBookPrice(level.price)}</b><em>{parseFloat(level.quantity).toFixed(2)}</em></small>)}</div>
        <div><span>DEMO ASKS</span>{topAsks.length === 0 ? <small>—</small> : topAsks.map((level, index) => <small key={index}><b>{fmtBookPrice(level.price)}</b><em>{parseFloat(level.quantity).toFixed(2)}</em></small>)}</div>
      </div>
      <div className="dippi-discovery-card__sparkline" aria-label="24 hour Binance reference trend">
        {sparkline.length === 0 ? <span>Reference trend unavailable</span> : sparkline.map((bar, index) => <i key={index} className={bar.up ? 'is-up' : 'is-down'} style={{ height: `${bar.height}%` }} />)}
      </div>
      <div className="dippi-discovery-card__foot">Binance Spot reference <span>Explore market →</span></div>
    </Link>
  )
}

function SkeletonCard() {
  return (
    <div className="market-card dippi-discovery-card dippi-discovery-card--loading">
      <Skeleton className="w-24 h-3" />
      <Skeleton className="w-32 h-8 mt-3" />
      <Skeleton className="w-16 h-2 mt-2" />
      <div className="dippi-discovery-card__book">
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
      <div style={{ display: 'flex', flexDirection: 'column' }} className="market-grid-page min-h-[calc(100vh-96px)]">

        <div className="markets-header dippi-markets-header">
          <div>
            <span className="dippi-kicker">MARKETS</span>
            <h1>Find something interesting.</h1>
            <p>Live spot price references and separate synthetic Sepolia order books.</p>
          </div>
          <span className="dippi-markets-header__source"><i /> Binance Spot reference</span>
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

        <div className="markets-footer dippi-markets-footer"><span>{ORDERED_PAIRS.length} markets</span><span>Binance Spot reference</span><span>Synthetic demo books</span><span>Sepolia testnet</span></div>

      </div>
    </>
  )
}
