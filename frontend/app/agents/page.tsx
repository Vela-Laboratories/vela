'use client'

import { useCallback, useEffect, useState } from 'react'
import Link from 'next/link'
import { useAuth } from '@/lib/auth'
import { getAgents, revokeAgent, type AgentListResponse } from '@/lib/api'
import { agentRevocationMessage } from '@/lib/signing'
import { Button } from '@/components/ui/Button'
import { Card } from '@/components/ui/Card'

function statusFor(agent: AgentListResponse['agents'][number]): string {
  if (agent.revoked) return 'Revoked'
  if (!agent.active || agent.expires_at_ms <= Date.now()) return 'Expired'
  return 'Active'
}

function nextRevocationNonce(previous: number): number {
  return Math.max(Date.now(), previous + 1)
}

export default function AgentsPage() {
  const { address, isAuthenticated, connect } = useAuth()
  const [snapshot, setSnapshot] = useState<AgentListResponse | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [revokingAgent, setRevokingAgent] = useState<string | null>(null)
  const agents = snapshot && snapshot.master.toLowerCase() === address?.toLowerCase() ? snapshot.agents : []
  const initialLoading = isAuthenticated && snapshot?.master.toLowerCase() !== address?.toLowerCase()

  const refresh = useCallback(async () => {
    if (!address || !isAuthenticated) return
    setLoading(true)
    setError(null)
    try {
      const response = await getAgents(address)
      if (!response.ok || !response.data) throw new Error(response.error ?? 'Unable to load agents')
      setSnapshot(response.data)
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : 'Unable to load agents')
    } finally {
      setLoading(false)
    }
  }, [address, isAuthenticated])

  useEffect(() => {
    if (!address || !isAuthenticated) return
    let cancelled = false
    void getAgents(address).then((response) => {
      if (cancelled) return
      if (!response.ok || !response.data) {
        setError(response.error ?? 'Unable to load agents')
      } else {
        setSnapshot(response.data)
      }
    }).catch((caught: unknown) => {
      if (!cancelled) setError(caught instanceof Error ? caught.message : 'Unable to load agents')
    })
    return () => { cancelled = true }
  }, [address, isAuthenticated])

  async function revoke(agent: AgentListResponse['agents'][number]) {
    if (!address || !isAuthenticated) return
    setError(null)
    setRevokingAgent(agent.agent)
    try {
      if (!Number.isSafeInteger(agent.nonce)) throw new Error('Agent nonce cannot be signed safely in this browser')
      if (!window.ethereum) throw new Error('No browser wallet detected')
      const nonce = nextRevocationNonce(agent.nonce)
      const message = agentRevocationMessage(agent.agent, nonce)
      const signature = await window.ethereum.request({ method: 'personal_sign', params: [message, address] })
      if (typeof signature !== 'string') throw new Error('Wallet did not return a signature')
      const response = await revokeAgent({ master: address, agent: agent.agent, nonce, signature })
      if (!response.ok || !response.data?.revoked) throw new Error(response.error ?? 'Agent was not revoked')
      await refresh()
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : 'Revocation failed')
    } finally {
      setRevokingAgent(null)
    }
  }

  if (!address || !isAuthenticated) {
    return <main className="max-w-3xl mx-auto px-6 py-16"><Card className="p-8 space-y-5">
      <p className="text-brown text-xs uppercase tracking-widest">Agent wallets</p>
      <h1 className="text-ink text-3xl font-serif">Authenticate to manage agents</h1>
      <p className="text-brown text-sm">Review the trading keys delegated by your wallet.</p>
      <Button onClick={() => void connect().catch((caught: unknown) => setError(caught instanceof Error ? caught.message : 'Wallet connection failed'))}>Connect wallet</Button>
      {error && <p role="alert" className="text-terra text-sm">{error}</p>}
    </Card></main>
  }

  return <main className="max-w-4xl mx-auto px-6 py-10 space-y-6">
    <header className="flex flex-wrap items-end justify-between gap-4">
      <div><p className="text-brown text-xs uppercase tracking-widest">Account security</p>
        <h1 className="text-ink text-3xl font-serif mt-2">Agent wallets</h1>
        <p className="text-brown text-sm mt-2">Delegated trading keys for {address.slice(0, 6)}…{address.slice(-4)}</p>
      </div>
      <Button variant="ghost" onClick={() => void refresh()} disabled={loading || initialLoading}>{loading || initialLoading ? 'Loading…' : 'Refresh'}</Button>
    </header>

    <Card className="p-5 text-brown text-sm">
      Delegations are held in this testnet server’s memory. They disappear after a server restart.
      A revoked key stops working on this server immediately; this is not an on-chain revocation.
    </Card>
    {error && <Card className="p-4 text-terra text-sm" role="alert">{error}</Card>}
    {!loading && !initialLoading && agents.length === 0 && <Card className="p-8 text-brown text-sm">No agent wallets are registered for this account.</Card>}
    <div className="space-y-3">
      {agents.map((agent) => <Card key={agent.agent} className="p-5 flex flex-wrap items-center justify-between gap-4">
        <div className="space-y-2">
          <p className="font-mono text-ink text-sm break-all">{agent.agent}</p>
          <p className="text-brown text-xs">{statusFor(agent)} · Expires {new Date(agent.expires_at_ms).toLocaleString()}</p>
          <p className="text-brown text-xs">Per-order cap: ${(agent.max_notional_per_order / 1_000_000).toLocaleString(undefined, { maximumFractionDigits: 2 })} USDC</p>
        </div>
        {statusFor(agent) === 'Active' && <Button variant="danger" disabled={revokingAgent !== null} onClick={() => void revoke(agent)}>
          {revokingAgent === agent.agent ? 'Revoking…' : 'Revoke'}
        </Button>}
      </Card>)}
    </div>
    <Link href="/profile" className="inline-block text-brown text-sm underline">Back to profile</Link>
  </main>
}
