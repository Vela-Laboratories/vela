'use client'

import { useEffect, useState } from 'react'
import Link from 'next/link'
import { fetchReferenceMarkets, type ReferenceMarket } from '@/lib/market-data'
import { getMarketInfo } from '@/lib/markets'
import TokenIcon from '@/components/TokenIcon'

const FEATURED_PAIRS = ['BTC-USDC', 'SOL-USDC', 'DOGE-USDC'] as const

function formatPrice(price: number): string {
  return new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD', maximumFractionDigits: price < 1 ? 4 : 2 }).format(price)
}

export default function HomeMarkets() {
  const [markets, setMarkets] = useState<ReferenceMarket[]>([])
  const [unavailable, setUnavailable] = useState(false)

  useEffect(() => {
    let active = true
    async function refresh() {
      try {
        const snapshot = await fetchReferenceMarkets()
        if (active) { setMarkets(snapshot.markets); setUnavailable(false) }
      } catch {
        if (active) setUnavailable(true)
      }
    }
    void refresh()
    const interval = window.setInterval(refresh, 15_000)
    return () => { active = false; window.clearInterval(interval) }
  }, [])

  return <div className="dippi-market-cards">
    {FEATURED_PAIRS.map((pair) => {
      const info = getMarketInfo(pair)
      const market = markets.find((item) => item.market === pair)
      const change = market?.change24h
      return <Link key={pair} className="dippi-market-card" href={`/markets/${pair}`}>
        <div className="dippi-market-card__top"><TokenIcon ticker={info.ticker} /><span><strong>{info.name}</strong><small>{info.ticker} / USDC</small></span></div>
        <div className="dippi-market-card__bottom"><strong>{market ? formatPrice(market.price) : unavailable ? 'Unavailable' : 'Loading…'}</strong><span className={change === undefined ? '' : change >= 0 ? 'positive' : 'negative'}>{change === undefined ? '—' : `${change >= 0 ? '+' : ''}${change.toFixed(2)}%`}</span><i aria-hidden="true">↗</i></div>
      </Link>
    })}
  </div>
}
