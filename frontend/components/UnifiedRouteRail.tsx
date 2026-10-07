'use client'

import Link from 'next/link'
import { usePathname } from 'next/navigation'

const ROUTE_GROUPS = [
  {
    label: 'Discover',
    links: [
      { href: '/markets/BTC-USDC', label: 'Trade', glyph: '↗' },
      { href: '/markets', label: 'Markets', glyph: '◇' },
      { href: '/leaderboard', label: 'Leaderboard', glyph: '⌁' },
    ],
  },
  {
    label: 'Account',
    links: [
      { href: '/profile', label: 'Overview', glyph: '○' },
      { href: '/dashboard', label: 'Orders', glyph: '▤' },
      { href: '/history', label: 'Activity', glyph: '↻' },
      { href: '/portfolio', label: 'Tax lots', glyph: '◫' },
      { href: '/deposit', label: 'Deposit', glyph: '↓' },
      { href: '/withdraw', label: 'Withdraw', glyph: '↑' },
    ],
  },
  {
    label: 'Transparency',
    links: [
      { href: '/transparency', label: 'Overview', glyph: '◎' },
      { href: '/status', label: 'System status', glyph: '●' },
      { href: '/proofs', label: 'Proofs', glyph: '✓' },
      { href: '/batches', label: 'Batches', glyph: '▦' },
      { href: '/verify', label: 'Verify', glyph: '⌕' },
      { href: '/transparency-score', label: 'Scorecard', glyph: '◈' },
    ],
  },
  {
    label: 'Research',
    links: [
      { href: '/analytics', label: 'Analytics', glyph: '⌁' },
      { href: '/benchmarks', label: 'Benchmarks', glyph: '≋' },
      { href: '/decisions', label: 'Decisions', glyph: '◧' },
      { href: '/tee', label: 'TEE', glyph: '⬡' },
    ],
  },
  {
    label: 'Advanced',
    links: [
      { href: '/operator', label: 'Operator', glyph: '⚙' },
      { href: '/market-makers/register', label: 'Market makers', glyph: '±' },
      { href: '/docs', label: 'Documentation', glyph: '?' },
    ],
  },
]

function isActive(pathname: string, href: string): boolean {
  if (href === '/markets/BTC-USDC') return /^\/markets\/[^/]+/.test(pathname)
  return pathname === href || pathname.startsWith(`${href}/`)
}

export default function UnifiedRouteRail() {
  const pathname = usePathname()

  return <aside className="unified-route-rail">
    <div className="unified-route-rail__heading"><span>dippi.fun</span><strong>Your workspace</strong><small>Public beta · Sepolia</small></div>
    <div className="unified-route-rail__notice"><i>◆</i><div><strong>Testnet environment</strong><span>No real funds or production custody</span></div></div>
    <nav aria-label="Dippi workspace navigation">
      {ROUTE_GROUPS.map((group) => <section key={group.label}><h2>{group.label}</h2>{group.links.map((link) => <Link key={link.href} href={link.href} className={isActive(pathname, link.href) ? 'is-active' : ''} aria-current={isActive(pathname, link.href) ? 'page' : undefined}><i>{link.glyph}</i><span>{link.label}</span></Link>)}</section>)}
    </nav>
    <footer><span><i />Reference services</span><strong>Operational</strong></footer>
  </aside>
}
