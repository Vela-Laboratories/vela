import { keccak256, stringToHex } from 'viem'
import type { AgentCapabilityScope, PostOrderBody, RegisterAgentBody } from './api'

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

export function agentRevocationMessage(agent: string, nonce: number): string {
  if (!/^0x[0-9a-fA-F]{40}$/.test(agent)) throw new Error('Invalid agent address')
  if (!Number.isSafeInteger(nonce) || nonce < 1) throw new Error('Invalid revocation nonce')
  return `vela:agent:revoke:${agent.toLowerCase()}:${nonce}`
}

export function agentRegistrationMessage(body: Omit<RegisterAgentBody, 'master' | 'signature'>): string {
  if (!/^0x[0-9a-fA-F]{40}$/.test(body.agent)) throw new Error('Invalid agent address')
  for (const value of [body.expires_at_ms, body.max_notional_per_order, body.nonce]) {
    if (!Number.isSafeInteger(value) || value < 1) throw new Error('Invalid delegation integer')
  }
  // Match the Rust CapabilityScope field order and explicit nulls in serde_json.
  const scope: Required<AgentCapabilityScope> = {
    allowed_markets: body.scope.allowed_markets ?? null,
    allowed_order_types: body.scope.allowed_order_types ?? null,
    allowed_sides: body.scope.allowed_sides ?? null,
    max_notional_per_hour: body.scope.max_notional_per_hour ?? null,
    max_notional_per_day: body.scope.max_notional_per_day ?? null,
  }
  for (const cap of [scope.max_notional_per_hour, scope.max_notional_per_day]) {
    if (cap !== null && (!Number.isSafeInteger(cap) || cap < 1)) throw new Error('Invalid rolling cap')
  }
  const scopeHash = keccak256(stringToHex(JSON.stringify(scope))).slice(2)
  return `vela:agent:register:${body.agent.toLowerCase()}:${body.expires_at_ms}:${body.max_notional_per_order}:${body.nonce}:${scopeHash}`
}
