'use client'

import { useState, useEffect, useCallback, useRef } from 'react'
import { useRouter } from 'next/navigation'
import Link from 'next/link'
import HexCanvas from '@/components/HexCanvas'
import Skeleton from '@/components/ui/Skeleton'
import { COMPLETE_COUNT } from '@/lib/transparency-score'

const PF = "var(--font-inter-sans), 'Inter', sans-serif"
const IN = 'Inter, sans-serif'
const CN = "'Courier New', monospace"

const API_URL = process.env.NEXT_PUBLIC_API_URL ?? 'https://vela-engine.fly.dev'
const CONTRACT_ADDRESS = process.env.NEXT_PUBLIC_CONTRACT_ADDRESS ?? '0xAa8E680c11a883F9bf6eb980B2D4E9D18DD25686'
const ALCHEMY_URL = process.env.NEXT_PUBLIC_ALCHEMY_API_URL ?? 'https://eth-sepolia.g.alchemy.com/v2/demo'

interface StoredFill {
  id: string
  market_id: string
  price: number
  quantity: number
  maker_order_id: number
  taker_order_id: number
  maker_address: string
  taker_address: string
  timestamp: number
  side: string
}

function formatPrice(price: number): string {
  if (price >= 1000) return price.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 })
  if (price >= 1) return price.toFixed(2)
  return price.toFixed(4)
}

function truncateAddress(addr: string): string {
  if (!addr || addr.length < 12) return addr
  return `${addr.slice(0, 6)}...${addr.slice(-4)}`
}

const MARKETS = ['ALL', 'BTC-USDC', 'ETH-USDC', 'SOL-USDC']

