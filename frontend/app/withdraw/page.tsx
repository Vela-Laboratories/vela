'use client'

import { useCallback, useEffect, useState } from 'react'
import Link from 'next/link'
import { useAuth } from '@/lib/auth'
import {
  cancelWithdrawal,
  getBalances,
  getWithdrawals,
  initiateWithdrawal,
  type BalanceResponse,
  type WithdrawalStatus,
} from '@/lib/api'
import {
  parseWithdrawalAmount,
  withdrawalCancelMessage,
  withdrawalReservationMessage,
} from '@/lib/withdrawal'
import { Spinner } from '@/components/ui/Spinner'

const ASSETS = ['USDC', 'ETH'] as const
type SupportedAsset = (typeof ASSETS)[number]

function ConnectGate() {
  const { connect, isConnected } = useAuth()
  const [connecting, setConnecting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function handleConnect() {
    setConnecting(true)
    setError(null)
    try {
      await connect()
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : 'Failed to connect wallet')
    } finally {
      setConnecting(false)
    }
  }

  return (
    <div className="min-h-[60vh] flex items-center justify-center px-4">
      <div className="w-full max-w-sm text-center bg-canvas border border-border p-8">
        <h2 className="text-xl font-semibold text-ink mb-2">
          {isConnected ? 'Authenticate your wallet' : 'Connect your wallet'}
        </h2>
        <p className="text-sm text-brown mb-6">
          Sign a one-time challenge to access the exchange balance you want to withdraw.
        </p>
        {error && <p className="text-xs text-red-600 mb-4">{error}</p>}
        <button
          type="button"
          onClick={handleConnect}
          disabled={connecting}
          className="w-full flex items-center justify-center gap-2 h-10 border-[1.5px] border-ink text-ink text-[0.8rem] font-medium uppercase tracking-[0.08em] disabled:opacity-50"
        >
          {connecting && <Spinner size="xs" className="text-ink" />}
          {isConnected ? 'Authenticate Wallet' : 'Connect Wallet'}
        </button>
      </div>
    </div>
  )
}

