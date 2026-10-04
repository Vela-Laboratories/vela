import { describe, expect, it } from 'vitest'
import { agentRegistrationMessage, agentRevocationMessage } from './signing'

describe('agent revocation signing', () => {
  const agent = `0x${'Ab'.repeat(20)}`

  it('uses the canonical lowercase address and nonce', () => {
    expect(agentRevocationMessage(agent, 42)).toBe(
      `vela:agent:revoke:0x${'ab'.repeat(20)}:42`,
    )
  })

  it('rejects malformed addresses and unsafe nonces', () => {
    expect(() => agentRevocationMessage('0x123', 1)).toThrow('Invalid agent address')
    expect(() => agentRevocationMessage(agent, Number.MAX_SAFE_INTEGER + 1)).toThrow('Invalid revocation nonce')
  })
})

describe('agent registration signing', () => {
  const body = {
    agent: `0x${'Ab'.repeat(20)}`,
    expires_at_ms: 2_000_000_000_000,
    max_notional_per_order: 1_000_000,
    nonce: 42,
    scope: {},
  }

  it('binds the canonical scope, address, and limits', () => {
    const message = agentRegistrationMessage(body)
    expect(message).toMatch(/^vela:agent:register:0x(?:ab){20}:2000000000000:1000000:42:[0-9a-f]{64}$/)
    expect(agentRegistrationMessage({ ...body, scope: { allowed_sides: ['Bid'] } })).not.toBe(message)
  })

  it('rejects unsafe values', () => {
    expect(() => agentRegistrationMessage({ ...body, nonce: Number.MAX_SAFE_INTEGER + 1 })).toThrow()
    expect(() => agentRegistrationMessage({ ...body, scope: { max_notional_per_hour: -1 } })).toThrow()
  })
})
