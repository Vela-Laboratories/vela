export interface ReferenceMarket {
  market: string
  symbol: string
  price: number
  open24h: number
  high24h: number
  low24h: number
  change24h: number
  volume24h: number
  quoteVolume24h: number
  observedAt: number
}

export interface ReferenceMarketSnapshot {
  source: string
  quoteAsset: 'USDC'
  observedAt: number
  markets: ReferenceMarket[]
}

export interface ReferenceCandle {
  time: number
  open: number
  high: number
  low: number
  close: number
  volume: number
  closeTime: number
}

export interface ReferenceCandleSnapshot {
  source: string
  quoteAsset: 'USDC'
  market: string
  symbol: string
  timeframe: string
  observedAt: number
  candles: ReferenceCandle[]
}

export async function fetchReferenceMarkets(): Promise<ReferenceMarketSnapshot> {
  const response = await fetch('/api/market-data/tickers', { cache: 'no-store' })
  if (!response.ok) throw new Error('Live reference prices are unavailable.')
  return response.json() as Promise<ReferenceMarketSnapshot>
}

export async function fetchReferenceCandles(
  market: string,
  timeframe: string,
  limit = 200,
): Promise<ReferenceCandleSnapshot> {
  const query = new URLSearchParams({ market, timeframe, limit: String(limit) })
  const response = await fetch(`/api/market-data/candles?${query.toString()}`, { cache: 'no-store' })
  if (!response.ok) throw new Error('Live reference candles are unavailable.')
  return response.json() as Promise<ReferenceCandleSnapshot>
}
