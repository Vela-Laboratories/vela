'use client'

import Link from 'next/link'
import { usePathname, useRouter } from 'next/navigation'
import { type FormEvent, useEffect, useMemo, useRef, useState } from 'react'
import { useAuth } from '@/lib/auth'
import { getBalances, type BalanceResponse } from '@/lib/api'
import { Spinner } from '@/components/ui/Spinner'
import { ORDERED_PAIRS } from '@/lib/markets'
import { fetchReferenceMarkets, type ReferenceMarket } from '@/lib/market-data'

const PRIMARY_LINKS = [
  { href: '/markets', label: 'Markets', match: '/markets' },
  { href: '/markets/BTC-USDC', label: 'Trade', match: '/markets/' },
  { href: '/profile', label: 'Portfolio', match: '/profile' },
  { href: '/history', label: 'Activity', match: '/history' },
]

const SECONDARY_LINKS = [
  { href: '/leaderboard', label: 'Leaderboard' },
  { href: '/analytics', label: 'Analytics' },
  { href: '/transparency', label: 'Transparency' },
  { href: '/portfolio', label: 'Tax lots' },
  { href: '/agents', label: 'Agent keys' },
  { href: '/benchmarks', label: 'Benchmarks' },
  { href: '/decisions', label: 'Decisions' },
]

function formatMoney(value: number): string {
  if (!Number.isFinite(value)) return '$0.00'
  if (Math.abs(value) >= 1_000_000) return `$${(value / 1_000_000).toFixed(2)}M`
  if (Math.abs(value) >= 10_000) return `$${(value / 1_000).toFixed(1)}K`
  return value.toLocaleString('en-US', {
    style: 'currency',
    currency: 'USD',
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  })
}

function accountValue(balances: BalanceResponse[], markets: ReferenceMarket[]): number {
  const prices = new Map<string, number>()
  for (const market of markets) {
    if (Number.isFinite(market.price)) prices.set(market.market.split('-')[0], market.price)
  }

  return balances.reduce((total, balance) => {
    const quantity = Number(balance.total)
    if (!Number.isFinite(quantity)) return total
    const asset = balance.asset.toUpperCase()
    if (asset === 'USDC') return total + quantity
    return total + quantity * (prices.get(asset) ?? 0)
  }, 0)
}

function SearchIcon() {
  return (
    <svg aria-hidden="true" width="16" height="16" viewBox="0 0 16 16" fill="none">
      <circle cx="7" cy="7" r="4.25" stroke="currentColor" strokeWidth="1.5" />
      <path d="m10.25 10.25 3 3" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
    </svg>
  )
}

