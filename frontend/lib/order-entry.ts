import type { PostOrderBody } from './api'

export interface OrderEntry {
  market: string
  side: 'buy' | 'sell'
  kind: 'limit' | 'market'
  limitPrice: string
  bestBid?: string
  bestAsk?: string
  size: string
  nonce: number
  address: string
}

/** Convert ticket inputs to the Rust API's signed fixed-point order. */
export function prepareOrder(entry: OrderEntry): Omit<PostOrderBody, 'signature'> {
  const displayedPrice = entry.kind === 'limit'
    ? entry.limitPrice
    : entry.side === 'buy' ? entry.bestAsk : entry.bestBid
  const price = Math.round(Number(displayedPrice) * 1_000_000)
  const quantity = Math.round(Number(entry.size) * 1_000_000)
  if (!displayedPrice || !Number.isSafeInteger(price) || price <= 0 ||
      !entry.size || !Number.isSafeInteger(quantity) || quantity <= 0) {
    throw new Error('Enter a valid price and amount. Immediate orders need a live Vela quote.')
  }
  return {
    market: entry.market,
    side: entry.side === 'buy' ? 'Bid' : 'Ask',
    order_type: entry.kind === 'limit' ? 'GoodTillCanceled' : 'ImmediateOrCancel',
    price,
    quantity,
    nonce: entry.nonce,
    address: entry.address,
    signature_version: 2,
  }
}
