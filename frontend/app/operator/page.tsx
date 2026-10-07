'use client'

import { useState, useEffect, useCallback } from 'react'
import HexCanvas from '@/components/HexCanvas'

const PF = "var(--font-inter-sans), 'Inter', sans-serif"
const IN = 'Inter, sans-serif'
const CN = "'Courier New', monospace"

const OPERATOR_ADDRESS = '0x63c1C089e08EF6949f6Ee8dB1F3c2dC7f3e9B64EC0'
const API_URL = process.env.NEXT_PUBLIC_API_URL ?? 'https://vela-engine.fly.dev'

interface MarketMaker {
  address: string
  display_name: string | null
  registered_at: number
  is_internal: boolean
}

function formatDate(tsMs: number): string {
  const d = new Date(tsMs)
  const months = ['Jan','Feb','Mar','Apr','May','Jun','Jul','Aug','Sep','Oct','Nov','Dec']
  return `${months[d.getMonth()]} ${d.getDate()}, ${d.getFullYear()}`
}

const CAN_DO = [
  'Match orders submitted by users',
  'Set market parameters (tick size, lot size)',
  'Pause the exchange in emergencies',
  'Sign withdrawal authorizations',
  'Update the frontend',
  'Delist markets with 7-day notice',
]

const CANNOT_DO = [
  'Steal user funds (held in smart contract)',
  'Execute trades without valid user signatures',
  'Modify your order after submission',
  'Prevent emergency exit after 7-day timelock',
  'Forge a withdrawal without your wallet signing',
  'Alter historical trade records',
]

const COMMITMENTS = [
  {
    title: 'FAIR MATCHING',
    body: 'Orders are matched in strict price-time priority. We do not front-run orders, preference any user, or execute trades not submitted by users. The matching engine is open source and verifiable at github.com/arpjw/vela.',
  },
  {
    title: 'FULL RESERVES',
    body: 'Vela holds 100% of user funds in the VelaSettlement smart contract on Ethereum. We never lend, rehypothecate, or use user deposits for any purpose. Verifiable in real time at dippi.fun/transparency.',
  },
  {
    title: 'EMERGENCY EXIT',
    body: 'If Vela ever becomes unavailable, users can recover their funds directly from the smart contract after a 7-day timelock, without any operator involvement. The contract code is immutable and open source.',
  },
]

const SIGNED_STATEMENT = `I, the operator of Vela Exchange (Monolith Systematic LLC),
commit to: (1) matching all orders in strict price-time priority,
(2) maintaining 100% reserves of user funds in the VelaSettlement
contract, (3) never front-running or manipulating user orders,
(4) preserving user emergency exit rights.
Signed: April 2026. Operator: 0x63c1C089...3e9B64EC0`

const PLACEHOLDER_SIG = '0x' + '0'.repeat(130)

