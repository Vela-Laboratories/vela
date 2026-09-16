export const REFERENCE_MARKETS = [
  { market: 'BTC-USDC', symbol: 'BTCUSDC' },
  { market: 'ETH-USDC', symbol: 'ETHUSDC' },
  { market: 'SOL-USDC', symbol: 'SOLUSDC' },
  { market: 'AVAX-USDC', symbol: 'AVAXUSDC' },
  { market: 'LINK-USDC', symbol: 'LINKUSDC' },
  { market: 'ARB-USDC', symbol: 'ARBUSDC' },
  { market: 'OP-USDC', symbol: 'OPUSDC' },
  { market: 'UNI-USDC', symbol: 'UNIUSDC' },
  { market: 'AAVE-USDC', symbol: 'AAVEUSDC' },
  // Binance migrated MATIC spot markets to POL. Vela keeps its legacy
  // market identifier while displaying the current POL/USDC reference.
  { market: 'MATIC-USDC', symbol: 'POLUSDC' },
  { market: 'DOGE-USDC', symbol: 'DOGEUSDC' },
  { market: 'PEPE-USDC', symbol: 'PEPEUSDC' },
  { market: 'WIF-USDC', symbol: 'WIFUSDC' },
  { market: 'JUP-USDC', symbol: 'JUPUSDC' },
  { market: 'PENDLE-USDC', symbol: 'PENDLEUSDC' },
  { market: 'EIGEN-USDC', symbol: 'EIGENUSDC' },
] as const

export type ReferenceMarketId = (typeof REFERENCE_MARKETS)[number]['market']

export const REFERENCE_SOURCE = 'Binance Spot'
export const REFERENCE_QUOTE = 'USDC'

export function referenceSymbolForMarket(market: string): string | null {
  return REFERENCE_MARKETS.find((entry) => entry.market === market)?.symbol ?? null
}

export function referenceMarketForSymbol(symbol: string): string | null {
  return REFERENCE_MARKETS.find((entry) => entry.symbol === symbol)?.market ?? null
}
