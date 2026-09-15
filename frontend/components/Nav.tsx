'use client'

import Link from 'next/link'
import { usePathname, useRouter } from 'next/navigation'
import { type FormEvent, useEffect, useRef, useState } from 'react'
import { useAuth } from '@/lib/auth'
import { Spinner } from '@/components/ui/Spinner'
import { ORDERED_PAIRS } from '@/lib/markets'

const PRIMARY_LINKS = [
  { href: '/markets', label: 'Trade' },
  { href: '/portfolio', label: 'Portfolio' },
  { href: '/dashboard', label: 'Orders' },
  { href: '/history', label: 'History' },
]

const SECONDARY_LINKS = [
  { href: '/analytics', label: 'Analytics' },
  { href: '/transparency', label: 'Transparency' },
  { href: '/leaderboard', label: 'Leaderboard' },
  { href: '/benchmarks', label: 'Benchmarks' },
  { href: '/decisions', label: 'Decisions' },
]

function SearchIcon() {
  return (
    <svg aria-hidden="true" width="16" height="16" viewBox="0 0 16 16" fill="none">
      <circle cx="7" cy="7" r="4.25" stroke="currentColor" strokeWidth="1.5" />
      <path d="m10.25 10.25 3 3" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
    </svg>
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
  const links = [...PRIMARY_LINKS, ...SECONDARY_LINKS]

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
        <Link href="/" onClick={onClose} className="vela-wordmark">Vela</Link>
        <button type="button" onClick={onClose} className="icon-button" aria-label="Close menu">×</button>
      </div>

      <nav className="mobile-nav__links">
        {links.map(({ href, label }) => (
          <Link key={href} href={href} onClick={onClose} className={pathname.startsWith(href) ? 'is-active' : ''}>
            {label}
          </Link>
        ))}
        <a href="https://monolithsystematicllc.mintlify.app" target="_blank" rel="noopener noreferrer">Docs ↗</a>
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
            {connecting && <Spinner size="xs" className="text-ink" />}
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
  const { address, isConnected, connect, signOut } = useAuth()
  const [connecting, setConnecting] = useState(false)
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false)
  const [marketSearch, setMarketSearch] = useState('')
  const marketSearchRef = useRef<HTMLInputElement>(null)
  const isTrading = /^\/markets\/[^/]+/.test(pathname)

  useEffect(() => {
    if (!mobileMenuOpen) return
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setMobileMenuOpen(false)
    }
    window.addEventListener('keydown', closeOnEscape)
    return () => window.removeEventListener('keydown', closeOnEscape)
  }, [mobileMenuOpen])

  useEffect(() => {
    if (!isTrading) return
    const focusSearch = (event: KeyboardEvent) => {
      if (event.key === '/' && document.activeElement?.tagName !== 'INPUT') {
        event.preventDefault()
        marketSearchRef.current?.focus()
      }
    }
    window.addEventListener('keydown', focusSearch)
    return () => window.removeEventListener('keydown', focusSearch)
  }, [isTrading])

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
      <header className={`app-nav${isTrading ? ' app-nav--terminal' : ''}`}>
        <div className="app-nav__inner">
          <Link href="/" className="vela-wordmark" aria-label="Vela home">Vela</Link>

          <nav className="app-nav__primary" aria-label="Primary navigation">
            {PRIMARY_LINKS.map(({ href, label }) => (
              <Link key={href} href={href} className={pathname.startsWith(href) ? 'is-active' : ''}>
                {label}
              </Link>
            ))}
          </nav>

          {isTrading && (
            <form className="market-search" role="search" onSubmit={handleMarketSearch}>
              <SearchIcon />
              <input ref={marketSearchRef} value={marketSearch} onChange={(event) => setMarketSearch(event.target.value)} placeholder="Search markets" aria-label="Search markets" />
              <kbd>/</kbd>
            </form>
          )}

          <div className="app-nav__actions">
            <div className="app-nav__secondary">
              <Link href="/transparency">Transparency</Link>
              <a href="https://monolithsystematicllc.mintlify.app" target="_blank" rel="noopener noreferrer">Docs</a>
            </div>

            {isConnected && address ? (
              <>
                <Link href="/deposit" className="deposit-link">Deposit</Link>
                <button type="button" onClick={signOut} className="wallet-button">
                  {address.slice(0, 6)}…{address.slice(-4)}
                </button>
              </>
            ) : (
              <button type="button" onClick={handleConnect} disabled={connecting} className="wallet-button wallet-button--primary">
                {connecting && <Spinner size="xs" className="text-ink" />}
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

      {mobileMenuOpen && (
        <MobileMenu onClose={() => setMobileMenuOpen(false)} address={address} isConnected={isConnected} connect={connect} signOut={signOut} />
      )}
    </>
  )
}
