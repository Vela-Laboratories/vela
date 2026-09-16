import {
  REFERENCE_QUOTE,
  REFERENCE_SOURCE,
  referenceSymbolForMarket,
} from '@/lib/reference-market'

const BINANCE_MARKET_DATA_URL = 'https://data-api.binance.vision'
const TIMEFRAMES: Record<string, string> = {
  '1m': '1m',
  '5m': '5m',
  '15m': '15m',
  '1H': '1h',
  '4H': '4h',
  '1D': '1d',
}

type BinanceKline = [number, string, string, string, string, string, number, ...unknown[]]

function finiteNumber(value: string | number): number | null {
  const number = Number(value)
  return Number.isFinite(number) ? number : null
}

export async function GET(request: Request) {
  const { searchParams } = new URL(request.url)
  const market = searchParams.get('market')?.toUpperCase() ?? ''
  const timeframe = searchParams.get('timeframe') ?? '1H'
  const requestedLimit = Number(searchParams.get('limit') ?? 200)
  const limit = Number.isInteger(requestedLimit) ? Math.min(500, Math.max(2, requestedLimit)) : 200
  const symbol = referenceSymbolForMarket(market)
  const interval = TIMEFRAMES[timeframe]

  if (!symbol || !interval) {
    return Response.json({ error: 'Unsupported market or timeframe.' }, { status: 400 })
  }

  const url = new URL('/api/v3/klines', BINANCE_MARKET_DATA_URL)
  url.searchParams.set('symbol', symbol)
  url.searchParams.set('interval', interval)
  url.searchParams.set('limit', String(limit))

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

    const payload = await upstream.json() as BinanceKline[]
    const candles = payload.flatMap((row) => {
      const time = finiteNumber(row[0])
      const open = finiteNumber(row[1])
      const high = finiteNumber(row[2])
      const low = finiteNumber(row[3])
      const close = finiteNumber(row[4])
      const volume = finiteNumber(row[5])
      const closeTime = finiteNumber(row[6])
      if (time === null || open === null || high === null || low === null || close === null || volume === null || closeTime === null) return []
      return [{ time: Math.floor(time / 1_000), open, high, low, close, volume, closeTime }]
    })

    if (candles.length < 2) {
      return Response.json({ error: 'Reference market provider returned insufficient candle data.' }, { status: 502 })
    }

    return Response.json(
      {
        source: REFERENCE_SOURCE,
        quoteAsset: REFERENCE_QUOTE,
        market,
        symbol,
        timeframe,
        observedAt: Date.now(),
        candles,
      },
      { headers: { 'Cache-Control': 'public, s-maxage=2, stale-while-revalidate=8' } },
    )
  } catch (error) {
    return Response.json(
      { error: 'Reference market provider is unavailable.', detail: error instanceof Error ? error.message : 'Unknown error' },
      { status: 502 },
    )
  }
}
