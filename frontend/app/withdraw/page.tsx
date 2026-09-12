'use client'

import { useEffect, useState } from 'react'
import Link from 'next/link'
import { useAuth } from '@/lib/auth'
import { withdraw, getBalances, type BalanceResponse } from '@/lib/api'
import { requestWithdrawalSignature, withdrawETH } from '@/lib/contract'
import { Spinner } from '@/components/ui/Spinner'

const ASSETS = [
  'USDC', 'ETH', 'BTC', 'SOL', 'AVAX', 'MATIC',
  'LINK', 'UNI', 'ARB', 'OP', 'AAVE', 'DOGE',
]

function ConnectGate() {
  const { connect } = useAuth()
  const [connecting, setConnecting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function handleConnect() {
    setConnecting(true)
    setError(null)
    try {
      await connect()
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to connect wallet')
    } finally {
      setConnecting(false)
    }
  }

  return (
    <div className="min-h-[60vh] flex items-center justify-center px-4">
      <div className="w-full max-w-sm text-center bg-canvas border border-border shadow-[0_4px_24px_rgba(14,26,32,0.08)] p-8" style={{ borderRadius: 0 }}>
        <div className="w-16 h-16 bg-ochre/10 flex items-center justify-center mx-auto mb-5">
          <svg width="28" height="28" viewBox="0 0 24 24" fill="none">
            <path
              d="M21 7H3C2.44772 7 2 7.44772 2 8V20C2 20.5523 2.44772 21 3 21H21C21.5523 21 22 20.5523 22 20V8C22 7.44772 21.5523 7 21 7Z"
              stroke="#00D2D2"
              strokeWidth="1.5"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
            <circle cx="14" cy="14" r="2" fill="#00D2D2" />
            <path
              d="M17 7V5C17 3.89543 16.1046 3 15 3H9C7.89543 3 7 3.89543 7 5V7"
              stroke="#00D2D2"
              strokeWidth="1.5"
              strokeLinecap="round"
            />
          </svg>
        </div>
        <h2 className="text-xl font-semibold text-ink mb-2">Connect your wallet</h2>
        <p className="text-sm text-brown mb-6">Connect your wallet to withdraw funds.</p>
        {error && (
          <p className="text-xs text-red-600 mb-4">{error}</p>
        )}
        <button
          type="button"
          onClick={handleConnect}
          disabled={connecting}
          className="w-full flex items-center justify-center gap-2 px-4 h-10 border-[1.5px] border-ink text-ink text-[0.8rem] font-medium uppercase tracking-[0.08em] hover:bg-ink hover:text-parchment transition-colors duration-150 disabled:opacity-50"
          style={{ borderRadius: 0 }}
        >
          {connecting && <Spinner size="xs" className="text-ink" />}
          Connect Wallet
        </button>
      </div>
    </div>
  )
}

interface SuccessState {
  amount: string
  asset: string
  nonce: number
  txHash: string | null
}

export default function WithdrawPage() {
  const { address, isConnected } = useAuth()

  const [selectedAsset, setSelectedAsset] = useState('USDC')
  const [amount, setAmount] = useState('')
  const [balances, setBalances] = useState<BalanceResponse[]>([])
  const [submitting, setSubmitting] = useState(false)
  const [loadingMsg, setLoadingMsg] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [success, setSuccess] = useState<SuccessState | null>(null)

  useEffect(() => {
    if (!address) return
    getBalances(address).then((res) => {
      if (res.ok && res.data) setBalances(res.data)
    })
  }, [address])

  const currentBalance = balances.find(
    (b) => b.asset.toUpperCase() === selectedAsset.toUpperCase(),
  )

  const availableAmount = currentBalance ? parseFloat(currentBalance.available) : 0
  const amountNum = parseFloat(amount) || 0
  const amountExceedsBalance = amount !== '' && amountNum > availableAmount

  function handleMax() {
    if (currentBalance) {
      setAmount(currentBalance.available)
    }
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    if (!address || !amount || amountExceedsBalance) return
    setSubmitting(true)
    setError(null)

    try {
      if (selectedAsset === 'ETH') {
        const nonce = Date.now()
        setLoadingMsg('Requesting operator signature...')
        const { signature, amountWei } = await requestWithdrawalSignature(address, 'ETH', amount, nonce)
        setLoadingMsg('Confirm transaction in MetaMask...')
        const txHash = await withdrawETH(amountWei, nonce, signature)
        setSuccess({ amount, asset: selectedAsset, nonce, txHash })
      } else {
        const rawAmount = Math.round(parseFloat(amount) * 1_000_000)
        const nonce = Date.now()
        const message = `vela:withdraw:${selectedAsset}:${rawAmount}:${nonce}`
        const signature = (await (window as any).ethereum.request({
          method: 'personal_sign',
          params: [message, address],
        })) as string

        const res = await withdraw(address, selectedAsset, amount, signature, nonce)
        if (!res.ok) {
          setError(res.error ?? 'Withdrawal failed')
          setSubmitting(false)
          return
        }
        setSuccess({ amount, asset: selectedAsset, nonce, txHash: null })
      }
      setSubmitting(false)
    } catch (err) {
      setSubmitting(false)
      setError(err instanceof Error ? err.message : 'Withdrawal failed')
    }
  }

  if (!isConnected) {
    return <ConnectGate />
  }

  const shortAddress = address
    ? `${address.slice(0, 6)}...${address.slice(-4)}`
    : ''

  const isOnChain = selectedAsset === 'ETH'

  return (
    <div className="min-h-[calc(100vh-60px)] bg-parchment flex items-start justify-center px-4 py-12">
      <div className="w-full max-w-[480px]">
        <p className="text-[0.7rem] font-medium uppercase tracking-[0.15em] text-brown mb-6">
          WITHDRAW FUNDS
        </p>

        {success ? (
          <div
            className="bg-canvas border border-border p-8 flex flex-col items-center text-center"
            style={{ borderRadius: 0 }}
          >
            <svg
              width="48"
              height="48"
              viewBox="0 0 48 48"
              fill="none"
              className="mb-5"
            >
              <circle cx="24" cy="24" r="23" stroke="#00A090" strokeWidth="2" />
              <path
                d="M14 24l7 7 13-14"
                stroke="#00A090"
                strokeWidth="2.5"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
            </svg>
            {success.txHash ? (
              <>
                <p className="text-xl font-semibold text-ink mb-2">
                  Withdrawal submitted on-chain
                </p>
                <p className="text-sm text-brown mb-5">
                  Your {success.amount} ETH will arrive in your wallet once the
                  transaction confirms on Ethereum Sepolia.
                </p>
                <p className="text-[0.7rem] uppercase tracking-[0.12em] text-brown mb-1">
                  TRANSACTION HASH
                </p>
                <p
                  className="text-xs font-mono text-ink mb-4 break-all"
                  style={{ fontFamily: 'var(--font-mono)' }}
                >
                  {success.txHash}
                </p>
                <a
                  href={`https://sepolia.etherscan.io/tx/${success.txHash}`}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="w-full flex items-center justify-center h-10 text-[0.8rem] font-medium uppercase tracking-[0.08em] text-ink transition-colors duration-150 mb-4"
                  style={{ background: '#00D2D2', borderRadius: 0 }}
                >
                  VIEW ON ETHERSCAN
                </a>
              </>
            ) : (
              <>
                <p className="text-xl font-semibold text-ink mb-2">
                  Withdrawal requested
                </p>
                <p className="text-sm text-brown mb-5">
                  Your withdrawal of {success.amount} {success.asset} has been submitted.
                  This testnet request requires manual operator handling and has no guaranteed completion time.
                </p>
                <p className="text-[0.7rem] uppercase tracking-[0.12em] text-brown mb-1">
                  REFERENCE NUMBER
                </p>
                <p
                  className="text-sm font-mono text-ink mb-7"
                  style={{ fontFamily: 'var(--font-mono)' }}
                >
                  {success.nonce}
                </p>
              </>
            )}
            <Link
              href="/dashboard"
              className="w-full flex items-center justify-center h-10 text-[0.8rem] font-medium uppercase tracking-[0.08em] text-ink border border-border transition-colors duration-150 hover:bg-vellum"
              style={{ borderRadius: 0 }}
            >
              VIEW BALANCE
            </Link>
          </div>
        ) : (
          <form
            onSubmit={handleSubmit}
            className="bg-canvas border border-border p-8"
            style={{ borderRadius: 0 }}
          >
            <div className="mb-6">
              <p className="text-[0.7rem] font-medium uppercase tracking-[0.12em] text-brown mb-2">
                CONNECTED WALLET
              </p>
              <p
                className="text-sm"
                style={{ fontFamily: 'var(--font-mono)', color: '#7BA4B8' }}
              >
                {shortAddress}
              </p>
            </div>

            <div className="mb-5">
              <label className="block text-[0.7rem] font-medium uppercase tracking-[0.12em] text-brown mb-2">
                ASSET
              </label>
              <select
                value={selectedAsset}
                onChange={(e) => {
                  setSelectedAsset(e.target.value)
                  setAmount('')
                }}
                className="w-full h-10 px-3 bg-vellum border border-border text-sm text-ink appearance-none focus:outline-none focus:border-ink"
                style={{ borderRadius: 0, fontFamily: 'var(--font-mono)' }}
              >
                {ASSETS.map((a) => {
                  const bal = balances.find((b) => b.asset.toUpperCase() === a.toUpperCase())
                  const available = bal ? parseFloat(bal.available).toFixed(2) : '0.00'
                  return (
                    <option key={a} value={a}>
                      {a} — {available} available
                    </option>
                  )
                })}
              </select>
            </div>

            <div className="mb-5">
              <div
                style={{
                  borderLeft: '3px solid ' + (isOnChain ? '#00D2D2' : 'rgba(123,164,184,0.4)'),
                  background: isOnChain ? 'rgba(0,210,210,0.04)' : 'rgba(123,164,184,0.04)',
                  padding: '8px 12px',
                }}
              >
                <p className="text-[0.72rem] text-brown leading-relaxed">
                  {isOnChain
                    ? '● Ethereum Sepolia test withdrawal'
                    : '● Manual beta request; no completion-time guarantee'}
                </p>
                {!isOnChain && (
                  <p className="text-[0.68rem] text-brown mt-1 opacity-70">
                    ERC20 on-chain withdrawals coming soon
                  </p>
                )}
              </div>
            </div>

            <div className="mb-5">
              <label className="block text-[0.7rem] font-medium uppercase tracking-[0.12em] text-brown mb-2">
                AMOUNT
              </label>
              <div className="relative">
                <input
                  type="text"
                  value={amount}
                  onChange={(e) => setAmount(e.target.value)}
                  placeholder="0.00"
                  className={[
                    'w-full h-10 pl-3 pr-24 bg-vellum border text-sm text-ink focus:outline-none',
                    amountExceedsBalance
                      ? 'border-terra focus:border-terra'
                      : 'border-border focus:border-ink',
                  ].join(' ')}
                  style={{ borderRadius: 0, fontFamily: 'var(--font-mono)' }}
                />
                <div className="absolute right-3 top-1/2 -translate-y-1/2 flex items-center gap-2">
                  <button
                    type="button"
                    onClick={handleMax}
                    className="text-[0.65rem] font-medium uppercase tracking-[0.08em] text-brown hover:text-ink transition-colors duration-150"
                  >
                    MAX
                  </button>
                  <span
                    className="text-xs text-brown"
                    style={{ fontFamily: 'var(--font-mono)' }}
                  >
                    {selectedAsset}
                  </span>
                </div>
              </div>
              {amountExceedsBalance && (
                <p className="text-xs mt-1" style={{ color: '#00D2D2' }}>
                  Amount exceeds available balance
                </p>
              )}
            </div>

            <div className="mb-7">
              <p className="text-[0.7rem] font-medium uppercase tracking-[0.12em] text-brown mb-2">
                WITHDRAWAL DESTINATION
              </p>
              <p
                className="text-sm break-all"
                style={{ fontFamily: 'var(--font-mono)', color: '#7BA4B8' }}
              >
                {address}
              </p>
            </div>

            {error && (
              <p className="text-xs mb-4" style={{ color: '#00D2D2' }}>
                {error}
              </p>
            )}

            <button
              type="submit"
              disabled={submitting || !amount || amountExceedsBalance}
              className="w-full flex items-center justify-center gap-2 h-10 text-[0.8rem] font-medium uppercase tracking-[0.08em] text-ink transition-colors duration-150 disabled:opacity-50"
              style={{ background: '#00D2D2', borderRadius: 0 }}
            >
              {submitting && <Spinner size="xs" className="text-white" />}
              {submitting ? loadingMsg || 'Processing...' : `WITHDRAW ${amount || '0'} ${selectedAsset}`}
            </button>
          </form>
        )}

        <div className="mt-8">
          <p className="text-[0.7rem] font-medium uppercase tracking-[0.15em] text-brown mb-4">
            PENDING WITHDRAWALS
          </p>
          <div className="bg-canvas border border-border" style={{ borderRadius: 0 }}>
            <p className="text-sm text-brown text-center py-8">No pending withdrawals</p>
          </div>
        </div>
      </div>
    </div>
  )
}