export default function WithdrawPage() {
  const { address, isConnected, isAuthenticated } = useAuth()
  const [selectedAsset, setSelectedAsset] = useState<SupportedAsset>('USDC')
  const [amount, setAmount] = useState('')
  const [balances, setBalances] = useState<BalanceResponse[]>([])
  const [withdrawals, setWithdrawals] = useState<WithdrawalStatus[]>([])
  const [submitting, setSubmitting] = useState(false)
  const [cancellingId, setCancellingId] = useState<string | null>(null)
  const [loadingMessage, setLoadingMessage] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [success, setSuccess] = useState<WithdrawalStatus | null>(null)

  const refresh = useCallback(async () => {
    if (!address || !isAuthenticated) return
    const [balanceResponse, withdrawalResponse] = await Promise.all([
      getBalances(address),
      getWithdrawals(address),
    ])
    if (balanceResponse.ok && balanceResponse.data) setBalances(balanceResponse.data)
    if (withdrawalResponse.ok && withdrawalResponse.data) {
      setWithdrawals(withdrawalResponse.data)
    }
  }, [address, isAuthenticated])

  useEffect(() => {
    const initial = window.setTimeout(refresh, 0)
    const interval = window.setInterval(refresh, 5_000)
    return () => {
      window.clearTimeout(initial)
      window.clearInterval(interval)
    }
  }, [refresh])

  const currentBalance = balances.find(
    (balance) => balance.asset.toUpperCase() === selectedAsset,
  )
  const availableAmount = Number(currentBalance?.available ?? '0')
  const enteredAmount = Number(amount || '0')
  const amountExceedsBalance = Number.isFinite(enteredAmount) && enteredAmount > availableAmount

  async function walletSignature(message: string): Promise<string> {
    if (!address) throw new Error('Connect a wallet first.')
    const ethereum = (window as Window & {
      ethereum?: { request(input: { method: string; params: string[] }): Promise<unknown> }
    }).ethereum
    if (!ethereum) throw new Error('No browser wallet detected.')
    const signature = await ethereum.request({
      method: 'personal_sign',
      params: [message, address],
    })
    if (typeof signature !== 'string') throw new Error('Wallet returned an invalid signature.')
    return signature
  }

  async function handleSubmit(event: React.FormEvent) {
    event.preventDefault()
    if (!address || !amount || amountExceedsBalance) return
    setSubmitting(true)
    setError(null)
    setSuccess(null)
    try {
      setLoadingMessage('Checking authoritative withdrawal service...')
      const preflight = await getWithdrawals(address)
      if (!preflight.ok || !preflight.data) {
        throw new Error('Authoritative withdrawal service is unavailable. No funds were reserved.')
      }
      const withdrawalId = crypto.randomUUID()
      const nonce = Date.now()
      const rawAmount = parseWithdrawalAmount(amount, selectedAsset)
      const message = withdrawalReservationMessage({
        withdrawalId,
        customer: address,
        destination: address,
        asset: selectedAsset,
        amount: rawAmount,
        nonce,
      })
      setLoadingMessage('Authorize the exact reservation in your wallet...')
      const signature = await walletSignature(message)
      setLoadingMessage('Reserving funds in the authoritative ledger...')
      const response = await initiateWithdrawal({
        withdrawal_id: withdrawalId,
        asset: selectedAsset,
        amount: rawAmount,
        nonce,
        address,
        destination_address: address,
        signature,
      })
      if (!response.ok || !response.data) {
        throw new Error(response.error ?? 'Withdrawal reservation failed.')
      }
      setSuccess(response.data)
      setWithdrawals((current) => [
        response.data!,
        ...current.filter((item) => item.id !== response.data!.id),
      ])
      setAmount('')
      await refresh()
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : 'Withdrawal reservation failed.')
    } finally {
      setSubmitting(false)
      setLoadingMessage('')
    }
  }

  async function handleCancel(withdrawal: WithdrawalStatus) {
    if (!address || withdrawal.state !== 'reserved') return
    setCancellingId(withdrawal.id)
    setError(null)
    try {
      const nonce = Number(withdrawal.nonce)
      if (!Number.isSafeInteger(nonce)) throw new Error('Withdrawal nonce is invalid.')
      const signature = await walletSignature(withdrawalCancelMessage(withdrawal.id, nonce))
      const response = await cancelWithdrawal(withdrawal.id, { nonce, address, signature })
      if (!response.ok || !response.data) {
        throw new Error(response.error ?? 'Withdrawal cancellation failed.')
      }
      setWithdrawals((current) => current.map((item) => (
        item.id === response.data!.id ? response.data! : item
      )))
      await refresh()
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : 'Withdrawal cancellation failed.')
    } finally {
      setCancellingId(null)
    }
  }

  if (!isConnected || !isAuthenticated) return <ConnectGate />

  return (
    <div className="min-h-[calc(100vh-60px)] bg-parchment flex items-start justify-center px-4 py-12">
      <div className="w-full max-w-[520px]">
        <p className="text-[0.7rem] font-medium uppercase tracking-[0.15em] text-brown mb-6">
          WITHDRAW FUNDS
        </p>
        <div className="mb-6 border-l-[3px] border-[#9488ee] bg-[rgba(0,210,210,0.04)] px-4 py-3">
          <p className="text-[0.8rem] text-ink leading-relaxed">
            A wallet signature authorizes one exact request. Vela reserves the balance atomically
            before review, signing, or broadcast. This build does not issue custody signatures yet.
          </p>
        </div>

        {success && (
          <div className="bg-canvas border border-border p-6 mb-6">
            <p className="text-lg font-semibold text-ink mb-2">Funds reserved</p>
            <p className="text-sm text-brown mb-3">
              {success.amount} {success.asset} is unavailable for trading or another withdrawal.
              No custody transaction has been signed or broadcast.
            </p>
            <p className="text-xs text-brown font-mono break-all">Request {success.id}</p>
          </div>
        )}

        <form onSubmit={handleSubmit} className="bg-canvas border border-border p-8">
          <label className="block text-[0.7rem] font-medium uppercase tracking-[0.12em] text-brown mb-2">
            ASSET
          </label>
          <select
            value={selectedAsset}
            onChange={(event) => {
              setSelectedAsset(event.target.value as SupportedAsset)
              setAmount('')
            }}
            className="w-full h-10 px-3 mb-5 bg-vellum border border-border text-sm text-ink"
          >
            {ASSETS.map((asset) => <option key={asset} value={asset}>{asset}</option>)}
          </select>

          <label className="block text-[0.7rem] font-medium uppercase tracking-[0.12em] text-brown mb-2">
            AMOUNT
          </label>
          <div className="relative mb-2">
            <input
              value={amount}
              onChange={(event) => setAmount(event.target.value)}
              inputMode="decimal"
              placeholder="0.00"
              className="w-full h-10 px-3 pr-20 bg-vellum border border-border text-sm text-ink font-mono"
            />
            <button
              type="button"
              onClick={() => setAmount(currentBalance?.available ?? '')}
              className="absolute right-3 top-1/2 -translate-y-1/2 text-[0.65rem] uppercase text-brown"
            >
              MAX
            </button>
          </div>
          <p className="text-xs text-brown mb-5">
            Available: {currentBalance?.available ?? '0'} {selectedAsset}
          </p>

          <p className="text-[0.7rem] font-medium uppercase tracking-[0.12em] text-brown mb-1">
            DESTINATION
          </p>
          <p className="text-xs text-ink font-mono break-all mb-6">{address}</p>

          {amountExceedsBalance && (
            <p className="text-xs text-red-600 mb-4">Amount exceeds available balance.</p>
          )}
          {error && <p className="text-xs text-red-600 mb-4">{error}</p>}
          {loadingMessage && <p className="text-xs text-brown mb-3">{loadingMessage}</p>}
          <button
            type="submit"
            disabled={submitting || !amount || amountExceedsBalance}
            className="w-full flex items-center justify-center gap-2 h-10 bg-[#9488ee] text-[0.8rem] font-medium uppercase tracking-[0.08em] text-ink disabled:opacity-50"
          >
            {submitting && <Spinner size="xs" className="text-ink" />}
            RESERVE WITHDRAWAL
          </button>
        </form>

        <section className="mt-8" aria-label="Withdrawal activity">
          <p className="text-[0.7rem] font-medium uppercase tracking-[0.15em] text-brown mb-4">
            WITHDRAWAL ACTIVITY
          </p>
          <div className="bg-canvas border border-border">
            {withdrawals.length === 0 ? (
              <p className="text-sm text-brown text-center py-8">No authoritative withdrawals</p>
            ) : withdrawals.map((withdrawal) => (
              <article key={withdrawal.id} className="p-4 border-b border-border last:border-b-0">
                <div className="flex justify-between gap-4 mb-2">
                  <span className="text-sm text-ink font-mono">
                    {withdrawal.amount} {withdrawal.asset}
                  </span>
                  <span className="text-[0.65rem] uppercase tracking-[0.1em] text-[#00A090]">
                    {withdrawal.state}
                  </span>
                </div>
                <p className="text-xs text-brown mb-1">{withdrawal.status_message}</p>
                <p className="text-[0.65rem] uppercase tracking-[0.08em] text-brown mb-2">
                  Funds {withdrawal.funds_status.replace('_', ' ')}
                  {withdrawal.action_required === 'operator_review' ? ' · Operator review required' : ''}
                </p>
                <p className="text-[0.65rem] text-brown font-mono break-all mb-3">
                  {withdrawal.id}
                </p>
                {withdrawal.transaction_hash && (
                  <p className="text-[0.65rem] text-brown font-mono break-all mb-3">
                    Transaction {withdrawal.transaction_hash}
                  </p>
                )}
                {withdrawal.can_cancel && (
                  <button
                    type="button"
                    onClick={() => void handleCancel(withdrawal)}
                    disabled={cancellingId === withdrawal.id}
                    className="text-[0.65rem] uppercase tracking-[0.1em] text-brown underline disabled:opacity-50"
                  >
                    {cancellingId === withdrawal.id ? 'Releasing...' : 'Cancel and release funds'}
                  </button>
                )}
              </article>
            ))}
          </div>
        </section>

        <Link href="/dashboard" className="block text-center text-xs text-brown mt-6 underline">
          Return to dashboard
        </Link>
      </div>
    </div>
  )
}
