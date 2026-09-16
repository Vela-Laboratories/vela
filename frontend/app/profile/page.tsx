'use client'

import Link from 'next/link'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { useAuth } from '@/lib/auth'
import {
  getAccountFills,
  getBalances,
  getDeposits,
  getLeaderboard,
  getOrders,
  getPortfolio,
  getReferral,
  getWithdrawals,
  type AccountFill,
  type BalanceResponse,
  type DepositStatus,
  type LeaderboardData,
  type Order,
  type PortfolioResponse,
  type ReferralData,
  type WithdrawalStatus,
} from '@/lib/api'
import { Spinner } from '@/components/ui/Spinner'
import TokenIcon from '@/components/TokenIcon'
import { fetchReferenceMarkets, type ReferenceMarket } from '@/lib/market-data'

interface ProfileData {
  balances: BalanceResponse[]
  orders: Order[]
  fills: AccountFill[]
  deposits: DepositStatus[]
  withdrawals: WithdrawalStatus[]
  portfolio: PortfolioResponse | null
  referral: ReferralData | null
  leaderboard: LeaderboardData | null
  markets: ReferenceMarket[]
}

interface ActivityItem {
  id: string
  type: 'Trade' | 'Deposit' | 'Withdrawal'
  title: string
  detail: string
  status: string
  timestamp: number
  tone: 'buy' | 'sell' | 'neutral'
}

const EMPTY_DATA: ProfileData = {
  balances: [],
  orders: [],
  fills: [],
  deposits: [],
  withdrawals: [],
  portfolio: null,
  referral: null,
  leaderboard: null,
  markets: [],
}

function toNumber(value: string | number | undefined): number {
  const number = Number(value)
  return Number.isFinite(number) ? number : 0
}

function formatMoney(value: number, maximumFractionDigits = 2): string {
  return value.toLocaleString('en-US', {
    style: 'currency',
    currency: 'USD',
    minimumFractionDigits: 2,
    maximumFractionDigits,
  })
}

function formatQuantity(value: string): string {
  const quantity = toNumber(value)
  return quantity.toLocaleString('en-US', { maximumFractionDigits: 6 })
}

function formatMicroUsdc(value: string | undefined): string {
  return formatMoney(toNumber(value) / 1_000_000)
}

function normalizeTimestamp(value: number): number {
  return value < 1_000_000_000_000 ? value * 1_000 : value
}

function timeAgo(timestamp: number): string {
  const elapsed = Date.now() - timestamp
  if (elapsed < 60_000) return 'Just now'
  if (elapsed < 3_600_000) return `${Math.floor(elapsed / 60_000)}m ago`
  if (elapsed < 86_400_000) return `${Math.floor(elapsed / 3_600_000)}h ago`
  return new Date(timestamp).toLocaleDateString('en-US', { month: 'short', day: 'numeric' })
}

function marketPrice(asset: string, markets: ReferenceMarket[]): number {
  if (asset.toUpperCase() === 'USDC') return 1
  const market = markets.find((item) => item.market.split('-')[0] === asset.toUpperCase())
  return market?.price ?? 0
}

function ConnectProfile({ onConnect, isConnected }: { onConnect: () => Promise<string>; isConnected: boolean }) {
  const [connecting, setConnecting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function handleConnect() {
    setConnecting(true)
    setError(null)
    try {
      await onConnect()
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : 'Wallet connection failed.')
    } finally {
      setConnecting(false)
    }
  }

  return (
    <div className="profile-gate">
      <div className="profile-gate__card">
        <div className="profile-avatar profile-avatar--large">V</div>
        <span className="profile-eyebrow">Your Vela profile</span>
        <h1>{isConnected ? 'Authenticate your wallet' : 'Connect your wallet'}</h1>
        <p>Your balances, positions, orders, funding history, and trading activity stay private until you authenticate.</p>
        {error ? <div className="profile-error">{error}</div> : null}
        <button type="button" onClick={handleConnect} disabled={connecting} className="profile-primary-button">
          {connecting ? <Spinner size="xs" /> : null}
          {isConnected ? 'Authenticate wallet' : 'Connect wallet'}
        </button>
        <Link href="/markets/BTC-USDC">Continue browsing markets</Link>
      </div>
    </div>
  )
}

