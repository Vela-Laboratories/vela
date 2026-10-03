import { describe, expect, it } from 'vitest'
import { prepareOrder } from './order-entry'
import { orderSigningMessage } from './signing'

const base = {
  market: 'BTC-USDC',
  side: 'buy' as const,
  kind: 'limit' as const,
  limitPrice: '100',
  bestBid: '99',
  bestAsk: '101',
  size: '5',
  nonce: 42,
  address: '0x0000000000000000000000000000000000000001',
}

describe('order ticket payload', () => {
  it('uses the Rust enum names and canonical v2 signature', () => {
    const order = prepareOrder(base)
    expect(order).toMatchObject({
      side: 'Bid', order_type: 'GoodTillCanceled', price: 100_000_000,
      quantity: 5_000_000, signature_version: 2,
    })
    expect(orderSigningMessage({ ...order, client_order_id: 'client:1' })).toBe(
      '["vela:order:v2","BTC-USDC","Bid","GoodTillCanceled",100000000,5000000,42,"client:1","None",null,null]',
    )
  })

  it('sends an immediate buy at the best Vela ask and cancels the remainder', () => {
    expect(prepareOrder({ ...base, kind: 'market' })).toMatchObject({
      side: 'Bid', order_type: 'ImmediateOrCancel', price: 101_000_000,
    })
    expect(prepareOrder({ ...base, kind: 'market', side: 'sell' })).toMatchObject({
      side: 'Ask', order_type: 'ImmediateOrCancel', price: 99_000_000,
    })
    expect(() => prepareOrder({ ...base, kind: 'market', bestAsk: undefined })).toThrow('live Vela quote')
  })

  it('rejects values that cannot be signed as exact browser integers', () => {
    expect(() => prepareOrder({ ...base, size: '0' })).toThrow('valid price')
    expect(() => prepareOrder({ ...base, limitPrice: '100000000000' })).toThrow('valid price')
  })

  it('binds iceberg display size to a limit order', () => {
    const order = prepareOrder({ ...base, displaySize: '1.5' })
    expect(order.display_quantity).toBe(1_500_000)
    expect(orderSigningMessage(order)).toBe(
      '["vela:order:v2","BTC-USDC","Bid","GoodTillCanceled",100000000,5000000,42,null,"None",null,1500000]',
    )
    expect(() => prepareOrder({ ...base, displaySize: '6' })).toThrow('Iceberg display size')
    expect(() => prepareOrder({ ...base, kind: 'market', displaySize: '1' })).toThrow('limit order')
  })

  it('binds the minimum immediate fill and rejects an impossible minimum', () => {
    const order = prepareOrder({ ...base, minSize: '2' })
    expect(order.min_quantity).toBe(2_000_000)
    expect(orderSigningMessage(order)).toBe(
      '["vela:order:v2","BTC-USDC","Bid","GoodTillCanceled",100000000,5000000,42,null,"None",2000000,null]',
    )
    expect(() => prepareOrder({ ...base, minSize: '6' })).toThrow('Minimum immediate fill')
    expect(() => prepareOrder({ ...base, minSize: '0' })).toThrow('Minimum immediate fill')
  })
})
