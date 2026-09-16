import { describe, expect, it } from 'vitest'
import {
  REFERENCE_MARKETS,
  referenceMarketForSymbol,
  referenceSymbolForMarket,
} from './reference-market'

describe('reference market mapping', () => {
  it('covers every Vela market exactly once', () => {
    expect(REFERENCE_MARKETS).toHaveLength(16)
    expect(new Set(REFERENCE_MARKETS.map(({ market }) => market)).size).toBe(16)
    expect(new Set(REFERENCE_MARKETS.map(({ symbol }) => symbol)).size).toBe(16)
  })

  it('maps in both directions', () => {
    for (const { market, symbol } of REFERENCE_MARKETS) {
      expect(referenceSymbolForMarket(market)).toBe(symbol)
      expect(referenceMarketForSymbol(symbol)).toBe(market)
    }
  })

  it('uses Binance POL for Vela’s legacy MATIC market id', () => {
    expect(referenceSymbolForMarket('MATIC-USDC')).toBe('POLUSDC')
  })

  it('rejects unsupported markets and symbols', () => {
    expect(referenceSymbolForMarket('NOT-A-MARKET')).toBeNull()
    expect(referenceMarketForSymbol('NOTASYMBOL')).toBeNull()
  })
})