export default function ProfilePage() {
  const { address, isConnected, isAuthenticated, connect } = useAuth()
  const [data, setData] = useState<ProfileData>(EMPTY_DATA)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [copied, setCopied] = useState(false)

  const loadProfile = useCallback(async () => {
    if (!address || !isAuthenticated) return
    setLoading(true)
    setError(null)
    const [balances, orders, fills, deposits, withdrawals, portfolio, referral, leaderboard, marketSnapshot] = await Promise.all([
      getBalances(address),
      getOrders(address),
      getAccountFills(address),
      getDeposits(address),
      getWithdrawals(address),
      getPortfolio(address),
      getReferral(address),
      getLeaderboard(),
      fetchReferenceMarkets().catch(() => null),
    ])

    const successfulResponses = [balances, orders, fills, deposits, withdrawals].filter((response) => response.ok).length
    setData({
      balances: balances.ok ? balances.data ?? [] : [],
      orders: orders.ok ? orders.data ?? [] : [],
      fills: fills.ok ? fills.data ?? [] : [],
      deposits: deposits.ok ? deposits.data ?? [] : [],
      withdrawals: withdrawals.ok ? withdrawals.data ?? [] : [],
      portfolio: portfolio.ok ? portfolio.data ?? null : null,
      referral: referral.ok ? referral.data ?? null : null,
      leaderboard: leaderboard.ok ? leaderboard.data ?? null : null,
      markets: marketSnapshot?.markets ?? [],
    })
    if (successfulResponses === 0) setError('Account data is temporarily unavailable. Your funds and orders have not been changed.')
    setLoading(false)
  }, [address, isAuthenticated])

  useEffect(() => {
    const initial = window.setTimeout(() => void loadProfile(), 0)
    return () => window.clearTimeout(initial)
  }, [loadProfile])

  const openOrders = useMemo(
    () => data.orders.filter((order) => order.status === 'open' || order.status === 'partial'),
    [data.orders],
  )

  const cashBalance = useMemo(
    () => toNumber(data.balances.find((balance) => balance.asset.toUpperCase() === 'USDC')?.available),
    [data.balances],
  )

  const accountValue = useMemo(
    () => data.balances.reduce((total, balance) => total + toNumber(balance.total) * marketPrice(balance.asset, data.markets), 0),
    [data.balances, data.markets],
  )

  const positions = useMemo(
    () => data.balances
      .filter((balance) => balance.asset.toUpperCase() !== 'USDC' && toNumber(balance.total) > 0)
      .map((balance) => ({
        ...balance,
        price: marketPrice(balance.asset, data.markets),
        value: toNumber(balance.total) * marketPrice(balance.asset, data.markets),
      }))
      .sort((left, right) => right.value - left.value),
    [data.balances, data.markets],
  )

  const traderRank = useMemo(() => {
    if (!address || !data.leaderboard) return null
    const index = data.leaderboard.top_traders.findIndex((trader) => trader.address.toLowerCase() === address.toLowerCase())
    return index === -1 ? null : index + 1
  }, [address, data.leaderboard])

  const activity = useMemo<ActivityItem[]>(() => {
    const fills: ActivityItem[] = data.fills.map((fill) => ({
      id: `fill-${fill.id}`,
      type: 'Trade',
      title: `${fill.side === 'buy' ? 'Bought' : 'Sold'} ${fill.market.split('-')[0]}`,
      detail: `${formatQuantity(fill.quantity)} at ${formatMoney(toNumber(fill.price), 6)}`,
      status: fill.role,
      timestamp: normalizeTimestamp(fill.timestamp),
      tone: fill.side,
    }))
    const deposits: ActivityItem[] = data.deposits.map((deposit) => ({
      id: `deposit-${deposit.id}`,
      type: 'Deposit',
      title: `Deposited ${deposit.asset}`,
      detail: `${formatQuantity(deposit.amount)} ${deposit.asset}`,
      status: deposit.state,
      timestamp: new Date(deposit.updated_at).getTime(),
      tone: 'neutral',
    }))
    const withdrawals: ActivityItem[] = data.withdrawals.map((withdrawal) => ({
      id: `withdrawal-${withdrawal.id}`,
      type: 'Withdrawal',
      title: `Withdrew ${withdrawal.asset}`,
      detail: `${formatQuantity(withdrawal.amount)} ${withdrawal.asset}`,
      status: withdrawal.state,
      timestamp: new Date(withdrawal.created_at).getTime(),
      tone: 'neutral',
    }))
    return [...fills, ...deposits, ...withdrawals]
      .filter((item) => Number.isFinite(item.timestamp))
      .sort((left, right) => right.timestamp - left.timestamp)
      .slice(0, 8)
  }, [data.deposits, data.fills, data.withdrawals])

  const totalPnl = useMemo(() => {
    if (!data.portfolio) return null
    return (toNumber(data.portfolio.realized_pnl_usdc) + toNumber(data.portfolio.unrealized_pnl_usdc)) / 1_000_000
  }, [data.portfolio])

  async function copyAddress() {
    if (!address) return
    await navigator.clipboard.writeText(address)
    setCopied(true)
    window.setTimeout(() => setCopied(false), 1_500)
  }

  if (!address || !isConnected || !isAuthenticated) {
    return <ConnectProfile onConnect={connect} isConnected={isConnected} />
  }

  return (
    <div className="profile-page">
      <header className="profile-hero">
        <div className="profile-identity">
          <div className="profile-avatar profile-avatar--large">{address.slice(2, 4).toUpperCase()}</div>
          <div>
            <span className="profile-eyebrow">Vela trader</span>
            <div className="profile-name-row">
              <h1>{address.slice(0, 6)}…{address.slice(-4)}</h1>
              <button type="button" onClick={copyAddress}>{copied ? 'Copied' : 'Copy'}</button>
            </div>
            <div className="profile-badges">
              <span>Sepolia beta</span>
              <span>{traderRank ? `Trader rank #${traderRank}` : 'Unranked'}</span>
              <span>{data.fills.length} fills</span>
            </div>
          </div>
        </div>
        <div className="profile-hero__actions">
          <Link href="/deposit" className="profile-primary-button">Deposit</Link>
          <Link href="/withdraw" className="profile-secondary-button">Withdraw</Link>
        </div>
      </header>

      <nav className="profile-tabs" aria-label="Account sections">
        <span className="is-active">Overview</span>
        <Link href="/portfolio">PnL &amp; tax lots</Link>
        <Link href="/dashboard">Orders</Link>
        <Link href="/history">Full activity</Link>
      </nav>

      {error ? <div className="profile-error">{error}</div> : null}

      <section className="profile-stat-grid" aria-label="Account summary">
        <article>
          <span>Portfolio value</span>
          <strong>{formatMoney(accountValue)}</strong>
          <small>Binance Spot USDC reference marks</small>
        </article>
        <article>
          <span>Available cash</span>
          <strong>{formatMoney(cashBalance)}</strong>
          <small>USDC available to trade</small>
        </article>
        <article>
          <span>Total PnL</span>
          <strong className={totalPnl !== null && totalPnl < 0 ? 'is-negative' : totalPnl !== null && totalPnl > 0 ? 'is-positive' : ''}>
            {totalPnl === null ? '—' : formatMoney(totalPnl)}
          </strong>
          <small>Realized + unrealized</small>
        </article>
        <article>
          <span>Open orders</span>
          <strong>{openOrders.length}</strong>
          <small>{data.referral ? `${data.referral.referred_count} referred traders` : 'Live account state'}</small>
        </article>
      </section>

      <div className="profile-content-grid">
        <div className="profile-main-column">
          <section className="profile-panel">
            <div className="profile-panel__header">
              <div>
                <span className="profile-eyebrow">Holdings</span>
                <h2>Your positions</h2>
              </div>
              <Link href="/markets/BTC-USDC">Trade markets</Link>
            </div>
            {loading && data.balances.length === 0 ? (
              <div className="profile-loading"><Spinner /> Loading positions</div>
            ) : positions.length === 0 ? (
              <div className="profile-empty">
                <strong>No open positions</strong>
                <span>Fund your account or make your first testnet trade.</span>
                <Link href="/deposit">Deposit funds</Link>
              </div>
            ) : (
              <div className="profile-table-wrap">
                <table className="profile-table">
                  <thead><tr><th>Asset</th><th>Quantity</th><th>Available</th><th>Mark</th><th>Value</th></tr></thead>
                  <tbody>
                    {positions.map((position) => (
                      <tr key={position.asset}>
                        <td><TokenIcon ticker={position.asset} size="sm" /><strong>{position.asset}</strong></td>
                        <td>{formatQuantity(position.total)}</td>
                        <td>{formatQuantity(position.available)}</td>
                        <td>{position.price > 0 ? formatMoney(position.price, 6) : '—'}</td>
                        <td>{position.price > 0 ? formatMoney(position.value) : '—'}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </section>

          <section className="profile-panel">
            <div className="profile-panel__header">
              <div>
                <span className="profile-eyebrow">Timeline</span>
                <h2>Recent activity</h2>
              </div>
              <Link href="/history">View all</Link>
            </div>
            {activity.length === 0 ? (
              <div className="profile-empty"><strong>No activity yet</strong><span>Your trades and funding events will appear here.</span></div>
            ) : (
              <div className="profile-activity-list">
                {activity.map((item) => (
                  <div key={item.id} className="profile-activity-row">
                    <span className={`profile-activity-icon is-${item.tone}`}>{item.type.slice(0, 1)}</span>
                    <div><strong>{item.title}</strong><span>{item.detail}</span></div>
                    <div><span className="profile-status">{item.status}</span><time>{timeAgo(item.timestamp)}</time></div>
                  </div>
                ))}
              </div>
            )}
          </section>
        </div>

        <aside className="profile-side-column">
          <section className="profile-panel profile-panel--compact">
            <span className="profile-eyebrow">Account</span>
            <h2>Balances</h2>
            <div className="profile-balance-list">
              {data.balances.length === 0 ? <span className="profile-muted">No balances yet.</span> : data.balances
                .filter((balance) => toNumber(balance.total) > 0)
                .sort((left, right) => toNumber(right.total) * marketPrice(right.asset, data.markets) - toNumber(left.total) * marketPrice(left.asset, data.markets))
                .slice(0, 6)
                .map((balance) => (
                  <div key={balance.asset}>
                    <span>{balance.asset}</span>
                    <strong>{formatQuantity(balance.total)}</strong>
                  </div>
                ))}
            </div>
            <div className="profile-funding-actions">
              <Link href="/deposit">Deposit</Link>
              <Link href="/withdraw">Withdraw</Link>
            </div>
          </section>

          <section className="profile-panel profile-panel--compact">
            <span className="profile-eyebrow">Performance</span>
            <h2>PnL breakdown</h2>
            <div className="profile-metric-list">
              <div><span>Realized</span><strong>{formatMicroUsdc(data.portfolio?.realized_pnl_usdc)}</strong></div>
              <div><span>Unrealized</span><strong>{formatMicroUsdc(data.portfolio?.unrealized_pnl_usdc)}</strong></div>
              <div><span>Cost basis</span><strong>{data.portfolio?.cost_basis_method ?? 'FIFO'}</strong></div>
            </div>
            <Link href="/portfolio" className="profile-panel__link">Open detailed portfolio</Link>
          </section>

          <section className="profile-panel profile-panel--compact profile-disclosure">
            <span className="profile-eyebrow">Beta disclosure</span>
            <h2>Testnet only</h2>
            <p>Vela is an engineering preview. Balances shown here are testnet or demo balances and must not be treated as real funds.</p>
            <Link href="/transparency">Read system status</Link>
          </section>
        </aside>
      </div>
    </div>
  )
}
