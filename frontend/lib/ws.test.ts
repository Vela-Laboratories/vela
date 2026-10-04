import { afterEach, describe, expect, it, vi } from 'vitest'
import { normalizeWsEnvelope, VelaWsClient } from './ws'

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

class FakeSocket {
  static CONNECTING = 0
  static OPEN = 1
  static instances: FakeSocket[] = []
  readyState = FakeSocket.CONNECTING
  sent: string[] = []
  onopen?: () => void
  onclose?: () => void
  onerror?: () => void
  onmessage?: (event: MessageEvent) => void
  constructor() { FakeSocket.instances.push(this) }
  send(message: string) { this.sent.push(message) }
  close() { this.readyState = 3; this.onclose?.() }
  open() { this.readyState = FakeSocket.OPEN; this.onopen?.() }
  emit(seq: number, type = 'snapshot') {
    this.onmessage?.({ data: JSON.stringify({ type, channel: 'orderbook:ETH-USDC', seq, timestamp: 1, data: { bids: [], asks: [] } }) } as MessageEvent)
  }
}

describe('WebSocket sequence recovery', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    vi.useRealTimers()
    vi.restoreAllMocks()
    FakeSocket.instances = []
  })

  it('requests a fresh subscription on a gap and accepts a full snapshot', () => {
    vi.stubGlobal('WebSocket', FakeSocket)
    vi.spyOn(console, 'warn').mockImplementation(() => {})
    const client = new VelaWsClient('ws://example.test/ws')
    const consume = vi.fn()
    client.subscribe('orderbook:ETH-USDC', consume)
    client.connect()
    const socket = FakeSocket.instances[0]
    socket.open()
    socket.emit(1)
    socket.emit(3, 'delta')
    expect(consume).toHaveBeenCalledTimes(1)
    expect(socket.sent.at(-1)).toBe(JSON.stringify({ type: 'subscribe', channels: ['orderbook:ETH-USDC'] }))
    socket.emit(4, 'delta')
    expect(consume).toHaveBeenCalledTimes(1)
    socket.emit(5)
    expect(consume).toHaveBeenCalledTimes(2)
    client.disconnect()
  })

  it('resubscribes after reconnect and accepts a restarted server sequence', () => {
    vi.useFakeTimers()
    vi.stubGlobal('WebSocket', FakeSocket)
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const client = new VelaWsClient('ws://example.test/ws')
    const consume = vi.fn()
    client.subscribe('orderbook:ETH-USDC', consume)
    client.connect()
    const first = FakeSocket.instances[0]
    first.open()
    first.emit(42)
    first.close()
    vi.advanceTimersByTime(1000)
    const second = FakeSocket.instances[1]
    second.open()
    expect(second.sent[0]).toBe(JSON.stringify({ type: 'subscribe', channels: ['orderbook:ETH-USDC'] }))
    second.emit(7)
    expect(consume).toHaveBeenCalledTimes(2)
    expect(warn).not.toHaveBeenCalled()
    client.disconnect()
  })
})
