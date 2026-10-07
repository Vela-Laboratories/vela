'use client'

import Link from 'next/link'
import { useCallback, useEffect, useMemo, useState, type MouseEvent } from 'react'
import { useAuth } from '@/lib/auth'
import {
  getAccountFills, getBalances, getDeposits, getLeaderboard, getOrders, getPortfolio, getReferral, getWithdrawals,
  type AccountFill, type BalanceResponse, type DepositStatus, type LeaderboardData, type Order,
  type PortfolioResponse, type ReferralData, type WithdrawalStatus,
} from '@/lib/api'
import { Spinner } from '@/components/ui/Spinner'
import TokenIcon from '@/components/TokenIcon'
import { fetchReferenceMarkets, type ReferenceMarket } from '@/lib/market-data'
import { getMarketInfo, getPriceDecimals, ORDERED_PAIRS } from '@/lib/markets'

interface ProfileData {
  balances: BalanceResponse[]
  orders: Order[]
  fills: AccountFill[]
  deposits: DepositStatus[]
  withdrawals: WithdrawalStatus[]
  portfolio: PortfolioResponse | null
  referral: ReferralData | null
  leaderboard: LeaderboardData | null
}

interface ActivityItem {
  id: string
  type: 'Trade' | 'Deposit' | 'Withdrawal' | 'Order'
  title: string
  detail: string
  status: string
  timestamp: number
  tone: 'buy' | 'sell' | 'neutral'
}

type ActivityTab = 'all' | 'fills' | 'orders'

const EMPTY_DATA: ProfileData = { balances: [], orders: [], fills: [], deposits: [], withdrawals: [], portfolio: null, referral: null, leaderboard: null }

function toNumber(value: string | number | undefined): number {
  const number = Number(value)
  return Number.isFinite(number) ? number : 0
}

function formatMoney(value: number, maximumFractionDigits = 2): string {
  return value.toLocaleString('en-US', { style: 'currency', currency: 'USD', minimumFractionDigits: 2, maximumFractionDigits })
}

function formatCompact(value: number): string {
  if (!Number.isFinite(value)) return '—'
  return new Intl.NumberFormat('en-US', { notation: 'compact', maximumFractionDigits: 2 }).format(value)
}

function formatQuantity(value: string): string {
  return toNumber(value).toLocaleString('en-US', { maximumFractionDigits: 6 })
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
  return markets.find((item) => item.market.split('-')[0] === asset.toUpperCase())?.price ?? 0
}

function ProfileIcon({ name, size = 15 }: { name: 'search' | 'star' | 'collapse' | 'copy' | 'wallet' | 'chart'; size?: number }) {
  const paths = {
    search: <><circle cx="11" cy="11" r="6" /><path d="m16 16 4 4" /></>,
    star: <path d="m12 3 2.7 5.5 6.1.9-4.4 4.3 1 6.1-5.4-2.9-5.4 2.9 1-6.1L3.2 9.4l6.1-.9Z" />,
    collapse: <path d="m14.5 5-7 7 7 7" />,
    copy: <><rect x="8" y="8" width="11" height="11" rx="2" /><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2" /></>,
    wallet: <><path d="M4 7h14a2 2 0 0 1 2 2v9H4a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h12" /><path d="M15 11h5v4h-5a2 2 0 0 1 0-4Z" /></>,
    chart: <><path d="M4 19V9m6 10V5m6 14v-7m4 7H2" /></>,
  }
  return <svg aria-hidden="true" width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">{paths[name]}</svg>
}

