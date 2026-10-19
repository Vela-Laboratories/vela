'use client'

import { useEffect, useRef, useState } from 'react'
import Link from 'next/link'
import { parseUnits } from 'viem'
import { useAuth } from '@/lib/auth'
import {
  deposit,
  getBalances,
  getDeposits,
  type BalanceResponse,
  type DepositStatus,
} from '@/lib/api'
import {
  depositETH,
  approveUSDC,
  depositToken,
  checkUSDCAllowance,
  getVerifiedDepositConfig,
  getUSDCBalance,
  waitForTransactionReceipt,
} from '@/lib/contract'
import { Spinner } from '@/components/ui/Spinner'

const ASSETS = [
  'USDC', 'ETH', 'BTC', 'SOL', 'AVAX', 'MATIC',
  'LINK', 'UNI', 'ARB', 'OP', 'AAVE', 'DOGE',
]

const ON_CHAIN_ASSETS = new Set(['ETH', 'USDC'])
const VERIFIED_DEPOSIT_CONFIG = getVerifiedDepositConfig()

function AssetBadge({ asset }: { asset: string }) {
  if (ON_CHAIN_ASSETS.has(asset)) {
    return (
      <span
        style={{
          fontSize: 8,
          letterSpacing: '0.1em',
          textTransform: 'uppercase',
          color: '#20c98a',
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
        color: 'rgba(232,234,240,0.25)',
        border: '1px solid rgba(232,234,240,0.08)',
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
  const { connect, isConnected } = useAuth()
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
    <div className="dippi-connect-gate min-h-[60vh] flex items-center justify-center px-4">
      <div className="dippi-connect-gate__card w-full max-w-sm text-center bg-canvas border border-border p-8">
        <div className="w-16 h-16 bg-ochre/10 flex items-center justify-center mx-auto mb-5">
          <svg width="28" height="28" viewBox="0 0 24 24" fill="none">
            <path
              d="M21 7H3C2.44772 7 2 7.44772 2 8V20C2 20.5523 2.44772 21 3 21H21C21.5523 21 22 20.5523 22 20V8C22 7.44772 21.5523 7 21 7Z"
              stroke="#9488ee"
              strokeWidth="1.5"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
            <circle cx="14" cy="14" r="2" fill="#9488ee" />
            <path
              d="M17 7V5C17 3.89543 16.1046 3 15 3H9C7.89543 3 7 3.89543 7 5V7"
              stroke="#9488ee"
              strokeWidth="1.5"
              strokeLinecap="round"
            />
          </svg>
        </div>
        <h2 className="text-xl font-semibold text-ink mb-2">
          {isConnected ? 'Authenticate your wallet' : 'Connect your wallet'}
        </h2>
        <p className="text-sm text-brown mb-6">
          Sign a one-time challenge to access your private deposit data.
        </p>
        {error && (
          <p className="text-xs text-red-600 mb-4">{error}</p>
        )}
        <button
          type="button"
          onClick={handleConnect}
          disabled={connecting}
          className="dippi-connect-gate__button w-full flex items-center justify-center gap-2 px-4 h-10 text-[0.8rem] font-medium disabled:opacity-50"
        >
          {connecting && <Spinner size="xs" className="text-ink" />}
          {isConnected ? 'Authenticate Wallet' : 'Connect Wallet'}
        </button>
        <p className="dippi-connect-gate__note">Testnet only · No real funds or production custody</p>
      </div>
    </div>
  )
}

interface SuccessState {
  amount: string
  asset: string
  creditType: 'verified' | 'demo'
  newBalance?: string
  txHash?: string
}

export default function DepositPage() {
  const { address, isConnected, isAuthenticated } = useAuth()

  const [selectedAsset, setSelectedAsset] = useState('USDC')
  const [amount, setAmount] = useState('')
  const [balances, setBalances] = useState<BalanceResponse[]>([])
  const [deposits, setDeposits] = useState<DepositStatus[]>([])
  const [submitting, setSubmitting] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [success, setSuccess] = useState<SuccessState | null>(null)
  const [stepLabel, setStepLabel] = useState<string | null>(null)
  const [usdcWalletBalance, setUsdcWalletBalance] = useState<string | null>(null)
  const [dropdownOpen, setDropdownOpen] = useState(false)
  const dropdownRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!address || !isAuthenticated) return
    getBalances(address).then((res) => {
      if (res.ok && res.data) setBalances(res.data)
    })
  }, [address, isAuthenticated])

  useEffect(() => {
    if (!address || !isAuthenticated || !VERIFIED_DEPOSIT_CONFIG) return
    let cancelled = false
    const refresh = async () => {
      const response = await getDeposits(address)
      if (!cancelled && response.ok && response.data) {
        setDeposits(response.data)
      }
    }
    void refresh()
    const interval = window.setInterval(refresh, 5_000)
    return () => {
      cancelled = true
      window.clearInterval(interval)
    }
  }, [address, isAuthenticated])

  useEffect(() => {
    if (!address || selectedAsset !== 'USDC' || !VERIFIED_DEPOSIT_CONFIG?.usdcAddress) {
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
      if (ON_CHAIN_ASSETS.has(selectedAsset)) {
        setStepLabel('Checking authoritative deposit tracking...')
        const tracking = await getDeposits(address)
        if (!tracking.ok || !tracking.data) {
          throw new Error(
            'Verified deposit tracking is unavailable. No transaction was submitted.',
          )
        }
        setDeposits(tracking.data)
      }
      if (selectedAsset === 'ETH') {
        setStepLabel('Confirm the verified deposit in your wallet...')
        const txHash = await depositETH(amount)
        setStepLabel('Waiting for the Base transaction to confirm...')
        await waitForTransactionReceipt(txHash)
        setSubmitting(false)
        setStepLabel(null)
        setSuccess({
          amount,
          asset: selectedAsset,
          creditType: 'verified',
          txHash,
        })
      } else if (selectedAsset === 'USDC') {
        const amountWei = parseUnits(amount, 6).toString()
        const allowance = await checkUSDCAllowance(address)

        if (allowance < BigInt(amountWei)) {
          setStepLabel('Step 1 of 2: Approving USDC...')
          const approvalHash = await approveUSDC(amount)
          await waitForTransactionReceipt(approvalHash)
          setStepLabel('Approval confirmed. Proceeding to deposit...')
        }

        setStepLabel('Step 2 of 2: Depositing USDC...')
        const txHash = await depositToken(amountWei)
        setStepLabel('Waiting for the Base transaction to confirm...')
        await waitForTransactionReceipt(txHash)
        setSubmitting(false)
        setStepLabel(null)
        setSuccess({
          amount,
          asset: selectedAsset,
          creditType: 'verified',
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
          creditType: 'demo',
          newBalance: String(Number(newBal?.available ?? '0') / 1_000_000),
        })
      }
    } catch (err) {
      setSubmitting(false)
      setStepLabel(null)
      setError(err instanceof Error ? err.message : 'Deposit failed')
    }
  }

  if (!isConnected || !isAuthenticated) {
    return <ConnectGate />
  }

  const shortAddress = address
    ? `${address.slice(0, 6)}...${address.slice(-4)}`
    : ''
  const selectedVerifiedDepositReady = Boolean(
    VERIFIED_DEPOSIT_CONFIG
      && (selectedAsset === 'ETH'
        || (selectedAsset === 'USDC' && VERIFIED_DEPOSIT_CONFIG.usdcAddress)),
  )
  const submittedDeposit = success?.txHash
    ? deposits.find(
        (item) => item.transaction_hash.toLowerCase() === success.txHash?.toLowerCase(),
      )
    : undefined

  return (
    <div className="dippi-funding-page min-h-[calc(100vh-60px)] bg-parchment flex items-start justify-center px-4 py-12">
      <div className="w-full max-w-[480px]">
        <p className="text-[0.7rem] font-medium uppercase tracking-[0.15em] text-brown mb-6">
          DEPOSIT FUNDS
        </p>
        <p className="dippi-funding-page__intro">Add testnet funds to explore the Vela sandbox. Check the credit type and network before continuing.</p>

        <div
          className="dippi-funding-notice mb-6"
          style={{
            borderLeft: '3px solid #9488ee',
            background: 'rgba(0,210,210,0.04)',
            padding: '12px 16px',
          }}
        >
          <p className="text-[0.8rem] text-ink leading-relaxed">
            Verified deposits are credited only after the Base indexer observes and finalizes the
            configured custody-vault event. Sending a transaction never creates an immediate demo
            credit.
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
              {success.creditType === 'verified' ? 'Deposit submitted' : 'Demo credit created'}
            </p>
            <p className="text-sm text-brown mb-5">
              {success.creditType === 'verified'
                ? `The Base transaction confirmed. Authoritative status: ${submittedDeposit?.state ?? 'awaiting indexer discovery'}. No demo balance was created.`
                : `Your ${success.amount} ${success.asset} unverified demo balance was credited.`}
            </p>
            {success.creditType === 'verified' ? (
              <>
                <p className="text-[0.7rem] uppercase tracking-[0.12em] text-brown mb-1">
                  AMOUNT
                </p>
                <p className="text-lg font-mono text-ink mb-5" style={{ fontFamily: 'var(--font-mono)' }}>
                  {success.amount} {success.asset}
                </p>
              </>
            ) : (
              <>
                <p className="text-[0.7rem] uppercase tracking-[0.12em] text-brown mb-1">
                  NEW DEMO BALANCE
                </p>
                <p className="text-lg font-mono text-ink mb-5" style={{ fontFamily: 'var(--font-mono)' }}>
                  {success.newBalance} {success.asset}
                </p>
              </>
            )}
            {success.txHash && VERIFIED_DEPOSIT_CONFIG && (
              <a
                href={`${VERIFIED_DEPOSIT_CONFIG.explorerUrl}/tx/${success.txHash}`}
                target="_blank"
                rel="noopener noreferrer"
                className="text-[0.7rem] font-medium uppercase tracking-[0.12em] mb-7"
                style={{ color: '#9488ee' }}
              >
                VIEW ON BLOCK EXPLORER ↗
              </a>
            )}
            {!success.txHash && <div className="mb-7" />}
            <Link
              href="/markets/ETH-USDC"
              className="w-full flex items-center justify-center h-10 text-[0.8rem] font-medium uppercase tracking-[0.08em] text-ink transition-colors duration-150"
              style={{ background: '#9488ee', borderRadius: 0 }}
            >
              VIEW MARKETS
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
                <p className="text-[0.72rem] font-medium" style={{ color: '#9488ee' }}>
                  {selectedVerifiedDepositReady
                    ? `● ${VERIFIED_DEPOSIT_CONFIG?.chainName} verified custody deposit`
                    : '● Verified deposits unavailable until the Base vault is configured'}
                </p>
              ) : (
                <p className="text-[0.72rem] font-medium text-brown">
                  ● Trust-based beta deposit
                </p>
              )}
              {selectedAsset === 'USDC' && selectedVerifiedDepositReady && (
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
                      color: 'rgba(232,234,240,0.3)',
                      marginTop: 4,
                      display: 'inline-block',
                      textDecoration: 'none',
                    }}
                  >
                    Get testnet USDC from Circle faucet →
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
              <p className="text-xs mb-4" style={{ color: '#9488ee' }}>
                {error}
              </p>
            )}

            {stepLabel && (
              <p
                className="text-xs mb-3"
                style={{ color: 'rgba(232,234,240,0.35)', fontSize: 10, fontFamily: 'var(--font-inter)' }}
              >
                {stepLabel}
              </p>
            )}

            <button
              type="submit"
              disabled={
                submitting
                || !amount
                || (ON_CHAIN_ASSETS.has(selectedAsset) && !selectedVerifiedDepositReady)
              }
              className="w-full flex items-center justify-center gap-2 h-10 text-[0.8rem] font-medium uppercase tracking-[0.08em] text-ink transition-colors duration-150 disabled:opacity-50"
              style={{ background: '#9488ee', borderRadius: 0 }}
            >
              {submitting && <Spinner size="xs" className="text-white" />}
              DEPOSIT {amount || '0'} {selectedAsset}
            </button>
          </form>
        )}

        {VERIFIED_DEPOSIT_CONFIG && deposits.length > 0 && (
          <section className="mt-6 bg-canvas border border-border p-5" aria-label="Verified deposit activity">
            <p className="text-[0.7rem] font-medium uppercase tracking-[0.12em] text-brown mb-4">
              VERIFIED DEPOSIT ACTIVITY
            </p>
            <div className="space-y-3">
              {deposits.map((item) => (
                <a
                  key={item.id}
                  href={`${VERIFIED_DEPOSIT_CONFIG.explorerUrl}/tx/${item.transaction_hash}`}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="flex items-center justify-between gap-4 border-t border-border pt-3"
                >
                  <span>
                    <span className="block text-sm text-ink" style={{ fontFamily: 'var(--font-mono)' }}>
                      {item.amount} {item.asset}
                    </span>
                    <span className="block text-[0.65rem] text-brown mt-1" style={{ fontFamily: 'var(--font-mono)' }}>
                      {item.transaction_hash.slice(0, 10)}…{item.transaction_hash.slice(-8)}
                    </span>
                  </span>
                  <span className="text-[0.65rem] uppercase tracking-[0.1em]" style={{ color: '#00A090' }}>
                    {item.state}
                  </span>
                </a>
              ))}
            </div>
          </section>
        )}
      </div>
    </div>
  )
}
