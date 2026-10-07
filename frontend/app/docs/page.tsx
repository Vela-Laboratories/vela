import Link from 'next/link'

const SECTIONS = [
  {
    title: 'Introduction',
    description: 'Meet Dippi and learn about the Vela engine behind the testnet beta.',
    href: '/docs/introduction/what-is-vela',
  },
  {
    title: 'Getting Started',
    description: 'Connect a wallet and explore the Sepolia testnet workflow.',
    href: '/docs/getting-started/connect-wallet',
  },
  {
    title: 'Trading',
    description: 'Order types, time in force options, fees, and how to read the order book.',
    href: '/docs/trading/order-types',
  },
  {
    title: 'Market Making',
    description: 'The credit system, capital efficiency mechanics, and private L3 data feeds.',
    href: '/docs/market-making/credit-system',
  },
  {
    title: 'API Reference',
    description: 'HTTP and WebSocket API documentation for the Vela engine.',
    href: '/docs/api-reference/http-endpoints',
  },
  {
    title: 'Architecture',
    description: 'The matching engine, MPT state layer, committer, zkVM, and DA pipeline.',
    href: '/docs/architecture/matching-engine',
  },
  {
    title: 'Security',
    description: 'ECDSA authentication, replay protection, fraud proofs, and the trust model.',
    href: '/docs/security/ecdsa-authentication',
  },
  {
    title: 'FAQ',
    description: "Frequently asked questions about Dippi's public beta.",
    href: '/docs/faq',
  },
]

export default function DocsIndex() {
  return (
    <div>
      <h1
        style={{
          fontFamily: 'var(--font-inter)',
          fontWeight: 700,
          fontSize: '2rem',
          color: '#f8f5ee',
          marginBottom: '8px',
          lineHeight: 1.2,
        }}
      >
        Dippi documentation
      </h1>
      <p
        style={{
          fontFamily: 'var(--font-inter)',
          fontWeight: 400,
          fontSize: '1rem',
          lineHeight: 1.8,
          color: '#a2a9b7',
          marginBottom: '48px',
          maxWidth: '560px',
        }}
      >
        Dippi is a Sepolia testnet trading sandbox powered by the Vela matching engine.
        Explore the current beta, its limits, and the underlying research architecture.
        Do not use real funds or assume production custody or verification guarantees.
      </p>
      <div
        className="dippi-docs-grid"
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(2, 1fr)',
          gap: '16px',
        }}
      >
        {SECTIONS.map(({ title, description, href }) => (
          <Link
            key={href}
            href={href}
            style={{
              display: 'block',
              padding: '24px',
              backgroundColor: '#1a1825',
              border: '1px solid rgba(248,245,238,0.11)',
              borderRadius: '10px',
              textDecoration: 'none',
            }}
          >
            <div
              style={{
                fontSize: '0.875rem',
                fontWeight: 600,
                fontFamily: 'var(--font-inter)',
                color: '#f8f5ee',
                marginBottom: '8px',
              }}
            >
              {title}
            </div>
            <div
              style={{
                fontSize: '0.875rem',
                fontFamily: 'var(--font-inter)',
                color: '#7d8597',
                lineHeight: 1.6,
                marginBottom: '16px',
              }}
            >
              {description}
            </div>
            <div
              style={{
                fontSize: '0.8rem',
                fontFamily: 'var(--font-inter)',
                color: '#f8df86',
                fontWeight: 500,
              }}
            >
              Read more →
            </div>
          </Link>
        ))}
      </div>
    </div>
  )
}