export default function OperatorPage() {
  const [mms, setMms] = useState<MarketMaker[] | null>(null)

  const fetchMMs = useCallback(async () => {
    try {
      const res = await fetch(`${API_URL}/market-makers`)
      const json = await res.json()
      if (json.ok) setMms(json.data.market_makers)
    } catch {
      // silently ignore
    }
  }, [])

  useEffect(() => {
    fetchMMs()
    const id = setInterval(fetchMMs, 5 * 60_000)
    return () => clearInterval(id)
  }, [fetchMMs])

  return (
    <div style={{ background: '#0C0C0C', minHeight: '100vh' }}>
      <div style={{ position: 'relative', background: '#0C0C0C', overflow: 'hidden' }} className="px-6 pt-12 pb-12 lg:px-[52px] lg:pt-[60px]">
        <HexCanvas />
        <div style={{ position: 'relative', zIndex: 1 }}>
          <p style={{ fontFamily: IN, fontSize: '10px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', marginBottom: '16px' }}>
            Vela Exchange — Operator Disclosure
          </p>
          <div>
            <span style={{ fontFamily: PF, fontWeight: 900, fontSize: '48px', color: '#e9eaf0', display: 'block', lineHeight: 1.1 }}>
              Who runs Vela.
            </span>
            <span style={{ fontFamily: PF, fontWeight: 400, fontStyle: 'italic', fontSize: '48px', color: 'rgba(232,234,240,0.3)', display: 'block', lineHeight: 1.1 }}>
              And what they can&apos;t do.
            </span>
          </div>
          <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '15px', lineHeight: 1.75, color: 'rgba(232,234,240,0.4)', maxWidth: '520px', marginTop: '20px' }}>
            Transparency starts with knowing who is operating this exchange and what powers they hold.
          </p>
        </div>
      </div>

      <div style={{ background: '#090b11' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <p style={{ fontFamily: IN, fontSize: '10px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', marginBottom: '20px' }}>
          The Operator
        </p>
        <h2 style={{ fontFamily: PF, fontWeight: 700, fontSize: '36px', color: '#e9eaf0', margin: '0 0 32px' }}>
          Monolith Systematic LLC
        </h2>

        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-[1px] mb-10" style={{ background: 'rgba(232,234,240,0.07)' }}>
          {[
            ['ENTITY', 'Monolith Systematic LLC'],
            ['TYPE', 'Delaware Limited Liability Company'],
            ['FOUNDED', 'March 2026'],
            ['LOCATION', 'San Francisco, California'],
            ['FOUNDER', 'Arya Somu'],
            ['CONTACT', 'arya@monolithsystematic.com'],
            ['WEBSITE', 'monolithsystematic.com'],
            ['GITHUB', 'github.com/arpjw/vela'],
            ['WHITE PAPER', 'ssrn.com/abstract=6579199'],
          ].map(([label, value]) => (
            <div key={label} style={{ background: '#0d1018', padding: '20px 24px' }}>
              <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.3)', margin: '0 0 6px' }}>{label}</p>
              <p style={{ fontFamily: IN, fontSize: '13px', color: '#e9eaf0', margin: 0 }}>{value}</p>
            </div>
          ))}
        </div>

        <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.3)', marginBottom: '10px' }}>
          Operator Wallet Address
        </p>
        <div style={{ background: '#0C0C0C', padding: '16px 20px', marginBottom: '10px' }}>
          <p style={{ fontFamily: CN, fontSize: '12px', color: '#e9eaf0', margin: 0, wordBreak: 'break-all' }}>
            {OPERATOR_ADDRESS}
          </p>
        </div>
        <a
          href={`https://sepolia.etherscan.io/address/${OPERATOR_ADDRESS}`}
          target="_blank"
          rel="noopener noreferrer"
          style={{ fontFamily: IN, fontSize: '11px', color: '#e9eaf0', textDecoration: 'underline' }}
        >
          View on Sepolia Etherscan →
        </a>
        <p style={{ fontFamily: IN, fontSize: '12px', color: 'rgba(232,234,240,0.45)', marginTop: '10px', maxWidth: '480px' }}>
          This wallet holds the operator signing key for withdrawal authorization and batch signing.
        </p>
      </div>

      <div style={{ position: 'relative', background: '#0C0C0C', overflow: 'hidden' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <HexCanvas />
        <div style={{ position: 'relative', zIndex: 1 }}>
          <p style={{ fontFamily: IN, fontSize: '10px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', marginBottom: '20px' }}>
            Operator Powers
          </p>
          <h2 style={{ fontFamily: PF, fontWeight: 700, fontSize: '32px', color: '#e9eaf0', margin: '0 0 36px' }}>
            What we can do.
          </h2>

          <div className="grid grid-cols-1 sm:grid-cols-2 gap-[48px]">
            <div>
              <div style={{ borderLeft: '3px solid #20c98a', paddingLeft: '20px' }}>
                {CAN_DO.map((item) => (
                  <div key={item} style={{ display: 'flex', gap: '10px', marginBottom: '14px', alignItems: 'flex-start' }}>
                    <span style={{ color: '#20c98a', fontWeight: 700, flexShrink: 0 }}>✓</span>
                    <span style={{ fontFamily: IN, fontSize: '13px', color: '#e9eaf0', lineHeight: 1.5 }}>{item}</span>
                  </div>
                ))}
              </div>
            </div>

            <div>
              <div style={{ borderLeft: '3px solid #ef6a5b', paddingLeft: '20px' }}>
                {CANNOT_DO.map((item) => (
                  <div key={item} style={{ display: 'flex', gap: '10px', marginBottom: '14px', alignItems: 'flex-start' }}>
                    <span style={{ color: '#ef6a5b', fontWeight: 700, flexShrink: 0 }}>✗</span>
                    <span style={{ fontFamily: IN, fontSize: '13px', color: 'rgba(232,234,240,0.8)', lineHeight: 1.5 }}>{item}</span>
                  </div>
                ))}
              </div>
            </div>
          </div>
        </div>
      </div>

      <div style={{ background: '#090b11' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <p style={{ fontFamily: IN, fontSize: '10px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', marginBottom: '20px' }}>
          Commitments
        </p>
        <h2 style={{ fontFamily: PF, fontWeight: 700, fontSize: '32px', color: '#e9eaf0', margin: '0 0 32px' }}>
          Our promise to users.
        </h2>

        <div className="grid grid-cols-1 lg:grid-cols-3 gap-[1px]" style={{ background: 'rgba(232,234,240,0.07)' }}>
          {COMMITMENTS.map((c) => (
            <div key={c.title} style={{ background: '#0d1018', padding: '28px 28px' }}>
              <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.3)', margin: '0 0 12px' }}>{c.title}</p>
              <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '13px', color: 'rgba(232,234,240,0.7)', lineHeight: 1.8, margin: 0 }}>{c.body}</p>
            </div>
          ))}
        </div>
      </div>

      <div style={{ background: '#090b11' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <p style={{ fontFamily: IN, fontSize: '10px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', marginBottom: '20px' }}>
          Conflicts of Interest
        </p>
        <h2 style={{ fontFamily: PF, fontWeight: 700, fontSize: '28px', color: '#e9eaf0', margin: '0 0 24px' }}>
          Disclosed in full.
        </h2>

        <div style={{ background: '#0d1018', padding: '28px 32px', marginBottom: '24px' }}>
          <p style={{ fontFamily: IN, fontWeight: 600, fontSize: '14px', color: '#e9eaf0', margin: '0 0 16px' }}>
            Monolith Systematic LLC operates both Vela Exchange and the Onyx Fund LP, a systematic global macro CTA fund.
          </p>
          <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '13px', lineHeight: 1.8, color: 'rgba(232,234,240,0.6)', margin: '0 0 16px' }}>
            Monolith Systematic does not trade spot crypto assets on Vela Exchange with proprietary capital. The Onyx Fund trades futures contracts on IBKR Pro and has no positions on Vela Exchange.
          </p>
          <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '13px', lineHeight: 1.8, color: 'rgba(232,234,240,0.6)', margin: '0 0 16px' }}>
            If this ever changes, Vela will publish a signed decision notice at least 14 days in advance. The decision will be signed by the operator wallet and permanently recorded.
          </p>
          <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '13px', lineHeight: 1.8, color: 'rgba(232,234,240,0.6)', margin: 0 }}>
            This disclosure is itself signed by the operator wallet. The signature below proves that the operator acknowledges this conflict and has made this commitment publicly.
          </p>
        </div>

        <div>
          <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.3)', marginBottom: '8px' }}>
            Operator Signature — Conflict of Interest Disclosure
          </p>
          <div style={{ background: '#0C0C0C', padding: '12px 16px', marginBottom: '8px' }}>
            <p style={{ fontFamily: CN, fontSize: '9px', color: 'rgba(232,234,240,0.25)', margin: 0, wordBreak: 'break-all' }}>
              {'0x' + 'pending'.padEnd(130, '0')}
            </p>
          </div>
          <p style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.35)', margin: 0 }}>
            Cryptographic signature of this disclosure will be published at mainnet launch.
          </p>
        </div>
      </div>

      <div style={{ position: 'relative', background: '#0C0C0C', overflow: 'hidden' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <HexCanvas />
        <div style={{ position: 'relative', zIndex: 1 }}>
          <p style={{ fontFamily: IN, fontSize: '10px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', marginBottom: '20px' }}>
            Active Market Makers
          </p>
          <h2 style={{ fontFamily: PF, fontWeight: 700, fontSize: '28px', color: '#e9eaf0', margin: '0 0 16px' }}>
            Who makes markets on Vela.
          </h2>
          <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '14px', lineHeight: 1.75, color: 'rgba(232,234,240,0.35)', maxWidth: '520px', marginBottom: '32px' }}>
            Market makers who choose to disclose their identity are listed here. Anonymous market makers are shown by wallet address only. All active market makers are visible regardless of disclosure status.
          </p>

          {mms === null ? (
            <div style={{ background: 'rgba(232,234,240,0.04)', border: '1px solid rgba(232,234,240,0.08)', padding: '24px', marginBottom: '16px' }}>
              <span style={{ fontFamily: IN, fontSize: '12px', color: 'rgba(232,234,240,0.3)' }}>Loading…</span>
            </div>
          ) : (
            <div style={{ display: 'flex', flexDirection: 'column', gap: '8px', marginBottom: '16px' }}>
              {mms.map(mm => (
                <div key={mm.address} style={{ padding: '20px 24px', background: 'rgba(232,234,240,0.04)', borderLeft: '2px solid rgba(232,234,240,0.07)' }}>
                  <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', marginBottom: '6px' }}>
                    <div>
                      <p style={{ fontFamily: IN, fontWeight: 600, fontSize: '13px', color: '#e9eaf0', margin: '0 0 2px' }}>
                        {mm.display_name ?? 'Anonymous Market Maker'}
                      </p>
                      {mm.is_internal && (
                        <p style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.35)', margin: '2px 0 0' }}>
                          Official MM Bot — CoinGecko-priced, 10 levels per market at 0.05% spread
                        </p>
                      )}
                    </div>
                    <span style={{ fontFamily: IN, fontSize: '8px', textTransform: 'uppercase', letterSpacing: '0.1em', color: '#20c98a', flexShrink: 0 }}>● ACTIVE</span>
                  </div>
                  <p style={{ fontFamily: CN, fontSize: '10px', color: 'rgba(232,234,240,0.35)', margin: '6px 0 4px' }}>
                    {mm.address.slice(0, 10)}…{mm.address.slice(-8)}
                  </p>
                  <p style={{ fontFamily: IN, fontSize: '10px', color: 'rgba(232,234,240,0.2)', margin: 0 }}>
                    Active since {formatDate(mm.registered_at)}
                  </p>
                </div>
              ))}
            </div>
          )}

          <p style={{ fontFamily: IN, fontSize: '10px', color: 'rgba(232,234,240,0.2)', margin: 0 }}>
            Want to be listed? Register your MM wallet at{' '}
            <a href="/market-makers/register" style={{ color: 'rgba(232,234,240,0.4)', textDecoration: 'underline' }}>
              dippi.fun/market-makers/register
            </a>
          </p>
        </div>
      </div>

      <div style={{ position: 'relative', background: '#0C0C0C', overflow: 'hidden' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <HexCanvas />
        <div style={{ position: 'relative', zIndex: 1 }}>
          <p style={{ fontFamily: IN, fontSize: '10px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', marginBottom: '20px' }}>
            Governance
          </p>
          <h2 style={{ fontFamily: PF, fontWeight: 700, fontSize: '28px', color: '#e9eaf0', margin: '0 0 16px' }}>
            Decision Log
          </h2>

          <div style={{ background: 'rgba(232,234,240,0.04)', border: '1px solid rgba(232,234,240,0.08)', padding: '24px', marginBottom: '16px' }}>
            <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '13px', color: 'rgba(232,234,240,0.55)', lineHeight: 1.8, margin: '0 0 16px' }}>
              Every material decision about Vela Exchange is published at /decisions before it takes effect, signed by the operator wallet. Fee changes require 14 days notice.
            </p>
            <a
              href="/decisions"
              style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.6)', textDecoration: 'none' }}
              onMouseEnter={(e) => (e.currentTarget.style.color = '#e9eaf0')}
              onMouseLeave={(e) => (e.currentTarget.style.color = 'rgba(232,234,240,0.6)')}
            >
              View decision log →
            </a>
          </div>
        </div>
      </div>

      <div style={{ position: 'relative', background: '#0C0C0C', overflow: 'hidden' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <HexCanvas />
        <div style={{ position: 'relative', zIndex: 1 }}>
          <p style={{ fontFamily: IN, fontSize: '10px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', marginBottom: '20px' }}>
            Operator Key Rotation
          </p>
          <h2 style={{ fontFamily: PF, fontWeight: 700, fontSize: '28px', color: '#e9eaf0', margin: '0 0 16px' }}>
            The key rotates. The record is permanent.
          </h2>
          <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '14px', lineHeight: 1.75, color: 'rgba(232,234,240,0.35)', maxWidth: '500px', marginBottom: '32px' }}>
            The operator wallet key is rotated every 90 days. Each rotation is announced 14 days in advance via a signed message from the current key. The complete rotation history is published below.
          </p>

          <div style={{ padding: '24px', background: 'rgba(232,234,240,0.04)', borderLeft: '2px solid rgba(232,234,240,0.07)', marginBottom: '32px' }}>
            <div style={{ display: 'flex', flexDirection: 'column', gap: '20px' }}>
              {[
                { label: 'ROTATION FREQUENCY', value: 'Every 90 days' },
                { label: 'ADVANCE NOTICE', value: '14 days minimum' },
                { label: 'HANDOFF PROOF', value: 'Signed by outgoing key' },
              ].map(({ label, value }) => (
                <div key={label} style={{ display: 'flex', flexDirection: 'column', gap: '4px' }}>
                  <span style={{ fontFamily: IN, fontSize: '8px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.3)' }}>{label}</span>
                  <span style={{ fontFamily: IN, fontWeight: 600, fontSize: '13px', color: '#e9eaf0' }}>{value}</span>
                </div>
              ))}
            </div>
          </div>

          <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.25)', marginBottom: '12px' }}>
            Key Rotation History
          </p>

          <div style={{ display: 'grid', gridTemplateColumns: '80px 1fr 1fr 100px', gap: '0 16px', padding: '0 0 8px', borderBottom: '1px solid rgba(232,234,240,0.08)' }}>
            {['EVENT', 'FROM ADDRESS', 'TO ADDRESS', 'DATE'].map((h) => (
              <span key={h} style={{ fontFamily: IN, fontSize: '8px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.2)' }}>{h}</span>
            ))}
          </div>

          <div style={{ display: 'grid', gridTemplateColumns: '80px 1fr 1fr 100px', gap: '0 16px', padding: '14px 0', borderBottom: '1px solid rgba(232,234,240,0.06)', alignItems: 'center' }}>
            <span style={{ fontFamily: IN, fontSize: '8px', textTransform: 'uppercase', letterSpacing: '0.1em', color: 'rgba(232,234,240,0.5)', border: '1px solid rgba(232,234,240,0.15)', padding: '2px 6px', display: 'inline-block' }}>
              GENESIS
            </span>
            <span style={{ fontFamily: IN, fontSize: '12px', color: 'rgba(232,234,240,0.3)' }}>—</span>
            <a
              href={`https://sepolia.etherscan.io/address/${OPERATOR_ADDRESS}`}
              target="_blank"
              rel="noopener noreferrer"
              style={{ fontFamily: CN, fontSize: '10px', color: 'rgba(232,234,240,0.5)', textDecoration: 'none', wordBreak: 'break-all' }}
            >
              {OPERATOR_ADDRESS.slice(0, 10)}…{OPERATOR_ADDRESS.slice(-8)}
            </a>
            <span style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.35)' }}>April 2026</span>
          </div>

          {/* TODO VEL-T1-02: fetch from GET /key-rotations */}

          <p style={{ fontFamily: IN, fontSize: '10px', color: 'rgba(232,234,240,0.2)', marginTop: '12px' }}>
            No rotations have occurred yet. First rotation scheduled for July 2026.
          </p>
        </div>
      </div>

      <div style={{ position: 'relative', background: '#0C0C0C', overflow: 'hidden' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <HexCanvas />
        <div style={{ position: 'relative', zIndex: 1 }}>
          <p style={{ fontFamily: IN, fontSize: '10px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', marginBottom: '20px' }}>
            Signed Commitment
          </p>
          <h2 style={{ fontFamily: PF, fontWeight: 700, fontSize: '32px', color: '#e9eaf0', margin: '0 0 16px' }}>
            Cryptographically committed.
          </h2>
          <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '14px', lineHeight: 1.75, color: 'rgba(232,234,240,0.35)', maxWidth: '560px', marginBottom: '32px' }}>
            The following statement has been signed by the operator wallet. This signature proves that the operator acknowledges these commitments and cannot later deny them.
          </p>

          <div style={{ background: 'rgba(232,234,240,0.04)', border: '1px solid rgba(232,234,240,0.08)', padding: '24px', marginBottom: '24px' }}>
            <pre style={{ fontFamily: CN, fontSize: '11px', color: '#e9eaf0', margin: 0, whiteSpace: 'pre-wrap', lineHeight: 1.7 }}>{SIGNED_STATEMENT}</pre>
          </div>

          <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.25)', marginBottom: '8px' }}>
            Operator Signature (placeholder — mainnet launch)
          </p>
          <div style={{ background: 'rgba(232,234,240,0.03)', border: '1px solid rgba(232,234,240,0.06)', padding: '12px 16px', marginBottom: '12px' }}>
            <p style={{ fontFamily: CN, fontSize: '9px', color: 'rgba(232,234,240,0.25)', margin: 0, wordBreak: 'break-all' }}>{PLACEHOLDER_SIG}</p>
          </div>
          <p style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.2)', margin: 0 }}>
            Cryptographic signature of this commitment will be published at mainnet launch.
          </p>
        </div>
      </div>
    </div>
  )
}
