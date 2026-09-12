'use client'

import { useEffect, useRef, useState } from 'react'
import Link from 'next/link'
import { useAuth } from '@/lib/auth'
import { deposit, getBalances, type BalanceResponse } from '@/lib/api'
import {
  depositETH,
  switchToSepolia,
  approveUSDC,
  depositToken,
  checkUSDCAllowance,
  getUSDCBalance,
  SEPOLIA_USDC,
} from '@/lib/contract'
import { Spinner } from '@/components/ui/Spinner'

const ASSETS = [
  'USDC', 'ETH', 'BTC', 'SOL', 'AVAX', 'MATIC',
  'LINK', 'UNI', 'ARB', 'OP', 'AAVE', 'DOGE',
]

const ON_CHAIN_ASSETS = new Set(['ETH', 'USDC'])

function AssetBadge({ asset }: { asset: string }) {
  if (ON_CHAIN_ASSETS.has(asset)) {
    return (
      <span
        style={{
          fontSize: 8,
          letterSpacing: '0.1em',
          textTransform: 'uppercase',
          color: '#6B8A5A',
          border: '1px solid rgba(107,138,90,0.3)',
          padding: '1px 5px',
          marginLeft: 6,
          borderRadius: 0,
        }}
      >
        On-chain
      </span>
    )
  }
  return (
    <span
      style={{
        fontSize: 8,
        letterSpacing: '0.1em',
        textTransform: 'uppercase',
        color: 'rgba(232,228,216,0.25)',
        border: '1px solid rgba(232,228,216,0.08)',
        padding: '1px 5px',
        marginLeft: 6,
        borderRadius: 0,
      }}
    >
      Trust-based
    </span>
  )
}

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
        <p className="text-sm text-brown mb-6">Connect your wallet to deposit funds.</p>
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
  newBalance: string
  txHash?: string
}

