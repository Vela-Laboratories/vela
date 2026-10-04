import { describe, expect, it } from 'vitest'
import { agentRevocationMessage } from './signing'

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