function ProfileMarketRail({ markets, onCollapse }: { markets: ReferenceMarket[]; onCollapse: () => void }) {
  const [query, setQuery] = useState('')
  const [filter, setFilter] = useState<'watchlist' | 'all' | 'movers' | 'volume'>('all')
  const [favorites, setFavorites] = useState(['BTC-USDC', 'ETH-USDC'])
  const byId = useMemo(() => Object.fromEntries(markets.map((market) => [market.market, market])), [markets])
  const pairs = useMemo(() => {
    const normalized = query.trim().toUpperCase()
    const visible = ORDERED_PAIRS.filter((pair) => (!normalized || pair.includes(normalized)) && (filter !== 'watchlist' || favorites.includes(pair)))
    if (filter === 'movers') return [...visible].sort((a, b) => Math.abs(byId[b]?.change24h ?? 0) - Math.abs(byId[a]?.change24h ?? 0))
    if (filter === 'volume') return [...visible].sort((a, b) => (byId[b]?.quoteVolume24h ?? 0) - (byId[a]?.quoteVolume24h ?? 0))
    return visible
  }, [byId, favorites, filter, query])

  function toggleFavorite(event: MouseEvent, pair: string) {
    event.preventDefault()
    event.stopPropagation()
    setFavorites((current) => current.includes(pair) ? current.filter((item) => item !== pair) : [...current, pair])
  }

  return <aside className="terminal-market-rail profile-market-rail">
    <nav className="terminal-rail-tabs terminal-rail-tabs--primary" aria-label="Exchange navigation"><Link href="/status"><i>●</i>Status</Link><Link href="/markets/BTC-USDC">Markets</Link><Link href="/leaderboard">Leaders</Link><span className="is-active">Profile</span><button type="button" aria-label="Collapse market rail" onClick={onCollapse}><ProfileIcon name="collapse" /></button></nav>
    <label className="terminal-rail-search"><ProfileIcon name="search" /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search assets" /></label>
    <div className="terminal-rail-filters">{(['watchlist', 'all', 'movers', 'volume'] as const).map((item) => <button key={item} type="button" className={filter === item ? 'is-active' : ''} onClick={() => setFilter(item)}>{item === 'all' ? 'All markets' : item}</button>)}</div>
    <div className="terminal-rail-insight"><span>◆</span><p><strong>Live reference data</strong><small>Binance pricing · Dippi testnet book</small></p><i>↗</i></div>
    <div className="terminal-market-list-head"><span>Markets</span><span>Price / 24h</span></div>
    <div className="terminal-market-list" role="list">{pairs.map((pair) => {
      const market = byId[pair]
      const info = getMarketInfo(pair)
      const change = market?.change24h
      const favorite = favorites.includes(pair)
      return <div key={pair} className="terminal-market-row"><button type="button" aria-label={`${favorite ? 'Remove' : 'Add'} ${info.ticker} ${favorite ? 'from' : 'to'} watchlist`} className={favorite ? 'is-favorite' : ''} onClick={(event) => toggleFavorite(event, pair)}><ProfileIcon name="star" size={13} /></button><Link href={`/markets/${pair}`} aria-label={`${info.ticker} ${info.quoteTicker}, ${market ? market.price.toFixed(getPriceDecimals(pair)) : 'price unavailable'}`}><TokenIcon ticker={info.ticker} size="md" /><span className="terminal-market-name"><strong>{info.ticker}<i>SPOT</i></strong><small>{market ? `${formatCompact(market.quoteVolume24h)} 24h vol` : `${info.quoteTicker} reference`}</small></span><span className="terminal-market-values"><strong>{market ? market.price.toFixed(getPriceDecimals(pair)) : '—'}</strong><small className={change !== undefined && change >= 0 ? 'positive' : 'negative'}>{change === undefined ? '—' : `${change >= 0 ? '+' : ''}${change.toFixed(2)}%`}</small></span></Link></div>
    })}{pairs.length === 0 ? <p className="terminal-rail-empty">No matching markets</p> : null}</div>
    <div className="terminal-rail-footer"><button type="button" onClick={onCollapse}><ProfileIcon name="collapse" size={13} />Collapse</button><Link href="/dashboard"><span>▤</span>Orders</Link></div>
  </aside>
}

