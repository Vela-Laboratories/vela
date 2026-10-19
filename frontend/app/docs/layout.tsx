'use client'

import Link from 'next/link'
import { usePathname } from 'next/navigation'
import { useState } from 'react'

interface NavItem {
  label: string
  href: string
}

interface NavSection {
  section: string
  items: NavItem[]
}

const SIDEBAR_NAV: NavSection[] = [
  {
    section: 'INTRODUCTION',
    items: [
      { label: 'What is Vela?', href: '/docs/introduction/what-is-vela' },
      { label: 'Architecture Overview', href: '/docs/introduction/architecture-overview' },
      { label: 'White Paper', href: '/docs/introduction/white-paper' },
    ],
  },
  {
    section: 'GETTING STARTED',
    items: [
      { label: 'Connect Your Wallet', href: '/docs/getting-started/connect-wallet' },
      { label: 'Deposit Funds', href: '/docs/getting-started/deposit-funds' },
      { label: 'Place Your First Order', href: '/docs/getting-started/place-first-order' },
      { label: 'Understanding Order Status', href: '/docs/getting-started/understanding-order-status' },
    ],
  },
  {
    section: 'TRADING',
    items: [
      { label: 'Order Types', href: '/docs/trading/order-types' },
      { label: 'Time in Force', href: '/docs/trading/time-in-force' },
      { label: 'Fees and Rebates', href: '/docs/trading/fees-and-rebates' },
      { label: 'Reading the Order Book', href: '/docs/trading/reading-order-book' },
      { label: 'Depth Chart', href: '/docs/trading/depth-chart' },
    ],
  },
  {
    section: 'MARKET MAKING',
    items: [
      { label: 'Credit System', href: '/docs/market-making/credit-system' },
      { label: 'Managing Credit Ratio', href: '/docs/market-making/managing-credit-ratio' },
      { label: 'Private L3 Feeds', href: '/docs/market-making/private-l3-feeds' },
      { label: 'MM Dashboard', href: '/docs/market-making/mm-dashboard' },
    ],
  },
  {
    section: 'API REFERENCE',
    items: [
      { label: 'Authentication', href: '/docs/api-reference/authentication' },
      { label: 'HTTP Endpoints', href: '/docs/api-reference/http-endpoints' },
      { label: 'WebSocket Protocol', href: '/docs/api-reference/websocket-protocol' },
      { label: 'Error Codes', href: '/docs/api-reference/error-codes' },
    ],
  },
  {
    section: 'ARCHITECTURE',
    items: [
      { label: 'Matching Engine', href: '/docs/architecture/matching-engine' },
      { label: 'MPT State Layer', href: '/docs/architecture/mpt-state-layer' },
      { label: 'Committer Thread', href: '/docs/architecture/committer-thread' },
      { label: 'zkVM and Fraud Proofs', href: '/docs/architecture/zk-vm' },
      { label: 'DA Layer', href: '/docs/architecture/da-layer' },
      { label: 'Forced Inclusion', href: '/docs/architecture/forced-inclusion' },
    ],
  },
  {
    section: 'SECURITY',
    items: [
      { label: 'ECDSA Authentication', href: '/docs/security/ecdsa-authentication' },
      { label: 'Replay Protection', href: '/docs/security/replay-protection' },
      { label: 'Fraud Proofs', href: '/docs/security/fraud-proofs' },
      { label: 'Trust Model', href: '/docs/security/trust-model' },
    ],
  },
  {
    section: 'FAQ',
    items: [{ label: 'FAQ', href: '/docs/faq' }],
  },
]