function AccountControls({
  address,
  isAuthenticated,
  signOut,
}: {
  address: string
  isAuthenticated: boolean
  signOut: () => void
}) {
  const [balances, setBalances] = useState<BalanceResponse[]>([])
  const [markets, setMarkets] = useState<ReferenceMarket[]>([])
  const [menuOpen, setMenuOpen] = useState(false)
  const menuRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!isAuthenticated) return
    let cancelled = false
    const refresh = async () => {
      const [balanceResult, marketSnapshot] = await Promise.all([
        getBalances(address),
        fetchReferenceMarkets().catch(() => null),
      ])
      if (cancelled) return
      if (balanceResult.ok && balanceResult.data) setBalances(balanceResult.data)
      if (marketSnapshot) setMarkets(marketSnapshot.markets)
    }
    void refresh()
    const interval = window.setInterval(refresh, 5_000)
    return () => {
      cancelled = true
      window.clearInterval(interval)
    }
  }, [address, isAuthenticated])

  useEffect(() => {
    if (!menuOpen) return
    const close = (event: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(event.target as Node)) setMenuOpen(false)
    }
    document.addEventListener('mousedown', close)
    return () => document.removeEventListener('mousedown', close)
  }, [menuOpen])

  const cash = Number(balances.find((balance) => balance.asset.toUpperCase() === 'USDC')?.available ?? 0)
  const equity = useMemo(() => accountValue(balances, markets), [balances, markets])

  return (
    <div className="account-controls">
      <Link href="/deposit" className="account-tile">
        <strong>{isAuthenticated ? formatMoney(cash) : 'Locked'}</strong>
        <span>{isAuthenticated ? 'Cash · Deposit' : 'Authenticate'}</span>
      </Link>
      <Link href="/profile" className="account-tile account-tile--equity">
        <strong>{isAuthenticated ? formatMoney(equity) : '—'}</strong>
        <span>Portfolio</span>
      </Link>
      <div className="account-menu" ref={menuRef}>
        <button
          type="button"
          className="account-avatar"
          aria-label="Open account menu"
          aria-expanded={menuOpen}
          onClick={() => setMenuOpen((open) => !open)}
        >
          {address.slice(2, 4).toUpperCase()}
        </button>
        {menuOpen ? (
          <div className="account-menu__panel">
            <div className="account-menu__identity">
              <span>Connected wallet</span>
              <strong>{address.slice(0, 6)}…{address.slice(-4)}</strong>
            </div>
            <Link href="/profile" onClick={() => setMenuOpen(false)}>View profile</Link>
            <Link href="/agents" onClick={() => setMenuOpen(false)}>Agent keys</Link>
            <Link href="/deposit" onClick={() => setMenuOpen(false)}>Deposit funds</Link>
            <Link href="/withdraw" onClick={() => setMenuOpen(false)}>Withdraw funds</Link>
            <button type="button" onClick={signOut}>Sign out</button>
          </div>
        ) : null}
      </div>
    </div>
  )
}

function MobileMenu({
  onClose,
  address,
  isConnected,
  connect,
  signOut,
}: {
  onClose: () => void
  address: string | null
  isConnected: boolean
  connect: () => Promise<string>
  signOut: () => void
}) {
  const pathname = usePathname()
  const [connecting, setConnecting] = useState(false)
  const links = [{ href: '/', label: 'Home' }, ...PRIMARY_LINKS, ...SECONDARY_LINKS]

  async function handleConnect() {
    setConnecting(true)
    try {
      await connect()
      onClose()
    } catch {
      // Wallet errors are surfaced by the provider.
    } finally {
      setConnecting(false)
    }
  }

  return (
    <div className="mobile-nav" role="dialog" aria-modal="true" aria-label="Navigation">
      <div className="mobile-nav__header">
        <Link href="/" onClick={onClose} className="dippi-wordmark" aria-label="Vela home">Vela</Link>
        <button type="button" onClick={onClose} className="icon-button" aria-label="Close menu">×</button>
      </div>

      <nav className="mobile-nav__links">
        {links.map(({ href, label }) => (
          <Link key={href} href={href} onClick={onClose} className={pathname === href || (href !== '/' && pathname.startsWith(`${href}/`)) ? 'is-active' : ''}>
            {label}
          </Link>
        ))}
        <Link href="/docs" onClick={onClose}>Docs</Link>
      </nav>

      <div className="mobile-nav__actions">
        {isConnected && address ? (
          <>
            <div className="mobile-nav__funds">
              <Link href="/deposit" onClick={onClose}>Deposit</Link>
              <Link href="/withdraw" onClick={onClose}>Withdraw</Link>
            </div>
            <button type="button" className="wallet-button" onClick={() => { signOut(); onClose() }}>
              {address.slice(0, 6)}…{address.slice(-4)} · Sign out
            </button>
          </>
        ) : (
          <button type="button" className="wallet-button wallet-button--primary" onClick={handleConnect} disabled={connecting}>
            {connecting ? <Spinner size="xs" className="text-ink" /> : null}
            Connect wallet
          </button>
        )}
      </div>
    </div>
  )
}

