import { describe, expect, it } from 'vitest'
import { normalizeWsEnvelope } from './ws'

describe('normalizeWsEnvelope', () => {
  it('maps an order-book snapshot envelope to the validated client message', () => {
    expect(normalizeWsEnvelope(
      { type: 'snapshot', timestamp: 123 },
      'orderbook:ETH-USDC',
      { bids: [['3200.00', '1.25']], asks: [['3201.00', '0.75']] },
    )).toEqual({
      type: 'book_snapshot',
      market: 'ETH-USDC',
      timestamp: 123,
      bids: [['3200.00', '1.25']],
      asks: [['3201.00', '0.75']],
    })
  })

  it('preserves explicit market and event types', () => {
    expect(normalizeWsEnvelope(
      { type: 'trade', timestamp: 456 },
      'trades:BTC-USDC',
      { market_id: 'BTC-USDC', price: '75000', quantity: '0.1' },
    )).toMatchObject({ type: 'trade', market: 'BTC-USDC', timestamp: 456 })
  })
})