export default function DocsLayout({ children }: { children: React.ReactNode }) {
  const pathname = usePathname()
  const [mobileOpen, setMobileOpen] = useState(false)

  const sidebarContent = (
    <nav style={{ padding: '24px 0 48px' }}>
      {SIDEBAR_NAV.map(({ section, items }) => (
        <div key={section} style={{ marginBottom: '24px' }}>
          <div
            style={{
              fontSize: '0.625rem',
              fontWeight: 600,
              letterSpacing: '0.12em',
              color: '#666f82',
              textTransform: 'uppercase' as const,
              padding: '0 20px',
              marginBottom: '6px',
              fontFamily: 'var(--font-inter)',
            }}
          >
            {section}
          </div>
          {items.map(({ label, href }) => {
            const active = pathname === href
            return (
              <Link
                key={href}
                href={href}
                onClick={() => setMobileOpen(false)}
                style={{
                  display: 'block',
                  padding: '5px 20px 5px 18px',
                  fontSize: '0.875rem',
                  fontWeight: 400,
                  fontFamily: 'var(--font-inter)',
                  color: active ? '#f8f5ee' : '#aaa4b4',
                  backgroundColor: active ? 'rgba(248,223,134,0.12)' : 'transparent',
                  borderLeft: active ? '2px solid #f8df86' : '2px solid transparent',
                  textDecoration: 'none',
                  transition: 'color 0.15s, background-color 0.15s',
                  lineHeight: '1.6',
                }}
              >
                {label}
              </Link>
            )
          })}
        </div>
      ))}
    </nav>
  )

  return (
    <div className="dippi-docs" style={{ display: 'flex', minHeight: 'calc(100vh - 74px)', background: '#13111d' }}>
      <aside
        className="hidden md:block"
        style={{
          width: '260px',
          flexShrink: 0,
          backgroundColor: '#1a1825',
          borderRight: '1px solid rgba(248,245,238,0.11)',
          position: 'sticky',
          top: 0,
          height: 'calc(100vh - 74px)',
          overflowY: 'auto',
        }}
      >
        <div
          style={{
            padding: '16px 20px 12px',
            borderBottom: '1px solid rgba(248,245,238,0.11)',
          }}
        >
          <Link
            href="/docs"
            style={{
              fontSize: '0.65rem',
              fontWeight: 700,
              letterSpacing: '0.12em',
              color: '#f8df86',
              textTransform: 'uppercase' as const,
              textDecoration: 'none',
              fontFamily: 'var(--font-inter)',
            }}
          >
            Documentation
          </Link>
        </div>
        {sidebarContent}
      </aside>

      {mobileOpen && (
        <div
          style={{
            position: 'fixed',
            inset: 0,
            zIndex: 50,
            backgroundColor: 'rgba(0,0,0,0.4)',
          }}
          onClick={() => setMobileOpen(false)}
        >
          <aside
            style={{
              width: '280px',
              height: '100%',
              backgroundColor: '#1a1825',
              overflowY: 'auto',
              borderRight: '1px solid rgba(248,245,238,0.11)',
            }}
            onClick={(e) => e.stopPropagation()}
          >
            <div
              style={{
                padding: '16px 20px',
                borderBottom: '1px solid rgba(248,245,238,0.11)',
                display: 'flex',
                justifyContent: 'space-between',
                alignItems: 'center',
              }}
            >
              <span
                style={{
                  fontSize: '0.65rem',
                  fontWeight: 700,
                  letterSpacing: '0.12em',
                  color: '#f8df86',
                  textTransform: 'uppercase' as const,
                  fontFamily: 'var(--font-inter)',
                }}
              >
                Documentation
              </span>
              <button
                onClick={() => setMobileOpen(false)}
                style={{
                  background: 'none',
                  border: 'none',
                  cursor: 'pointer',
                  fontSize: '1rem',
                  color: '#7d8597',
                  lineHeight: 1,
                  padding: '4px',
                }}
              >
                ✕
              </button>
            </div>
            {sidebarContent}
          </aside>
        </div>
      )}

      <div style={{ flex: 1, minWidth: 0 }}>
        <div
          className="md:hidden"
          style={{
            padding: '12px 24px',
            borderBottom: '1px solid rgba(248,245,238,0.11)',
            backgroundColor: '#1a1825',
          }}
        >
          <button
            onClick={() => setMobileOpen(true)}
            style={{
              background: 'none',
              border: 'none',
              cursor: 'pointer',
              fontSize: '0.875rem',
              color: '#f8f5ee',
              fontFamily: 'var(--font-inter)',
              fontWeight: 500,
              display: 'flex',
              alignItems: 'center',
              gap: '8px',
            }}
          >
            <svg width="16" height="12" viewBox="0 0 16 12" fill="none">
              <path d="M0 0h16v2H0zm0 5h16v2H0zm0 5h16v2H0z" fill="#f8f5ee" />
            </svg>
            Documentation
          </button>
        </div>
        <div className="px-6 py-12 md:px-16" style={{ maxWidth: '760px' }}>
          <div className="dippi-docs-note">
            <strong>Public beta on Sepolia</strong>
            <span>These docs also cover Vela research and planned systems. Vela is a testnet demo with no real funds or production custody. It does not currently offer an authoritative production ledger, reconciled custody, real SP1 proofs, or enforceable forced inclusion.</span>
          </div>
          {children}
        </div>
      </div>
    </div>
  )
}
