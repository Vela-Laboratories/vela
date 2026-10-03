import type { PostOrderBody } from './api'

export function orderSigningMessage(order: Omit<PostOrderBody, 'signature'>): string {
  return JSON.stringify([
    'vela:order:v2',
    order.market,
    order.side,
    order.order_type,
    order.price,
    order.quantity,
    order.nonce,
    order.client_order_id ?? null,
    order.stp ?? 'None',
    order.min_quantity ?? null,
    order.display_quantity ?? null,
  ])
}

export async function signOrder(order: Omit<PostOrderBody, 'signature'>): Promise<string> {
  if (!window.ethereum) throw new Error('No wallet detected. Please install MetaMask.')
  const message = orderSigningMessage(order)
  return window.ethereum.request({
    method: 'personal_sign',
    params: [message, order.address],
  }) as Promise<string>
}

export async function signCancel(cancel: {
  order_id?: number
  client_order_id?: string
  nonce: number
  address: string
}): Promise<string> {
  if (!window.ethereum) throw new Error('No wallet detected. Please install MetaMask.')
  const message = `vela:cancel:${cancel.order_id ?? ''}:${cancel.client_order_id ?? ''}:${cancel.nonce}`
  return window.ethereum.request({
    method: 'personal_sign',
    params: [message, cancel.address],
  }) as Promise<string>
}
