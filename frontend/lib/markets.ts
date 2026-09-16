export const MARKET_DATA: Record<string, { name: string; ticker: string; quoteTicker: string }> = {
  'BTC-USDC':  { name: 'Bitcoin',   ticker: 'BTC',  quoteTicker: 'USDC' },
  'ETH-USDC':  { name: 'Ethereum',  ticker: 'ETH',  quoteTicker: 'USDC' },
  'SOL-USDC':  { name: 'Solana',    ticker: 'SOL',  quoteTicker: 'USDC' },
  'AVAX-USDC': { name: 'Avalanche', ticker: 'AVAX', quoteTicker: 'USDC' },
  'LINK-USDC': { name: 'Chainlink', ticker: 'LINK', quoteTicker: 'USDC' },
  'ARB-USDC':  { name: 'Arbitrum',  ticker: 'ARB',  quoteTicker: 'USDC' },
  'OP-USDC':   { name: 'Optimism',  ticker: 'OP',   quoteTicker: 'USDC' },
  'UNI-USDC':  { name: 'Uniswap',   ticker: 'UNI',  quoteTicker: 'USDC' },
  'AAVE-USDC': { name: 'Aave',      ticker: 'AAVE', quoteTicker: 'USDC' },
  'MATIC-USDC':{ name: 'Polygon',   ticker: 'MATIC',quoteTicker: 'USDC' },
  'DOGE-USDC':  { name: 'Dogecoin',   ticker: 'DOGE',   quoteTicker: 'USDC' },
  'PEPE-USDC':  { name: 'Pepe',       ticker: 'PEPE',   quoteTicker: 'USDC' },
  'WIF-USDC':   { name: 'dogwifhat',  ticker: 'WIF',    quoteTicker: 'USDC' },
  'JUP-USDC':   { name: 'Jupiter',    ticker: 'JUP',    quoteTicker: 'USDC' },
  'PENDLE-USDC':{ name: 'Pendle',     ticker: 'PENDLE', quoteTicker: 'USDC' },
  'EIGEN-USDC': { name: 'EigenLayer', ticker: 'EIGEN',  quoteTicker: 'USDC' },
}

export const ORDERED_PAIRS = Object.keys(MARKET_DATA)

export function getMarketInfo(pair: string) {
  return (
    MARKET_DATA[pair] ?? {
      name: pair.split('-')[0] ?? pair,
      ticker: pair.split('-')[0] ?? pair,
      quoteTicker: pair.split('-')[1] ?? 'USDC',
    }
  )
}

export function getPriceDecimals(pair: string): number {
  const ticker = pair.split('-')[0] ?? ''
  if (ticker === 'PEPE') return 8
  if (['ARB', 'OP', 'MATIC', 'DOGE', 'WIF', 'JUP', 'EIGEN'].includes(ticker)) return 4
  return 2
}