export default function DepositPage() {
  const { address, isConnected } = useAuth()

  const [selectedAsset, setSelectedAsset] = useState('USDC')
  const [amount, setAmount] = useState('')
  const [balances, setBalances] = useState<BalanceResponse[]>([])
  const [submitting, setSubmitting] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [success, setSuccess] = useState<SuccessState | null>(null)
  const [stepLabel, setStepLabel] = useState<string | null>(null)
  const [usdcWalletBalance, setUsdcWalletBalance] = useState<string | null>(null)
  const [dropdownOpen, setDropdownOpen] = useState(false)
  const dropdownRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!address) return
    getBalances(address).then((res) => {
      if (res.ok && res.data) setBalances(res.data)
    })
  }, [address])

  useEffect(() => {
    if (!address || selectedAsset !== 'USDC') {
      setUsdcWalletBalance(null)
      return
    }
    getUSDCBalance(address).then(setUsdcWalletBalance).catch(() => setUsdcWalletBalance(null))
  }, [address, selectedAsset])

  useEffect(() => {
    function handleClickOutside(e: MouseEvent) {
      if (dropdownRef.current && !dropdownRef.current.contains(e.target as Node)) {
        setDropdownOpen(false)
      }
    }
    if (dropdownOpen) {
      document.addEventListener('mousedown', handleClickOutside)
    }
    return () => document.removeEventListener('mousedown', handleClickOutside)
  }, [dropdownOpen])

  const currentBalance = balances.find(
    (b) => b.asset.toUpperCase() === selectedAsset.toUpperCase(),
  )

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    if (!address || !amount) return
    setSubmitting(true)
    setError(null)
    setStepLabel(null)

    try {
      if (selectedAsset === 'ETH') {
        await switchToSepolia()
        const txHash = await depositETH(amount)
        const res = await deposit(address, selectedAsset, amount)
        setSubmitting(false)

        if (!res.ok || !res.data) {
          setError(res.error ?? 'Engine credit failed — on-chain tx sent: ' + txHash)
          return
        }
        const updated = res.data.balances
        setBalances(updated)
        const newBal = updated.find(
          (b) => b.asset.toUpperCase() === selectedAsset.toUpperCase(),
        )
        setSuccess({
          amount,
          asset: selectedAsset,
          newBalance: String(Number(newBal?.available ?? '0') / 1_000_000),
          txHash,
        })
      } else if (selectedAsset === 'USDC') {
        await switchToSepolia()
        const amountWei = Math.round(parseFloat(amount) * 1_000_000).toString()
        const allowance = await checkUSDCAllowance(address)

        if (allowance < BigInt(amountWei)) {
          setStepLabel('Step 1 of 2: Approving USDC...')
          await approveUSDC(amount)
          setStepLabel('Approval confirmed. Proceeding to deposit...')
        }

        setStepLabel('Step 2 of 2: Depositing USDC...')
        const txHash = await depositToken(SEPOLIA_USDC, amountWei)

        const res = await deposit(address, selectedAsset, amount)
        setSubmitting(false)
        setStepLabel(null)

        if (!res.ok || !res.data) {
          setError(res.error ?? 'Engine credit failed — on-chain tx sent: ' + txHash)
          return
        }
        const updated = res.data.balances
        setBalances(updated)
        const newBal = updated.find((b) => b.asset.toUpperCase() === 'USDC')
        setSuccess({
          amount,
          asset: selectedAsset,
          newBalance: String(Number(newBal?.available ?? '0') / 1_000_000),
          txHash,
        })
      } else {
        const res = await deposit(address, selectedAsset, amount)
        setSubmitting(false)

        if (!res.ok || !res.data) {
          setError(res.error ?? 'Deposit failed')
          return
        }

        const updated = res.data.balances
        setBalances(updated)
        const newBal = updated.find(
          (b) => b.asset.toUpperCase() === selectedAsset.toUpperCase(),
        )
        setSuccess({
          amount,
          asset: selectedAsset,
          newBalance: String(Number(newBal?.available ?? '0') / 1_000_000),
        })
      }
    } catch (err) {
      setSubmitting(false)
      setStepLabel(null)
      setError(err instanceof Error ? err.message : 'Deposit failed')
    }
  }

  if (!isConnected) {
    return <ConnectGate />
  }

  const shortAddress = address
    ? `${address.slice(0, 6)}...${address.slice(-4)}`
    : ''

  return (
    <div className="min-h-[calc(100vh-60px)] bg-parchment flex items-start justify-center px-4 py-12">
      <div className="w-full max-w-[480px]">
        <p className="text-[0.7rem] font-medium uppercase tracking-[0.15em] text-brown mb-6">
          DEPOSIT FUNDS
        </p>

        <div
          className="mb-6"
          style={{
            borderLeft: '3px solid #00D2D2',
            background: 'rgba(0,210,210,0.04)',
            padding: '12px 16px',
          }}
        >
          <p className="text-[0.8rem] text-ink leading-relaxed">
            Testnet demo only. Deposits use Sepolia and engine balances are unverified demo credits, not production custody.
          </p>
        </div>

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
            <p className="text-xl font-semibold text-ink mb-2">
              Deposit successful
            </p>
            <p className="text-sm text-brown mb-5">
              {success.txHash
                ? `Sepolia transaction submitted. Your ${success.amount} ${success.asset} testnet demo balance was credited.`
                : `Your ${success.amount} ${success.asset} unverified demo balance was credited.`}
            </p>
            <p className="text-[0.7rem] uppercase tracking-[0.12em] text-brown mb-1">
              NEW BALANCE
            </p>
            <p
              className="text-lg font-mono text-ink mb-5"
              style={{ fontFamily: 'var(--font-mono)' }}
            >
              {success.newBalance} {success.asset}
            </p>
            {success.txHash && (
              <a
                href={`https://sepolia.etherscan.io/tx/${success.txHash}`}
                target="_blank"
                rel="noopener noreferrer"
                className="text-[0.7rem] font-medium uppercase tracking-[0.12em] mb-7"
                style={{ color: '#00D2D2' }}
              >
                VIEW ON ETHERSCAN ↗
              </a>
            )}
            {!success.txHash && <div className="mb-7" />}
            <Link
              href="/markets/ETH-USDC"
              className="w-full flex items-center justify-center h-10 text-[0.8rem] font-medium uppercase tracking-[0.08em] text-ink transition-colors duration-150"
              style={{ background: '#00D2D2', borderRadius: 0 }}
            >
              START TRADING
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
                className="text-sm text-ink"
                style={{ fontFamily: 'var(--font-mono)', color: '#7BA4B8' }}
              >
                {shortAddress}
              </p>
            </div>

            <div className="mb-2">
              <label className="block text-[0.7rem] font-medium uppercase tracking-[0.12em] text-brown mb-2">
                ASSET
              </label>
              <div className="relative" ref={dropdownRef}>
                <button
                  type="button"
                  onClick={() => setDropdownOpen((o) => !o)}
                  className="w-full h-10 px-3 bg-vellum border border-border text-sm text-ink flex items-center justify-between focus:outline-none focus:border-ink"
                  style={{ borderRadius: 0, fontFamily: 'var(--font-mono)' }}
                >
                  <span className="flex items-center">
                    {selectedAsset}
                    <AssetBadge asset={selectedAsset} />
                  </span>
                  <svg width="12" height="12" viewBox="0 0 12 12" fill="none" style={{ opacity: 0.4 }}>
                    <path d="M2 4l4 4 4-4" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
                  </svg>
                </button>
                {dropdownOpen && (
                  <div
                    className="absolute z-10 w-full bg-vellum border border-border"
                    style={{ borderRadius: 0, top: '100%', left: 0 }}
                  >
                    {ASSETS.map((a) => (
                      <button
                        key={a}
                        type="button"
                        onClick={() => {
                          setSelectedAsset(a)
                          setDropdownOpen(false)
                        }}
                        className="w-full px-3 h-9 text-left text-sm text-ink flex items-center hover:bg-canvas transition-colors"
                        style={{ borderRadius: 0, fontFamily: 'var(--font-mono)' }}
                      >
                        {a}
                        <AssetBadge asset={a} />
                      </button>
                    ))}
                  </div>
                )}
              </div>
            </div>

            <div className="mb-5">
              {ON_CHAIN_ASSETS.has(selectedAsset) ? (
                <p className="text-[0.72rem] font-medium" style={{ color: '#00D2D2' }}>
                  ● Ethereum Sepolia — on-chain settlement
                </p>
              ) : (
                <p className="text-[0.72rem] font-medium text-brown">
                  ● Trust-based beta deposit
                </p>
              )}
              {selectedAsset === 'USDC' && (
                <div className="mt-2">
                  <p className="text-[0.7rem] font-medium uppercase tracking-[0.12em] text-brown mb-1">
                    WALLET USDC BALANCE
                  </p>
                  <p
                    className="text-sm text-ink mb-1"
                    style={{ fontFamily: 'var(--font-mono)' }}
                  >
                    {usdcWalletBalance !== null ? `${usdcWalletBalance} USDC` : '—'}
                  </p>
                  <a
                    href="https://faucet.circle.com"
                    target="_blank"
                    rel="noopener noreferrer"
                    style={{
                      fontFamily: 'var(--font-inter)',
                      fontSize: 10,
                      color: 'rgba(232,228,216,0.3)',
                      marginTop: 4,
                      display: 'inline-block',
                      textDecoration: 'none',
                    }}
                  >
                    Get Sepolia USDC from Circle faucet →
                  </a>
                </div>
              )}
              {!ON_CHAIN_ASSETS.has(selectedAsset) && (
                <p className="text-[0.7rem] text-brown mt-1">
                  This asset uses trust-based deposits during beta.
                </p>
              )}
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
                  className="w-full h-10 pl-3 pr-16 bg-vellum border border-border text-sm text-ink focus:outline-none focus:border-ink"
                  style={{ borderRadius: 0, fontFamily: 'var(--font-mono)' }}
                />
                <span
                  className="absolute right-3 top-1/2 -translate-y-1/2 text-xs text-brown"
                  style={{ fontFamily: 'var(--font-mono)' }}
                >
                  {selectedAsset}
                </span>
              </div>
            </div>

            <div className="mb-7">
              <p className="text-[0.7rem] font-medium uppercase tracking-[0.12em] text-brown mb-1">
                CURRENT BALANCE
              </p>
              <p
                className="text-sm text-ink"
                style={{ fontFamily: 'var(--font-mono)' }}
              >
                {currentBalance ? String(Number(currentBalance.available) / 1_000_000) : '0'}{' '}
                {selectedAsset}
              </p>
            </div>

            {error && (
              <p className="text-xs mb-4" style={{ color: '#00D2D2' }}>
                {error}
              </p>
            )}

            {stepLabel && (
              <p
                className="text-xs mb-3"
                style={{ color: 'rgba(232,228,216,0.35)', fontSize: 10, fontFamily: 'var(--font-inter)' }}
              >
                {stepLabel}
              </p>
            )}

            <button
              type="submit"
              disabled={submitting || !amount}
              className="w-full flex items-center justify-center gap-2 h-10 text-[0.8rem] font-medium uppercase tracking-[0.08em] text-ink transition-colors duration-150 disabled:opacity-50"
              style={{ background: '#00D2D2', borderRadius: 0 }}
            >
              {submitting && <Spinner size="xs" className="text-white" />}
              DEPOSIT {amount || '0'} {selectedAsset}
            </button>
          </form>
        )}
      </div>
    </div>
  )
}
