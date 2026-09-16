'use client'

import { useEffect, useState } from 'react'
import Link from 'next/link'
import HexCanvas from '@/components/HexCanvas'
import { getLeaderboard, type LeaderboardData, type LeaderboardTrader, type LeaderboardReferrer } from '@/lib/api'

const PF = "var(--font-inter-sans), 'Inter', sans-serif"
const IN = "var(--font-inter-sans), 'Inter', sans-serif"
const MONO = "'Courier New', monospace"

function truncateAddr(addr: string): string {
  return `${addr.slice(0, 6)}…${addr.slice(-4)}`
}

function fmtVolume(v: string): string {
  const n = parseFloat(v)
  if (isNaN(n)) return '—'
  return n.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 })
}

function rankColor(rank: number): string {
  if (rank === 1) return '#8A7F6E'
  if (rank === 2) return '#9B9B9B'
  if (rank === 3) return '#7A5C38'
  return 'rgba(232,234,240,0.38)'
}

function RankBadge({ rank }: { rank: number }) {
  const color = rankColor(rank)
  const isTop3 = rank <= 3
  return (
    <span style={{
      fontFamily: MONO,
      fontSize: '11px',
      color,
      fontWeight: isTop3 ? 700 : 400,
    }}>
      #{rank}
    </span>
  )
}

