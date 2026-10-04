import { afterEach, describe, expect, it, vi } from 'vitest'

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('lib/api', () => {
  it('exports the wrappers the pages import', async () => {
    // Smoke test — verifies the module loads without throwing and the
    // public exports the frontend depends on are actually present. This
    // catches accidental deletions in refactors before they crash a page.
    const mod = await import('./api')
    expect(typeof mod.listMarkets).toBe('function')
    expect(typeof mod.getBook).toBe('function')
    expect(typeof mod.getDeposits).toBe('function')
    expect(typeof mod.getWithdrawals).toBe('function')
    expect(typeof mod.getAccountFills).toBe('function')
    expect(typeof mod.requestAuthChallenge).toBe('function')
    expect(typeof mod.createAuthSession).toBe('function')
    expect(typeof mod.revokeAuthSession).toBe('function')
    expect(typeof mod.initiateWithdrawal).toBe('function')
    expect(typeof mod.cancelWithdrawal).toBe('function')
    expect(typeof mod.postOrder).toBe('function')
    expect(typeof mod.cancelOrder).toBe('function')
    expect(typeof mod.getPortfolio).toBe('function')
    expect(typeof mod.getPortfolioCsv).toBe('function')
    expect(typeof mod.getPerpMarkets).toBe('function')
    expect(typeof mod.getLiquidatablePositions).toBe('function')
    expect(typeof mod.getBorrowLendMarkets).toBe('function')
    expect(typeof mod.getAgentTier).toBe('function')
  })

  it('filters authoritative deposits by the real transaction hash', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ ok: true, data: [] }),
    })
    vi.stubGlobal('fetch', fetchMock)
    const { getDeposits } = await import('./api')
    const hash = `0x${'ab'.repeat(32)}`

    await getDeposits('0x0000000000000000000000000000000000000001', hash)

    expect(fetchMock).toHaveBeenCalledWith(
      `https://vela-engine.fly.dev/account/0x0000000000000000000000000000000000000001/deposits?transaction_hash=${hash}`,
      expect.objectContaining({ cache: 'no-store' }),
    )
  })

  it('attaches the in-memory bearer to JSON and CSV private reads', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ ok: true, data: [] }),
      text: async () => 'timestamp,market\n',
    })
    vi.stubGlobal('fetch', fetchMock)
    const { getBalances, getPortfolioCsv, setAuthSessionToken } = await import('./api')
    setAuthSessionToken('session-token')

    await getBalances('0x0000000000000000000000000000000000000001')
    await getPortfolioCsv('0x0000000000000000000000000000000000000001')

    for (const [, init] of fetchMock.mock.calls) {
      expect((init.headers as Headers).get('Authorization')).toBe('Bearer session-token')
    }
    setAuthSessionToken(null)
  })

  it('clears an expired session and signals the auth provider on 401', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: false,
      status: 401,
      statusText: 'Unauthorized',
      text: async () => 'authentication required',
    })
    vi.stubGlobal('fetch', fetchMock)
    const expired = vi.fn()
    window.addEventListener('vela:auth-expired', expired)
    const { getBalances, setAuthSessionToken } = await import('./api')
    setAuthSessionToken('expired-token')

    const response = await getBalances(
      '0x0000000000000000000000000000000000000001',
    )
    expect(response).toEqual({ ok: false, error: 'authentication required' })
    expect(expired).toHaveBeenCalledOnce()
    window.removeEventListener('vela:auth-expired', expired)
  })

  it('shows the API reason when a fill CSV export fails', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({
      ok: false,
      status: 403,
      statusText: 'Forbidden',
      text: async () => JSON.stringify({ ok: false, error: 'Session belongs to another account' }),
    }))
    const { getPortfolioCsv } = await import('./api')
    const response = await getPortfolioCsv('0x0000000000000000000000000000000000000001')
    expect(response).toEqual({ ok: false, error: 'Session belongs to another account' })
  })

  it('reads the current delegations for the requested master wallet', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ ok: true, data: { master: '0xabc', agents: [] } }),
    })
    vi.stubGlobal('fetch', fetchMock)
    const { getAgents } = await import('./api')
    await getAgents('0xabc')
    expect(fetchMock).toHaveBeenCalledWith(
      'https://vela-engine.fly.dev/agents/0xabc',
      expect.objectContaining({ cache: 'no-store' }),
    )
  })

  it('sends a signed revocation without treating revoked=false as success', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ ok: true, data: { agent: '0xagent', revoked: false } }),
    })
    vi.stubGlobal('fetch', fetchMock)
    const { revokeAgent } = await import('./api')
    const body = { master: '0xmaster', agent: '0xagent', nonce: 42, signature: '0xsig' }
    const response = await revokeAgent(body)
    expect(response.data?.revoked).toBe(false)
    expect(fetchMock).toHaveBeenCalledWith(
      'https://vela-engine.fly.dev/agents/revoke',
      expect.objectContaining({ method: 'POST', body: JSON.stringify(body) }),
    )
  })
})
