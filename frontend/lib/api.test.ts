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
})