function ProfileTicker({ markets, sourceLive }: { markets: ReferenceMarket[]; sourceLive: boolean | null }) {
  return <footer className="terminal-footer"><div>{markets.slice(0, 4).map((market) => <span key={market.market}><TokenIcon ticker={getMarketInfo(market.market).ticker} size="sm" /><strong>{market.price.toFixed(getPriceDecimals(market.market))}</strong><i className={market.change24h >= 0 ? 'positive' : 'negative'}>{market.change24h >= 0 ? '+' : ''}{market.change24h.toFixed(2)}%</i></span>)}</div><nav><span className={sourceLive === false ? 'is-offline' : ''}><i />{sourceLive === false ? 'Reference degraded' : 'Binance reference live'}</span><b>Sepolia testnet</b><Link href="/transparency">Transparency</Link><Link href="/docs">Help</Link></nav></footer>
}

function ConnectProfile({ onConnect, isConnected }: { onConnect: () => Promise<string>; isConnected: boolean }) {
  const [connecting, setConnecting] = useState(false)
  const [error, setError] = useState<string | null>(null)
  async function handleConnect() {
    setConnecting(true)
    setError(null)
    try { await onConnect() } catch (caught) { setError(caught instanceof Error ? caught.message : 'Wallet connection failed.') } finally { setConnecting(false) }
  }
  return <div className="profile-terminal-gate"><div className="profile-terminal-gate__mark"><ProfileIcon name="wallet" size={24} /></div><span>Private account workspace</span><h1>{isConnected ? 'Authenticate your wallet' : 'Connect your wallet'}</h1><p>Your testnet balances, orders, and activity are available after wallet authentication.</p>{error ? <div className="profile-error">{error}</div> : null}<button type="button" onClick={handleConnect} disabled={connecting}>{connecting ? <Spinner size="xs" /> : null}{isConnected ? 'Authenticate wallet' : 'Connect wallet'}</button><Link href="/markets">Continue browsing markets</Link></div>
}

function TraderRail({ leaderboard, currentAddress }: { leaderboard: LeaderboardData | null; currentAddress?: string }) {
  const traders = leaderboard?.top_traders.slice(0, 9) ?? []
  return <aside className="profile-trader-rail"><header><div><span>Testnet network</span><h2>Active traders</h2></div><Link href="/leaderboard">View all</Link></header><p>Ranked by reported Dippi testnet volume. No social following or performance endorsement is implied.</p><div className="profile-trader-list">{traders.length === 0 ? <div className="profile-trader-empty">Leaderboard activity will appear here as trades settle.</div> : traders.map((trader, index) => {
    const isCurrent = currentAddress?.toLowerCase() === trader.address.toLowerCase()
    return <div className={`profile-trader${isCurrent ? ' is-current' : ''}`} key={trader.address}><div className="profile-trader__avatar">{trader.address.slice(2, 4).toUpperCase()}</div><div><strong>{isCurrent ? 'You' : `${trader.address.slice(0, 6)}…${trader.address.slice(-4)}`}</strong><span>{trader.fill_count} fills · {formatCompact(toNumber(trader.volume_usdc))} USDC</span></div><b>#{index + 1}</b></div>
  })}</div><section><span>Account safety</span><strong>Wallet-authenticated data</strong><p>Private account requests require a signed, short-lived Vela session. This page never asks for a seed phrase.</p><Link href="/transparency">Review system boundaries</Link></section></aside>
}