export default function Nav() {
  const pathname = usePathname()
  const router = useRouter()
  const { address, isConnected, isAuthenticated, connect, signOut } = useAuth()
  const [connecting, setConnecting] = useState(false)
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false)
  const [marketSearch, setMarketSearch] = useState('')
  const marketSearchRef = useRef<HTMLInputElement>(null)
  const usesTerminalShell = pathname !== '/'

  useEffect(() => {
    if (!mobileMenuOpen) return
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setMobileMenuOpen(false)
    }
    window.addEventListener('keydown', closeOnEscape)
    return () => window.removeEventListener('keydown', closeOnEscape)
  }, [mobileMenuOpen])

  useEffect(() => {
    if (!usesTerminalShell) return
    const focusSearch = (event: KeyboardEvent) => {
      if (event.key === '/' && document.activeElement?.tagName !== 'INPUT') {
        event.preventDefault()
        marketSearchRef.current?.focus()
      }
    }
    window.addEventListener('keydown', focusSearch)
    return () => window.removeEventListener('keydown', focusSearch)
  }, [usesTerminalShell])

  async function handleConnect() {
    setConnecting(true)
    try {
      await connect()
    } catch {
      // Wallet errors are surfaced by the provider.
    } finally {
      setConnecting(false)
    }
  }

  function handleMarketSearch(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const query = marketSearch.trim().toUpperCase().replace(/\s+/g, '')
    if (!query) return
    const requestedPair = query.includes('-') ? query : `${query}-USDC`
    const match = ORDERED_PAIRS.find((pair) => pair === requestedPair || pair.startsWith(query))
    router.push(match ? `/markets/${match}` : '/markets')
    setMarketSearch('')
  }

  return (
    <>
      <header className={`app-nav${usesTerminalShell ? ' app-nav--terminal' : ''}`}>
        <div className="app-nav__inner">
          <Link href="/" className="dippi-wordmark" aria-label="Vela home">Vela</Link>

          {!usesTerminalShell ? <nav className="app-nav__primary" aria-label="Primary navigation">
            {PRIMARY_LINKS.map(({ href, label, match }) => (
              <Link key={href} href={href} className={pathname === href || (href === '/markets/BTC-USDC' && pathname.startsWith(match)) ? 'is-active' : ''}>
                {label}
              </Link>
            ))}
          </nav> : null}

          {usesTerminalShell ? <nav className="app-nav__quicklinks" aria-label="Main navigation">
            <Link href="/markets" className={pathname === '/markets' ? 'is-active' : ''}>Markets</Link>
            <Link href="/markets/BTC-USDC" className={/^\/markets\/[^/]+/.test(pathname) ? 'is-active' : ''}>Trade</Link>
            <Link href="/profile" className={pathname === '/profile' ? 'is-active' : ''}>Portfolio</Link>
          </nav> : null}

          {usesTerminalShell ? (
            <form className="market-search" role="search" onSubmit={handleMarketSearch}>
              <SearchIcon />
              <input ref={marketSearchRef} value={marketSearch} onChange={(event) => setMarketSearch(event.target.value)} placeholder="Search markets" aria-label="Search markets" />
              <kbd>/</kbd>
            </form>
          ) : null}

          <div className="app-nav__actions">
            {!usesTerminalShell ? <div className="app-nav__secondary">
              <Link href="/leaderboard">Leaderboard</Link>
              <Link href="/transparency">Transparency</Link>
            </div> : null}

            {isConnected && address ? (
              <AccountControls address={address} isAuthenticated={isAuthenticated} signOut={signOut} />
            ) : (
              <button type="button" onClick={handleConnect} disabled={connecting} className="wallet-button wallet-button--primary">
                {connecting ? <Spinner size="xs" className="text-ink" /> : null}
                Connect wallet
              </button>
            )}

            <button type="button" className="nav-menu-button" onClick={() => setMobileMenuOpen(true)} aria-label="Open menu">
              <span />
              <span />
            </button>
          </div>
        </div>
      </header>

      {mobileMenuOpen ? (
        <MobileMenu onClose={() => setMobileMenuOpen(false)} address={address} isConnected={isConnected} connect={connect} signOut={signOut} />
      ) : null}
    </>
  )
}