export default function TransparencyPage() {
  const router = useRouter()
  const [contractEth, setContractEth] = useState<number | null>(null)
  const [engineEth, setEngineEth] = useState<number | null>(null)
  const [reservesUpdated, setReservesUpdated] = useState<Date | null>(null)
  const [trades, setTrades] = useState<StoredFill[]>([])
  const [tradesLoaded, setTradesLoaded] = useState(false)
  const [activeMarket, setActiveMarket] = useState('ALL')
  const [orderInput, setOrderInput] = useState('')
  const [feeData, setFeeData] = useState<{
    total_taker_fees_collected_usdc: string
    total_maker_rebates_paid_usdc: string
    net_exchange_revenue_usdc: string
    fees_collected_today_usdc: string
  } | null>(null)
  const refreshTimerRef = useRef<ReturnType<typeof setInterval> | null>(null)
  const feeTimerRef = useRef<ReturnType<typeof setInterval> | null>(null)

  const fetchContractBalance = useCallback(async () => {
    try {
      const res = await fetch(ALCHEMY_URL, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          jsonrpc: '2.0',
          method: 'eth_getBalance',
          params: [CONTRACT_ADDRESS, 'latest'],
          id: 1,
        }),
      })
      const data = await res.json()
      if (data.result) {
        const wei = parseInt(data.result, 16)
        setContractEth(wei / 1e18)
      }
    } catch {
      // silently ignore
    }
  }, [])

  const fetchEngineReserves = useCallback(async () => {
    try {
      const res = await fetch(`${API_URL}/reserves/public`)
      const data = await res.json()
      if (data.ok && data.data?.liabilities_by_asset?.ETH !== undefined) {
        setEngineEth(data.data.liabilities_by_asset.ETH / 1_000_000)
      }
    } catch {
      // silently ignore
    }
  }, [])

  const fetchReserves = useCallback(async () => {
    await Promise.all([fetchContractBalance(), fetchEngineReserves()])
    setReservesUpdated(new Date())
  }, [fetchContractBalance, fetchEngineReserves])

  const fetchTrades = useCallback(async () => {
    try {
      const url = activeMarket === 'ALL'
        ? `${API_URL}/trades`
        : `${API_URL}/trades/${activeMarket}`
      const res = await fetch(url)
      const data = await res.json()
      if (data.ok && Array.isArray(data.data)) {
        setTrades(data.data.slice(0, 50))
      }
      setTradesLoaded(true)
    } catch {
      // silently ignore
    }
  }, [activeMarket])

  const fetchFees = useCallback(async () => {
    try {
      const res = await fetch(`${API_URL}/fees/public`)
      const data = await res.json()
      if (data.ok) setFeeData(data.data)
    } catch {
      // silently ignore
    }
  }, [])

  useEffect(() => {
    fetchReserves()
    fetchTrades()
    fetchFees()
  }, [fetchReserves, fetchTrades, fetchFees])

  useEffect(() => {
    if (refreshTimerRef.current) clearInterval(refreshTimerRef.current)
    refreshTimerRef.current = setInterval(() => {
      fetchReserves()
    }, 60_000)
    return () => { if (refreshTimerRef.current) clearInterval(refreshTimerRef.current) }
  }, [fetchReserves])

  useEffect(() => {
    feeTimerRef.current = setInterval(fetchFees, 60_000)
    return () => { if (feeTimerRef.current) clearInterval(feeTimerRef.current) }
  }, [fetchFees])

  useEffect(() => {
    const t = setInterval(fetchTrades, 3_000)
    return () => clearInterval(t)
  }, [fetchTrades])

  useEffect(() => { fetchTrades() }, [activeMarket, fetchTrades])

  const coverageObserved = contractEth !== null && engineEth !== null && contractEth >= engineEth

  return (
    <div style={{ background: '#15131f', minHeight: '100vh' }}>
      <div style={{ position: 'relative', background: '#15131f', overflow: 'hidden' }} className="px-6 pt-12 pb-10 lg:px-[52px] lg:pt-[60px] lg:pb-[40px]">
        <HexCanvas />
        <div style={{ position: 'relative', zIndex: 1 }}>
          <p style={{ fontFamily: IN, fontSize: '10px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', marginBottom: '16px' }}>
            Vela — Public Transparency
          </p>
          <div>
            <span style={{ fontFamily: PF, fontWeight: 900, fontSize: '52px', color: '#f8f5ee', display: 'block', lineHeight: 1.1 }}>
              Transparency
            </span>
            <span style={{ fontFamily: PF, fontWeight: 400, fontStyle: 'italic', fontSize: '52px', color: 'rgba(232,234,240,0.3)', display: 'block', lineHeight: 1.1 }}>
              by default.
            </span>
          </div>
          <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '15px', lineHeight: 1.75, color: 'rgba(232,234,240,0.4)', maxWidth: '560px', marginTop: '20px' }}>
            Inspect the current testnet engine, aggregate demo liabilities, local commitments, and roadmap limitations. These views are operational telemetry, not production attestations.
          </p>
          <p style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.3)', marginTop: '16px' }}>
            {COMPLETE_COUNT}/25 transparency criteria complete ·{' '}
            <Link href="/transparency-score" style={{ color: 'rgba(232,234,240,0.45)', textDecoration: 'underline' }}>
              View scorecard →
            </Link>
          </p>
        </div>
      </div>

      <div style={{ background: '#13111d' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <p style={{ fontFamily: IN, fontSize: '10px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', marginBottom: '20px' }}>
          Testnet Balance Comparison
        </p>
        <h2 style={{ fontFamily: PF, fontWeight: 700, fontSize: '32px', color: '#f8f5ee', margin: 0 }}>
          Compare public testnet balances.
        </h2>
        <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '14px', lineHeight: 1.8, color: 'rgba(232,234,240,0.45)', maxWidth: '500px', marginTop: '12px', marginBottom: '32px' }}>
          This compares Sepolia ETH held by VelaSettlement with aggregate ETH credits reported by the demo engine. It is not a production proof of reserves or an independent reconciliation.
        </p>

        {contractEth === null ? (
          <div className="grid grid-cols-1 sm:grid-cols-3 gap-[1px]" style={{ background: 'rgba(232,234,240,0.05)' }}>
            {[0, 1, 2].map((i) => (
              <div key={i} style={{ background: '#211e2b', padding: '28px 32px' }}>
                <Skeleton className="w-28 h-2" />
                <Skeleton className="w-20 h-10 mt-3" />
                <Skeleton className="w-36 h-2 mt-2" />
              </div>
            ))}
          </div>
        ) : (
        <div className="grid grid-cols-1 sm:grid-cols-3 gap-[1px]" style={{ background: 'rgba(232,234,240,0.05)' }}>
          <div style={{ background: '#211e2b', padding: '28px 32px', flex: 1 }}>
            <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', margin: '0 0 8px' }}>
              ETH In Contract
            </p>
            <p style={{ fontFamily: PF, fontWeight: 900, fontSize: '36px', color: '#f8f5ee', margin: '0 0 6px' }}>
              {contractEth !== null ? `${contractEth.toFixed(4)} ETH` : '—'}
            </p>
            <p style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.35)', margin: '0 0 10px' }}>
              Locked in VelaSettlement.sol
            </p>
            <a
              href={`https://sepolia.etherscan.io/address/${CONTRACT_ADDRESS}`}
              target="_blank"
              rel="noopener noreferrer"
              style={{ fontFamily: IN, fontSize: '11px', color: '#f8f5ee', textDecoration: 'underline' }}
            >
              View on Etherscan →
            </a>
          </div>

          <div style={{ background: '#211e2b', padding: '28px 32px', flex: 1 }}>
            <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', margin: '0 0 8px' }}>
              ETH Credited
            </p>
            <p style={{ fontFamily: PF, fontWeight: 900, fontSize: '36px', color: '#f8f5ee', margin: '0 0 6px' }}>
              {engineEth !== null ? `${engineEth.toFixed(4)} ETH` : '—'}
            </p>
            <p style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.35)', margin: 0 }}>
              Aggregate demo-engine liability
            </p>
          </div>

          <div style={{ background: '#211e2b', padding: '28px 32px', flex: 1 }}>
            <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', margin: '0 0 12px' }}>
              Status
            </p>
            {contractEth === null || engineEth === null ? (
              <p style={{ fontFamily: IN, fontSize: '14px', color: 'rgba(232,234,240,0.3)', margin: 0 }}>Checking…</p>
            ) : coverageObserved ? (
              <>
                <div style={{ display: 'flex', alignItems: 'center', gap: '10px', marginBottom: '8px' }}>
                  <svg width="32" height="32" viewBox="0 0 32 32" fill="none">
                    <circle cx="16" cy="16" r="15" stroke="#20c98a" strokeWidth="2" />
                    <path d="M9 16.5L13.5 21L23 11" stroke="#20c98a" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" />
                  </svg>
                  <span style={{ fontFamily: IN, fontWeight: 700, fontSize: '18px', color: '#20c98a' }}>COVERAGE OBSERVED</span>
                </div>
                <p style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.4)', margin: 0 }}>
                  Sepolia contract ETH is at least the engine-reported ETH credits at this snapshot.
                </p>
              </>
            ) : (
              <>
                <div style={{ display: 'flex', alignItems: 'center', gap: '10px', marginBottom: '8px' }}>
                  <svg width="32" height="32" viewBox="0 0 32 32" fill="none">
                    <path d="M16 3L29 28H3L16 3Z" stroke="#ef6a5b" strokeWidth="2" fill="none" />
                    <path d="M16 12V18" stroke="#ef6a5b" strokeWidth="2.5" strokeLinecap="round" />
                    <circle cx="16" cy="23" r="1.5" fill="#ef6a5b" />
                  </svg>
                  <span style={{ fontFamily: IN, fontWeight: 700, fontSize: '18px', color: '#ef6a5b' }}>UNDERCOLLATERALIZED</span>
                </div>
                <p style={{ fontFamily: IN, fontSize: '11px', color: '#ef6a5b', margin: 0 }}>
                  Contract balance is below engine credits.
                </p>
              </>
            )}
          </div>
        </div>
        )}

        <div style={{ display: 'flex', alignItems: 'center', gap: '16px', marginTop: '20px' }}>
          <button
            onClick={fetchReserves}
            style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.5)', background: 'transparent', border: '1px solid rgba(232,234,240,0.15)', padding: '6px 14px', cursor: 'pointer' }}
          >
            Refresh
          </button>
          {reservesUpdated && (
            <span style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.3)' }}>
              Last updated {reservesUpdated.toLocaleTimeString()}
            </span>
          )}
        </div>

        <div style={{ marginTop: '20px' }}>
          <Link
            href="/batches"
            style={{ fontFamily: IN, fontSize: '10px', color: 'rgba(232,234,240,0.4)', textDecoration: 'none' }}
            onMouseEnter={(e) => (e.currentTarget.style.color = '#b8b1ff')}
            onMouseLeave={(e) => (e.currentTarget.style.color = 'rgba(232,234,240,0.4)')}
          >
            View on-chain state root anchors →
          </Link>
        </div>
      </div>

      <div style={{ background: '#13111d' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <p style={{ fontFamily: IN, fontSize: '10px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', margin: '0 0 12px' }}>
          Exchange Revenue
        </p>
        <h2 style={{ fontFamily: PF, fontWeight: 700, fontSize: '32px', color: '#f8f5ee', margin: '0 0 12px' }}>
          Published in real time.
        </h2>
        <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '14px', lineHeight: 1.8, color: 'rgba(232,234,240,0.45)', maxWidth: '500px', marginBottom: '32px' }}>
          This view reports fee counters emitted by the current demo engine. It is operational telemetry, not audited revenue or an authoritative financial ledger.
        </p>

        {!feeData ? (
          <div className="grid grid-cols-1 sm:grid-cols-3 gap-[1px]" style={{ background: 'rgba(232,234,240,0.05)' }}>
            {[0, 1, 2].map((i) => (
              <div key={i} style={{ background: '#211e2b', padding: '28px 32px' }}>
                <Skeleton className="w-28 h-2" />
                <Skeleton className="w-20 h-10 mt-3" />
                <Skeleton className="w-36 h-2 mt-2" />
              </div>
            ))}
          </div>
        ) : (
          <div className="grid grid-cols-1 sm:grid-cols-3 gap-[1px]" style={{ background: 'rgba(232,234,240,0.05)' }}>
            <div style={{ background: '#211e2b', padding: '28px 32px' }}>
              <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', margin: '0 0 8px' }}>
                Taker Fees Collected
              </p>
              <p style={{ fontFamily: PF, fontWeight: 900, fontSize: '32px', color: '#f8f5ee', margin: '0 0 6px' }}>
                ${feeData.total_taker_fees_collected_usdc}
              </p>
              <p style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.35)', margin: 0 }}>
                Since launch
              </p>
            </div>
            <div style={{ background: '#211e2b', padding: '28px 32px' }}>
              <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', margin: '0 0 8px' }}>
                Maker Rebates Paid
              </p>
              <p style={{ fontFamily: PF, fontWeight: 900, fontSize: '32px', color: '#20c98a', margin: '0 0 6px' }}>
                ${feeData.total_maker_rebates_paid_usdc}
              </p>
              <p style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.35)', margin: 0 }}>
                Paid to market makers
              </p>
            </div>
            <div style={{ background: '#211e2b', padding: '28px 32px' }}>
              <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', margin: '0 0 8px' }}>
                Net Exchange Revenue
              </p>
              <p style={{ fontFamily: PF, fontWeight: 900, fontSize: '32px', color: '#f8f5ee', margin: '0 0 6px' }}>
                ${feeData.net_exchange_revenue_usdc}
              </p>
              <p style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.35)', margin: 0 }}>
                Fees minus rebates
              </p>
            </div>
          </div>
        )}
        <p style={{ fontFamily: IN, fontSize: '9px', color: 'rgba(232,234,240,0.3)', marginTop: '16px' }}>
          Fee rate: -1 bps maker rebate · 5 bps taker fee · Net margin: 4 bps per matched trade
        </p>
      </div>

      <div style={{ position: 'relative', background: '#15131f', overflow: 'hidden' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <HexCanvas />
        <div style={{ position: 'relative', zIndex: 1 }}>
          <p style={{ fontFamily: IN, fontSize: '10px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', margin: '0 0 12px' }}>
            Live Trade Feed
          </p>
          <h2 style={{ fontFamily: PF, fontWeight: 700, fontSize: '32px', color: '#f8f5ee', margin: '0 0 12px' }}>
            Every trade. Unfiltered.
          </h2>
          <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '14px', lineHeight: 1.75, color: 'rgba(232,234,240,0.38)', maxWidth: '500px', marginBottom: '32px' }}>
            Testnet fills reported by the Vela engine are displayed here. No anonymization. No aggregation. Every counterparty, every price, every timestamp.
          </p>

          <div style={{ display: 'flex', borderBottom: '1px solid rgba(232,234,240,0.07)', marginBottom: '0', overflowX: 'auto' }}>
            {MARKETS.map((m) => (
              <button
                key={m}
                onClick={() => setActiveMarket(m)}
                style={{
                  fontFamily: IN,
                  fontSize: '10px',
                  textTransform: 'uppercase',
                  letterSpacing: '0.1em',
                  color: activeMarket === m ? '#f8f5ee' : 'rgba(232,234,240,0.28)',
                  background: 'transparent',
                  border: 'none',
                  borderBottom: activeMarket === m ? '2px solid #f8f5ee' : '2px solid transparent',
                  padding: '8px 16px',
                  cursor: 'pointer',
                  whiteSpace: 'nowrap',
                  marginBottom: '-1px',
                }}
              >
                {m}
              </button>
            ))}
          </div>

          <div style={{ padding: '8px 0', borderBottom: '1px solid rgba(232,234,240,0.06)' }} className="grid grid-cols-[1fr_80px_100px_80px] lg:grid-cols-[1fr_100px_120px_100px_180px_180px]">
            {['Time', 'Market', 'Price', 'Size', 'Maker', 'Taker'].map((h, i) => (
              <span key={h} style={{ fontFamily: IN, fontSize: '8px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.2)' }} className={i >= 4 ? 'hidden lg:block' : ''}>{h}</span>
            ))}
          </div>

          {!tradesLoaded ? (
            Array.from({ length: 8 }).map((_, i) => (
              <div
                key={i}
                style={{ padding: '9px 0', borderBottom: '1px solid rgba(232,234,240,0.04)', alignItems: 'center' }}
                className="grid grid-cols-[1fr_80px_100px_80px] lg:grid-cols-[1fr_100px_120px_100px_180px_180px]"
              >
                <Skeleton className="w-20 h-3" />
                <Skeleton className="w-14 h-3" />
                <Skeleton className="w-16 h-3" />
                <Skeleton className="w-12 h-3" />
                <Skeleton className="w-24 h-3 hidden lg:block" />
                <Skeleton className="w-24 h-3 hidden lg:block" />
              </div>
            ))
          ) : trades.length === 0 ? (
            <p style={{ fontFamily: IN, fontSize: '12px', color: 'rgba(232,234,240,0.2)', textAlign: 'center', padding: '48px 0' }}>
              No trades yet. Be the first to trade on Vela.
            </p>
          ) : (
            trades.map((trade) => {
              const priceNum = trade.price / 1e8
              const sizeNum = trade.quantity / 1e8
              return (
                <div
                  key={trade.id}
                  style={{ padding: '9px 0', borderBottom: '1px solid rgba(232,234,240,0.04)', alignItems: 'center' }}
                  className="grid grid-cols-[1fr_80px_100px_80px] lg:grid-cols-[1fr_100px_120px_100px_180px_180px]"
                >
                  <span style={{ fontFamily: CN, fontSize: '10px', color: 'rgba(232,234,240,0.3)' }}>
                    {new Date(trade.timestamp / 1000).toLocaleTimeString()}
                  </span>
                  <span style={{ fontFamily: IN, fontWeight: 500, fontSize: '11px', color: '#f8f5ee' }}>{trade.market_id}</span>
                  <span style={{ fontFamily: CN, fontSize: '11px', color: trade.side === 'bid' ? '#20c98a' : '#ef6a5b' }}>
                    {formatPrice(priceNum)}
                  </span>
                  <span style={{ fontFamily: CN, fontSize: '10px', color: 'rgba(232,234,240,0.6)' }}>{sizeNum.toFixed(4)}</span>
                  <a
                    href={`/orders/${trade.maker_order_id}`}
                    style={{ fontFamily: CN, fontSize: '10px', color: 'rgba(232,234,240,0.4)', textDecoration: 'none' }}
                    onMouseEnter={(e) => (e.currentTarget.style.color = 'rgba(232,234,240,0.7)')}
                    onMouseLeave={(e) => (e.currentTarget.style.color = 'rgba(232,234,240,0.4)')}
                    className="hidden lg:block"
                  >
                    {truncateAddress(trade.maker_address)}
                  </a>
                  <a
                    href={`/orders/${trade.taker_order_id}`}
                    style={{ fontFamily: CN, fontSize: '10px', color: 'rgba(232,234,240,0.4)', textDecoration: 'none' }}
                    onMouseEnter={(e) => (e.currentTarget.style.color = 'rgba(232,234,240,0.7)')}
                    onMouseLeave={(e) => (e.currentTarget.style.color = 'rgba(232,234,240,0.4)')}
                    className="hidden lg:block"
                  >
                    {truncateAddress(trade.taker_address)}
                  </a>
                </div>
              )
            })
          )}
        </div>
      </div>

      <div style={{ background: '#13111d' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <p style={{ fontFamily: IN, fontSize: '10px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', margin: '0 0 12px' }}>
          Order Audit Trail
        </p>
        <h2 style={{ fontFamily: PF, fontWeight: 700, fontSize: '32px', color: '#f8f5ee', margin: '0 0 12px' }}>
          Every order has a permanent record.
        </h2>
        <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '14px', lineHeight: 1.8, color: 'rgba(232,234,240,0.45)', maxWidth: '500px', marginBottom: '28px' }}>
          Testnet orders can be inspected at /orders/{'{id}'}. You can verify your order was handled correctly — the price it matched at, the counterparty, the exact timestamp.
        </p>

        <label style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.4)', display: 'block', marginBottom: '8px' }}>
          Look up an order ID:
        </label>
        <div className="flex flex-col sm:flex-row gap-0">
          <input
            type="text"
            value={orderInput}
            onChange={(e) => setOrderInput(e.target.value)}
            onKeyDown={(e) => { if (e.key === 'Enter' && orderInput.trim()) router.push(`/orders/${orderInput.trim()}`) }}
            placeholder="e.g. 42"
            style={{
              fontFamily: CN,
              fontSize: '13px',
              color: '#f8f5ee',
              background: '#211e2b',
              border: '1px solid rgba(232,234,240,0.15)',
              borderRight: 'none',
              padding: '10px 16px',
              outline: 'none',
              width: '280px',
            }}
            className="w-full sm:w-[280px]"
          />
          <button
            onClick={() => { if (orderInput.trim()) router.push(`/orders/${orderInput.trim()}`) }}
            style={{
              fontFamily: IN,
              fontSize: '12px',
              fontWeight: 600,
              color: '#f8f5ee',
              background: '#15131f',
              border: 'none',
              padding: '10px 20px',
              cursor: 'pointer',
            }}
          >
            Look up →
          </button>
        </div>
        <p style={{ fontFamily: IN, fontSize: '10px', color: 'rgba(232,234,240,0.3)', marginTop: '10px' }}>
          Order IDs are shown in your trade history after placing orders.
        </p>
      </div>

      <div style={{ background: '#13111d' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <p style={{ fontFamily: IN, fontSize: '10px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', margin: '0 0 12px' }}>
          Go Deeper
        </p>
        <h2 style={{ fontFamily: PF, fontWeight: 700, fontSize: '32px', color: '#f8f5ee', margin: '0 0 32px' }}>
          Go deeper.
        </h2>

        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-[1px]" style={{ background: 'rgba(232,234,240,0.07)' }}>
          {[
            {
              href: '/transparency-score',
              title: 'Transparency Score',
              desc: 'A self-assessment across 25 criteria. It is project telemetry, not an independent audit or industry standard.',
              link: 'View scorecard →',
            },
            {
              href: '/operator',
              title: 'Operator Disclosure',
              desc: 'Who runs Vela, what they can and cannot do, and a signed commitment to fair operation.',
              link: 'Read disclosure →',
            },
            {
              href: '/batches',
              title: 'Batch Explorer',
              desc: 'Inspect locally produced batch records and state roots. Anchoring is testnet telemetry and does not provide production data availability or proof enforcement.',
              link: 'Explore batches →',
            },
            {
              href: '/verify',
              title: 'Fraud Proof System',
              desc: 'Understand the planned proof architecture, inspect local state roots, and report a discrepancy. On-chain challenges are not active.',
              link: 'Verify trades →',
            },
            {
              href: '/status',
              title: 'System Status',
              desc: 'Live engine health, uptime metrics, and incident history. Published in real time.',
              link: 'View status →',
            },
            {
              href: '/analytics',
              title: 'Market Analytics',
              desc: 'Real-time spread, slippage estimates, and market depth for all 16 markets. Published openly.',
              link: 'View analytics →',
            },
            {
              href: '/proofs',
              title: 'ZK Proof System',
              desc: 'Vela\'s optimistic-ZK architecture. Infrastructure deployed, SP1 integration and full per-batch proving ships post-Stanford AFT Lab (June 2026).',
              link: 'View proof system →',
            },
          ].map((card) => (
            <a
              key={card.href}
              href={card.href}
              style={{ background: '#211e2b', padding: '28px', textDecoration: 'none', display: 'block' }}
              onMouseEnter={(e) => (e.currentTarget.style.background = '#141824')}
              onMouseLeave={(e) => (e.currentTarget.style.background = '#211e2b')}
            >
              <p style={{ fontFamily: IN, fontWeight: 600, fontSize: '14px', color: '#f8f5ee', margin: '0 0 10px' }}>{card.title}</p>
              <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '13px', color: 'rgba(232,234,240,0.55)', lineHeight: 1.8, margin: '0 0 16px' }}>{card.desc}</p>
              <span style={{ fontFamily: IN, fontSize: '11px', color: '#f8f5ee', textDecoration: 'underline' }}>{card.link}</span>
            </a>
          ))}
        </div>
      </div>
    </div>
  )
}
