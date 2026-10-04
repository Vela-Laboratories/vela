'use client'

import { useCallback, useEffect, useState } from 'react'
import Link from 'next/link'
import { useAuth } from '@/lib/auth'
import { getAgents, registerAgent, revokeAgent, type AgentListResponse } from '@/lib/api'
import { agentRegistrationMessage, agentRevocationMessage } from '@/lib/signing'
import { Button } from '@/components/ui/Button'
import { Card } from '@/components/ui/Card'
import { Input } from '@/components/ui/Input'

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
  const [registering, setRegistering] = useState(false)
  const [agentAddress, setAgentAddress] = useState('')
  const [marketLimit, setMarketLimit] = useState('')
  const [sideLimit, setSideLimit] = useState<'Both' | 'Bid' | 'Ask'>('Both')
  const [expiryHours, setExpiryHours] = useState('24')
  const [orderCap, setOrderCap] = useState('100')
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

  async function register() {
    if (!address || !isAuthenticated) return
    setError(null)
    setRegistering(true)
    try {
      if (!window.ethereum) throw new Error('No browser wallet detected')
      if (!/^0x[0-9a-fA-F]{40}$/.test(agentAddress)) throw new Error('Enter an existing agent wallet address')
      if (agentAddress.toLowerCase() === address.toLowerCase()) throw new Error('Use a separate agent address')
      const prior = agents.find((agent) => agent.agent.toLowerCase() === agentAddress.toLowerCase())
      if (prior && !Number.isSafeInteger(prior.nonce)) throw new Error('Agent nonce cannot be signed safely in this browser')
      const hours = Number(expiryHours)
      if (!Number.isSafeInteger(hours) || hours < 1 || hours > 168) throw new Error('Expiry must be 1 to 168 hours')
      if (!/^\d{1,9}(?:\.\d{1,6})?$/.test(orderCap)) throw new Error('Enter a valid USDC cap with up to 6 decimals')
      const [whole, fraction = ''] = orderCap.split('.')
      const cap = Number(whole) * 1_000_000 + Number(fraction.padEnd(6, '0'))
      if (!Number.isSafeInteger(cap) || cap < 1) throw new Error('Per-order cap must be positive')
      const markets = marketLimit.split(',').map((market) => market.trim().toUpperCase()).filter(Boolean)
      if (markets.some((market) => !/^[A-Z0-9]+-[A-Z0-9]+$/.test(market))) throw new Error('Markets must look like ETH-USDC')
      const body = {
        agent: agentAddress,
        expires_at_ms: Date.now() + hours * 60 * 60 * 1000,
        max_notional_per_order: cap,
        nonce: Math.max(Date.now(), (prior?.nonce ?? 0) + 1),
        scope: {
          allowed_markets: markets.length ? markets : null,
          allowed_sides: sideLimit === 'Both' ? null : [sideLimit],
        },
      }
      const signature = await window.ethereum.request({
        method: 'personal_sign',
        params: [agentRegistrationMessage(body), address],
      })
      if (typeof signature !== 'string') throw new Error('Wallet did not return a signature')
      const response = await registerAgent({ ...body, master: address, signature })
      if (!response.ok) throw new Error(response.error ?? 'Registration failed')
      setAgentAddress('')
      await refresh()
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : 'Registration failed')
    } finally {
      setRegistering(false)
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
      {snapshot?.storage === 'postgres'
        ? 'Delegations are stored in the testnet database and checked on every delegated request. Revocations apply to all instances using this runtime and chain.'
        : 'Delegations are held in this testnet server’s memory and disappear after a restart. Revocation applies to this server.'}
      {' '}Revocation does not cancel a request that was already authorized.
    </Card>
    <Card className="p-5 space-y-4">
      <div><h2 className="text-ink font-serif text-xl">Register an existing agent</h2>
        <p className="text-brown text-sm mt-1">Enter the public address of a separate wallet you control. Never enter its private key here.</p></div>
      <Input label="Agent public address" value={agentAddress} onChange={(event) => setAgentAddress(event.target.value.trim())} placeholder="0x…" autoComplete="off" />
      <div className="grid sm:grid-cols-2 gap-4">
        <Input label="Expires after (hours)" type="number" min="1" max="168" value={expiryHours} onChange={(event) => setExpiryHours(event.target.value)} />
        <Input label="Per-order cap (USDC)" inputMode="decimal" value={orderCap} onChange={(event) => setOrderCap(event.target.value)} />
        <Input label="Allowed markets (optional)" value={marketLimit} onChange={(event) => setMarketLimit(event.target.value)} placeholder="ETH-USDC, BTC-USDC" hint="Leave blank for any market; separate multiple markets with commas." />
        <div className="flex flex-col gap-1.5"><label htmlFor="agent-side" className="text-[0.7rem] font-medium text-brown uppercase tracking-[0.12em]">Allowed side</label>
          <select id="agent-side" className="h-10 px-3 bg-parchment border border-border text-ink text-sm" value={sideLimit} onChange={(event) => setSideLimit(event.target.value as typeof sideLimit)}>
            <option value="Both">Buy and sell</option><option value="Bid">Buy only</option><option value="Ask">Sell only</option>
          </select></div>
      </div>
      <p className="text-brown text-xs">These limits are included in the message signed by your wallet. Revocation remains available at any time.</p>
      <Button onClick={() => void register()} disabled={registering || revokingAgent !== null}>{registering ? 'Signing…' : 'Sign and register'}</Button>
    </Card>
    {error && <Card className="p-4 text-terra text-sm" role="alert">{error}</Card>}
    {!loading && !initialLoading && agents.length === 0 && <Card className="p-8 text-brown text-sm">No agent wallets are registered for this account.</Card>}
    <div className="space-y-3">
      {agents.map((agent) => <Card key={agent.agent} className="p-5 flex flex-wrap items-center justify-between gap-4">
        <div className="space-y-2">
          <p className="font-mono text-ink text-sm break-all">{agent.agent}</p>
          <p className="text-brown text-xs">{statusFor(agent)} · Expires {new Date(agent.expires_at_ms).toLocaleString()}</p>
          <p className="text-brown text-xs">Per-order cap: ${(agent.max_notional_per_order / 1_000_000).toLocaleString(undefined, { maximumFractionDigits: 2 })} USDC</p>
          <p className="text-brown text-xs">Markets: {agent.scope?.allowed_markets?.length ? agent.scope.allowed_markets.join(', ') : 'Any'} · Side: {agent.scope?.allowed_sides?.length ? agent.scope.allowed_sides.join(', ') : 'Buy and sell'}</p>
        </div>
        {statusFor(agent) === 'Active' && <Button variant="danger" disabled={revokingAgent !== null} onClick={() => void revoke(agent)}>
          {revokingAgent === agent.agent ? 'Revoking…' : 'Revoke'}
        </Button>}
      </Card>)}
    </div>
    <Link href="/profile" className="inline-block text-brown text-sm underline">Back to profile</Link>
  </main>
}
