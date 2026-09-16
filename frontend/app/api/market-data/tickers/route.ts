import {
  REFERENCE_MARKETS,
  REFERENCE_QUOTE,
  REFERENCE_SOURCE,
  referenceMarketForSymbol,
} from '@/lib/reference-market'

const BINANCE_MARKET_DATA_URL = 'https://data-api.binance.vision'

interface BinanceMiniTicker {
  symbol: string
  openPrice: string
  highPrice: string
  lowPrice: string
  lastPrice: string
  volume: string
  quoteVolume: string
  closeTime: number
}

function finiteNumber(value: string | number): number | null {
  const number = Number(value)
  return Number.isFinite(number) ? number : null
}

export async function GET() {
  const symbols = JSON.stringify(REFERENCE_MARKETS.map((entry) => entry.symbol))
  const url = new URL('/api/v3/ticker/24hr', BINANCE_MARKET_DATA_URL)
  url.searchParams.set('symbols', symbols)
  url.searchParams.set('type', 'MINI')

  try {
    const upstream = await fetch(url, {
      cache: 'no-store',
      signal: AbortSignal.timeout(6_000),
    })
    if (!upstream.ok) {
      return Response.json(
        { error: 'Reference market provider rejected the request.', status: upstream.status },
        { status: 502 },
      )
    }

    const payload = await upstream.json() as BinanceMiniTicker[]
    const observedAt = Date.now()
    const markets = payload.flatMap((ticker) => {
      const market = referenceMarketForSymbol(ticker.symbol)
      const price = finiteNumber(ticker.lastPrice)
      const open24h = finiteNumber(ticker.openPrice)
      const high24h = finiteNumber(ticker.highPrice)
      const low24h = finiteNumber(ticker.lowPrice)
      const volume24h = finiteNumber(ticker.volume)
      const quoteVolume24h = finiteNumber(ticker.quoteVolume)
      if (!market || price === null || open24h === null || high24h === null || low24h === null || volume24h === null || quoteVolume24h === null) return []
      return [{
        market,
        symbol: ticker.symbol,
        price,
        open24h,
        high24h,
        low24h,
        change24h: open24h > 0 ? ((price - open24h) / open24h) * 100 : 0,
        volume24h,
        quoteVolume24h,
        observedAt: finiteNumber(ticker.closeTime) ?? observedAt,
      }]
    })

    if (markets.length !== REFERENCE_MARKETS.length) {
      return Response.json(
        { error: 'Reference market provider returned an incomplete snapshot.' },
        { status: 502 },
      )
    }

    return Response.json(
      { source: REFERENCE_SOURCE, quoteAsset: REFERENCE_QUOTE, observedAt, markets },
      { headers: { 'Cache-Control': 'public, s-maxage=1, stale-while-revalidate=4' } },
    )
  } catch (error) {
    return Response.json(
      { error: 'Reference market provider is unavailable.', detail: error instanceof Error ? error.message : 'Unknown error' },
      { status: 502 },
    )
  }
}