export default function ProfilePage() {
  const { address, isConnected, isAuthenticated, connect } = useAuth()
  const [data, setData] = useState<ProfileData>(EMPTY_DATA)
  const [markets, setMarkets] = useState<ReferenceMarket[]>([])
  const [sourceLive, setSourceLive] = useState<boolean | null>(null)
  const [publicLeaderboard, setPublicLeaderboard] = useState<LeaderboardData | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [copied, setCopied] = useState(false)
  const [railCollapsed, setRailCollapsed] = useState(false)
  const [activityTab, setActivityTab] = useState<ActivityTab>('all')

  useEffect(() => {
    let cancelled = false
    const refresh = async () => {
      const [marketResult, leaderboardResult] = await Promise.all([fetchReferenceMarkets(), getLeaderboard()])
      if (cancelled) return
      setMarkets(marketResult.markets)
      setSourceLive(true)
      if (leaderboardResult.ok && leaderboardResult.data) setPublicLeaderboard(leaderboardResult.data)
    }
    void refresh().catch(() => { if (!cancelled) setSourceLive(false) })
    const timer = window.setInterval(() => void refresh().catch(() => { if (!cancelled) setSourceLive(false) }), 10_000)
    return () => { cancelled = true; window.clearInterval(timer) }
  }, [])

  const loadProfile = useCallback(async () => {
    if (!address || !isAuthenticated) return
    setLoading(true)
    setError(null)
    const [balances, orders, fills, deposits, withdrawals, portfolio, referral, leaderboard] = await Promise.all([getBalances(address), getOrders(address), getAccountFills(address), getDeposits(address), getWithdrawals(address), getPortfolio(address), getReferral(address), getLeaderboard()])
    const successfulResponses = [balances, orders, fills, deposits, withdrawals].filter((response) => response.ok).length
    setData({ balances: balances.ok ? balances.data ?? [] : [], orders: orders.ok ? orders.data ?? [] : [], fills: fills.ok ? fills.data ?? [] : [], deposits: deposits.ok ? deposits.data ?? [] : [], withdrawals: withdrawals.ok ? withdrawals.data ?? [] : [], portfolio: portfolio.ok ? portfolio.data ?? null : null, referral: referral.ok ? referral.data ?? null : null, leaderboard: leaderboard.ok ? leaderboard.data ?? null : null })
    if (successfulResponses === 0) setError('Account data is temporarily unavailable. Your funds and orders have not been changed.')
    setLoading(false)
  }, [address, isAuthenticated])

  useEffect(() => { const initial = window.setTimeout(() => void loadProfile(), 0); return () => window.clearTimeout(initial) }, [loadProfile])

  const openOrders = useMemo(() => data.orders.filter((order) => order.status === 'open' || order.status === 'partial'), [data.orders])
  const cashBalance = useMemo(() => toNumber(data.balances.find((balance) => balance.asset.toUpperCase() === 'USDC')?.available), [data.balances])
  const accountValue = useMemo(() => data.balances.reduce((total, balance) => total + toNumber(balance.total) * marketPrice(balance.asset, markets), 0), [data.balances, markets])
  const positions = useMemo(() => data.balances.filter((balance) => balance.asset.toUpperCase() !== 'USDC' && toNumber(balance.total) > 0).map((balance) => ({ ...balance, price: marketPrice(balance.asset, markets), value: toNumber(balance.total) * marketPrice(balance.asset, markets) })).sort((left, right) => right.value - left.value), [data.balances, markets])
  const allocations = useMemo(() => data.balances.map((balance) => ({ asset: balance.asset.toUpperCase(), value: toNumber(balance.total) * marketPrice(balance.asset, markets) })).filter((item) => item.value > 0).sort((left, right) => right.value - left.value), [data.balances, markets])
  const traderRank = useMemo(() => { if (!address || !data.leaderboard) return null; const index = data.leaderboard.top_traders.findIndex((trader) => trader.address.toLowerCase() === address.toLowerCase()); return index === -1 ? null : index + 1 }, [address, data.leaderboard])
  const totalPnl = useMemo(() => data.portfolio ? toNumber(data.portfolio.realized_pnl_usdc) + toNumber(data.portfolio.unrealized_pnl_usdc) : null, [data.portfolio])

  const activity = useMemo<ActivityItem[]>(() => {
    const fills: ActivityItem[] = data.fills.map((fill) => ({ id: `fill-${fill.id}`, type: 'Trade', title: `${fill.side === 'buy' ? 'Bought' : 'Sold'} ${fill.market.split('-')[0]}`, detail: `${formatQuantity(fill.quantity)} at ${formatMoney(toNumber(fill.price), 6)}`, status: fill.role, timestamp: normalizeTimestamp(fill.timestamp), tone: fill.side }))
    const deposits: ActivityItem[] = data.deposits.map((deposit) => ({ id: `deposit-${deposit.id}`, type: 'Deposit', title: `Deposited ${deposit.asset}`, detail: `${formatQuantity(deposit.amount)} ${deposit.asset}`, status: deposit.state, timestamp: new Date(deposit.updated_at).getTime(), tone: 'neutral' }))
    const withdrawals: ActivityItem[] = data.withdrawals.map((withdrawal) => ({ id: `withdrawal-${withdrawal.id}`, type: 'Withdrawal', title: `Withdrew ${withdrawal.asset}`, detail: `${formatQuantity(withdrawal.amount)} ${withdrawal.asset}`, status: withdrawal.state, timestamp: new Date(withdrawal.created_at).getTime(), tone: 'neutral' }))
    const orders: ActivityItem[] = data.orders.map((order) => ({ id: `order-${order.id}`, type: 'Order', title: `${order.side === 'buy' || order.side === 'bid' ? 'Buy' : 'Sell'} ${order.market.split('-')[0]} order`, detail: `${formatQuantity(order.quantity)} at ${formatMoney(toNumber(order.price), 6)}`, status: order.status, timestamp: normalizeTimestamp(order.timestamp), tone: order.side === 'buy' || order.side === 'bid' ? 'buy' : 'sell' }))
    const selected = activityTab === 'fills' ? fills : activityTab === 'orders' ? orders : [...fills, ...deposits, ...withdrawals]
    return selected.filter((item) => Number.isFinite(item.timestamp)).sort((left, right) => right.timestamp - left.timestamp).slice(0, 12)
  }, [activityTab, data.deposits, data.fills, data.orders, data.withdrawals])

  async function copyAddress() { if (!address) return; await navigator.clipboard.writeText(address); setCopied(true); window.setTimeout(() => setCopied(false), 1_500) }

  const authenticated = Boolean(address && isConnected && isAuthenticated)
  const leaderboard = data.leaderboard ?? publicLeaderboard

  return <div className="profile-terminal"><div className={`profile-terminal__grid${railCollapsed ? ' is-rail-collapsed' : ''}`}>
    {railCollapsed ? <button type="button" className="terminal-rail-restore" onClick={() => setRailCollapsed(false)} aria-label="Open market discovery"><ProfileIcon name="collapse" /></button> : <ProfileMarketRail markets={markets} onCollapse={() => setRailCollapsed(true)} />}
    <main className="profile-workspace">{!authenticated || !address ? <ConnectProfile onConnect={connect} isConnected={isConnected} /> : <>
      <section className="profile-cover" aria-label="Profile header"><div className="profile-cover__grid" /><span>VELA / ACCOUNT</span><small>Sepolia testnet</small></section>
      <header className="profile-terminal-identity"><div className="profile-terminal-avatar">{address.slice(2, 4).toUpperCase()}<i>✓</i></div><div className="profile-terminal-name"><span>Vela trader</span><h1>{address.slice(0, 6)}…{address.slice(-4)}</h1><div><b>Wallet authenticated</b><span>{data.fills.length} fills</span><span>{traderRank ? `Rank #${traderRank}` : 'Unranked'}</span></div></div><div className="profile-terminal-social"><div><strong>{openOrders.length}</strong><span>Open orders</span></div><div><strong>{data.referral?.referred_count ?? 0}</strong><span>Referrals</span></div></div><div className="profile-terminal-actions"><button type="button" onClick={copyAddress}><ProfileIcon name="copy" />{copied ? 'Copied' : 'Copy address'}</button><Link href="/withdraw">Withdraw</Link><Link href="/deposit" className="is-primary">Deposit</Link></div></header>
      {error ? <div className="profile-error profile-terminal-error">{error}</div> : null}
      <div className="profile-dashboard"><div className="profile-dashboard__summary"><section className="profile-equity-panel"><header><div><span>Portfolio value</span><strong>{formatMoney(accountValue)}</strong><small className={totalPnl !== null && totalPnl < 0 ? 'negative' : 'positive'}>{totalPnl === null ? 'PnL unavailable' : `${totalPnl >= 0 ? '+' : ''}${formatMoney(totalPnl)} total PnL`}</small></div><nav><span className="is-active">Snapshot</span><Link href="/portfolio">Tax lots</Link></nav></header><div className="profile-allocation" aria-label="Current portfolio allocation"><div className="profile-allocation__chart"><div className="profile-allocation__baseline" /><div className="profile-allocation__value"><ProfileIcon name="chart" /><span>Current ledger-backed allocation</span></div><div className="profile-allocation__bar">{allocations.map((item, index) => <i key={item.asset} className={`allocation-${index % 6}`} style={{ width: `${accountValue > 0 ? item.value / accountValue * 100 : 0}%` }} title={`${item.asset}: ${formatMoney(item.value)}`} />)}</div></div><div className="profile-allocation__legend">{allocations.length === 0 ? <span>No funded balances yet.</span> : allocations.slice(0, 6).map((item, index) => <div key={item.asset}><i className={`allocation-${index % 6}`} /><span>{item.asset}</span><strong>{formatMoney(item.value)}</strong><small>{accountValue > 0 ? `${(item.value / accountValue * 100).toFixed(1)}%` : '0%'}</small></div>)}</div></div><footer><div><span className="profile-cash-mark">$</span><p><small>Available cash</small><strong>{formatMoney(cashBalance)}</strong></p></div><div><Link href="/withdraw">Withdraw</Link><Link href="/deposit" className="is-primary">Deposit</Link></div></footer></section>
      <section className="profile-positions-panel"><header><div><span>Your positions</span><small>{positions.length} assets</small></div><Link href="/markets/BTC-USDC">Trade markets</Link></header>{loading && data.balances.length === 0 ? <div className="profile-panel-empty"><Spinner />Loading positions</div> : positions.length === 0 ? <div className="profile-panel-empty"><strong>No open positions</strong><span>Fund your account or make your first testnet trade.</span></div> : <div className="profile-positions-table"><div className="profile-positions-head"><span>Asset</span><span>Position</span><span>Mark</span><span>Value</span></div>{positions.map((position) => <Link href={`/markets/${position.asset}-USDC`} key={position.asset}><span><TokenIcon ticker={position.asset} size="sm" /><strong>{position.asset}</strong></span><span>{formatQuantity(position.total)}</span><span>{position.price > 0 ? formatMoney(position.price, 6) : '—'}</span><strong>{position.price > 0 ? formatMoney(position.value) : '—'}</strong></Link>)}</div>}</section></div>
      <section className="profile-activity-panel"><header><div role="tablist" aria-label="Account activity">{([['all', 'All activity'], ['fills', 'Fills'], ['orders', 'Orders']] as [ActivityTab, string][]).map(([id, label]) => <button key={id} type="button" role="tab" aria-selected={activityTab === id} onClick={() => setActivityTab(id)}>{label}</button>)}</div><Link href="/history">Full history</Link></header><div className="profile-activity-head"><span>Event</span><span>Details</span><span>Status</span><span>Time</span></div>{activity.length === 0 ? <div className="profile-panel-empty profile-panel-empty--tall"><strong>No account activity yet</strong><span>Trades, orders, deposits, and withdrawals will appear here.</span></div> : <div className="profile-terminal-activity">{activity.map((item) => <div key={item.id}><span className={`profile-event-mark is-${item.tone}`}>{item.type.slice(0, 1)}</span><span><strong>{item.title}</strong><small>{item.type}</small></span><span>{item.detail}</span><span>{item.status}</span><time>{timeAgo(item.timestamp)}</time></div>)}</div>}</section></div>
    </>}</main><TraderRail leaderboard={leaderboard} currentAddress={address ?? undefined} /></div><ProfileTicker markets={markets} sourceLive={sourceLive} /></div>
}