function TradersTable({ traders }: { traders: LeaderboardTrader[] }) {
  if (traders.length === 0) {
    return (
      <p style={{ fontFamily: IN, fontSize: '13px', color: 'rgba(42,38,30,0.5)', textAlign: 'center', padding: '48px 0' }}>
        No trades yet. Be the first.
      </p>
    )
  }
  return (
    <div style={{ overflowX: 'auto' }}>
      <table style={{ width: '100%', borderCollapse: 'collapse' }}>
        <thead>
          <tr style={{ borderBottom: '1px solid rgba(42,38,30,0.12)' }}>
            {(['RANK', 'ADDRESS', 'VOLUME (USDC)', 'TRADES', 'MAKER/TAKER'] as const).map((col) => (
              <th key={col} style={{
                fontFamily: IN,
                fontSize: '10px',
                fontWeight: 500,
                color: 'rgba(42,38,30,0.45)',
                letterSpacing: '0.12em',
                textAlign: col === 'RANK' || col === 'ADDRESS' ? 'left' : 'right',
                padding: '10px 16px',
                textTransform: 'uppercase',
              }}>
                {col}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {traders.map((t, i) => {
            const total = t.maker_count + t.taker_count
            const ratio = total > 0
              ? `${((t.maker_count / total) * 100).toFixed(0)}% / ${((t.taker_count / total) * 100).toFixed(0)}%`
              : '— / —'
            return (
              <tr key={t.address} style={{ borderBottom: '1px solid rgba(42,38,30,0.08)' }}>
                <td style={{ padding: '12px 16px' }}><RankBadge rank={i + 1} /></td>
                <td style={{ padding: '12px 16px' }}>
                  <Link href="/transparency" style={{ fontFamily: MONO, fontSize: '11px', color: '#2A261E', textDecoration: 'none' }}>
                    {truncateAddr(t.address)}
                  </Link>
                </td>
                <td style={{ padding: '12px 16px', textAlign: 'right', fontFamily: MONO, fontSize: '11px', color: '#2A261E' }}>
                  {fmtVolume(t.volume_usdc)}
                </td>
                <td style={{ padding: '12px 16px', textAlign: 'right', fontFamily: MONO, fontSize: '11px', color: '#2A261E' }}>
                  {t.fill_count}
                </td>
                <td style={{ padding: '12px 16px', textAlign: 'right', fontFamily: MONO, fontSize: '11px', color: 'rgba(42,38,30,0.55)' }}>
                  {ratio}
                </td>
              </tr>
            )
          })}
        </tbody>
      </table>
    </div>
  )
}

function ReferrersTable({ referrers }: { referrers: LeaderboardReferrer[] }) {
  if (referrers.length === 0) {
    return (
      <p style={{ fontFamily: IN, fontSize: '13px', color: 'rgba(232,234,240,0.35)', textAlign: 'center', padding: '48px 0' }}>
        No referrals yet. Share your link from the dashboard.
      </p>
    )
  }
  return (
    <div style={{ overflowX: 'auto' }}>
      <table style={{ width: '100%', borderCollapse: 'collapse' }}>
        <thead>
          <tr style={{ borderBottom: '1px solid rgba(232,234,240,0.08)' }}>
            {(['RANK', 'ADDRESS', 'REFERRED USERS', 'EARNINGS (USDC)'] as const).map((col) => (
              <th key={col} style={{
                fontFamily: IN,
                fontSize: '10px',
                fontWeight: 500,
                color: 'rgba(232,234,240,0.35)',
                letterSpacing: '0.12em',
                textAlign: col === 'RANK' || col === 'ADDRESS' ? 'left' : 'right',
                padding: '10px 16px',
                textTransform: 'uppercase',
              }}>
                {col}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {referrers.map((r, i) => (
            <tr key={r.address} style={{ borderBottom: '1px solid rgba(232,234,240,0.05)' }}>
              <td style={{ padding: '12px 16px' }}><RankBadge rank={i + 1} /></td>
              <td style={{ padding: '12px 16px', fontFamily: MONO, fontSize: '11px', color: '#e9eaf0' }}>
                {truncateAddr(r.address)}
              </td>
              <td style={{ padding: '12px 16px', textAlign: 'right', fontFamily: MONO, fontSize: '11px', color: '#e9eaf0' }}>
                {r.referred_count}
              </td>
              <td style={{ padding: '12px 16px', textAlign: 'right', fontFamily: MONO, fontSize: '11px', color: '#e9eaf0' }}>
                {parseFloat(r.earnings_usdc).toFixed(6)}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

export default function LeaderboardPage() {
  const [data, setData] = useState<LeaderboardData | null>(null)

  async function load() {
    const res = await getLeaderboard()
    if (res.ok && res.data) setData(res.data)
  }

  useEffect(() => {
    load()
    const id = setInterval(load, 30_000)
    return () => clearInterval(id)
  }, [])

  return (
    <div style={{ background: '#0C0C0C', minHeight: '100vh' }}>
      <section style={{ position: 'relative', background: '#0C0C0C', overflow: 'hidden', padding: '80px 0 64px' }}>
        <HexCanvas />
        <div style={{ position: 'relative', zIndex: 2 }} className="max-w-7xl mx-auto px-6 lg:px-[52px]">
          <p style={{ fontFamily: IN, fontSize: '10px', letterSpacing: '0.18em', color: 'rgba(232,234,240,0.38)', textTransform: 'uppercase', marginBottom: '16px' }}>
            Updated in real time · All 16 markets
          </p>
          <h1 style={{ fontFamily: PF, fontStyle: 'italic', fontSize: '48px', color: '#e9eaf0', lineHeight: 1.1, marginBottom: '12px' }}>
            Leaderboard
          </h1>
          <p style={{ fontFamily: PF, fontStyle: 'italic', fontSize: '18px', color: 'rgba(232,234,240,0.5)' }}>
            Top traders. All-time.
          </p>
        </div>
      </section>

      <section style={{ background: '#090b11', padding: '64px 0' }}>
        <div className="max-w-7xl mx-auto px-6 lg:px-[52px]">
          <p style={{ fontFamily: IN, fontSize: '10px', letterSpacing: '0.18em', color: 'rgba(42,38,30,0.45)', textTransform: 'uppercase', marginBottom: '24px' }}>
            Top Traders by Volume
          </p>
          <div style={{ background: '#0d1018', border: '1px solid rgba(42,38,30,0.1)', borderRadius: 0 }}>
            <TradersTable traders={data?.top_traders ?? []} />
          </div>
        </div>
      </section>

      <section style={{ background: '#1A1916', padding: '64px 0' }}>
        <div className="max-w-7xl mx-auto px-6 lg:px-[52px]">
          <p style={{ fontFamily: IN, fontSize: '10px', letterSpacing: '0.18em', color: 'rgba(232,234,240,0.35)', textTransform: 'uppercase', marginBottom: '24px' }}>
            Top Referrers
          </p>
          <div style={{ border: '1px solid rgba(232,234,240,0.08)', borderRadius: 0 }}>
            <ReferrersTable referrers={data?.top_referrers ?? []} />
          </div>
        </div>
      </section>

      <footer style={{ background: '#0C0C0C', borderTop: '1px solid rgba(232,234,240,0.06)', padding: '48px 0' }}>
        <div className="max-w-7xl mx-auto px-6 lg:px-[52px] text-center">
          <p style={{ fontFamily: IN, fontSize: '13px', color: 'rgba(232,234,240,0.45)', lineHeight: 1.6, marginBottom: '16px' }}>
            Earn 20% of referred users&apos; taker fees for 90 days.{' '}
            Get your referral link from the dashboard.
          </p>
          <Link href="/dashboard" style={{ fontFamily: IN, fontSize: '12px', letterSpacing: '0.1em', textTransform: 'uppercase', color: '#e9eaf0', textDecoration: 'none', borderBottom: '1px solid rgba(232,234,240,0.3)', paddingBottom: '2px' }}>
            Go to Dashboard →
          </Link>
        </div>
      </footer>
    </div>
  )
}
