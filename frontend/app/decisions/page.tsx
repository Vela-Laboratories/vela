'use client'

import { useState, useEffect, useCallback } from 'react'
import HexCanvas from '@/components/HexCanvas'

const PF = "var(--font-inter-sans), 'Inter', sans-serif"
const IN = 'Inter, sans-serif'
const CN = "'Courier New', monospace"

const API_URL = process.env.NEXT_PUBLIC_API_URL ?? 'https://vela-engine.fly.dev'

interface Decision {
  id: number
  decision_type: string
  title: string
  description: string
  rationale: string
  effective_date: number
  announced_at: number
  status: string
  operator_signature: string
}

type FilterStatus = 'ALL' | 'PENDING' | 'ENACTED' | 'CANCELLED'

function formatDate(tsMs: number): string {
  const d = new Date(tsMs)
  const months = ['Jan','Feb','Mar','Apr','May','Jun','Jul','Aug','Sep','Oct','Nov','Dec']
  return `${months[d.getMonth()]} ${d.getDate()}, ${d.getFullYear()}`
}

function typeLabel(t: string): string {
  return t.replace(/_/g, ' ')
}

export default function DecisionsPage() {
  const [decisions, setDecisions] = useState<Decision[] | null>(null)
  const [filter, setFilter] = useState<FilterStatus>('ALL')
  const [expandedRationale, setExpandedRationale] = useState<Set<number>>(new Set())

  const fetchDecisions = useCallback(async () => {
    try {
      const res = await fetch(`${API_URL}/decisions`)
      const json = await res.json()
      if (json.ok) setDecisions(json.data.decisions)
    } catch {
      // silently ignore
    }
  }, [])

  useEffect(() => {
    fetchDecisions()
  }, [fetchDecisions])

  const filtered = decisions === null
    ? null
    : filter === 'ALL'
    ? decisions
    : decisions.filter(d => d.status === filter)

  function toggleRationale(id: number) {
    setExpandedRationale(prev => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }

  const filters: FilterStatus[] = ['ALL', 'PENDING', 'ENACTED', 'CANCELLED']

  return (
    <div style={{ background: '#15131f', minHeight: '100vh' }}>
      <div style={{ position: 'relative', background: '#15131f', overflow: 'hidden' }} className="px-6 pt-12 pb-12 lg:px-[52px] lg:pt-[60px] lg:pb-[48px]">
        <HexCanvas />
        <div style={{ position: 'relative', zIndex: 1 }}>
          <p style={{ fontFamily: IN, fontSize: '10px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', marginBottom: '16px' }}>
            Dippi — Governance
          </p>
          <div>
            <span style={{ fontFamily: PF, fontWeight: 700, fontSize: '52px', color: '#f8f5ee', display: 'block', lineHeight: 1.1 }}>
              Decision log.
            </span>
            <span style={{ fontFamily: PF, fontWeight: 400, fontStyle: 'italic', fontSize: '52px', color: 'rgba(232,234,240,0.3)', display: 'block', lineHeight: 1.1 }}>
              Every change. Announced in advance.
            </span>
          </div>
          <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '14px', lineHeight: 1.75, color: 'rgba(232,234,240,0.4)', maxWidth: '540px', marginTop: '20px' }}>
            Material decisions about Dippi are published here before they take effect, signed by the operator wallet. Decisions affecting fees or markets are announced at least 14 days in advance.
          </p>
        </div>
      </div>

      <div style={{ position: 'relative', background: '#15131f', overflow: 'hidden' }} className="px-6 pb-10 lg:px-[52px]">
        <div style={{ position: 'relative', zIndex: 1 }}>
          <div style={{ padding: '20px 28px', background: 'rgba(232,234,240,0.04)', border: '1px solid rgba(232,234,240,0.08)', borderLeft: '3px solid #20c98a', display: 'flex', alignItems: 'center', gap: '16px' }}>
            <span style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.15em', color: '#20c98a', whiteSpace: 'nowrap' }}>● ACTIVE POLICY</span>
            <span style={{ fontFamily: IN, fontSize: '12px', color: 'rgba(232,234,240,0.5)', lineHeight: 1.7 }}>
              Vela commits to 14-day advance notice for all fee changes and market delistings. This policy is itself a signed decision and cannot be revoked without 14 days notice.
            </span>
          </div>
        </div>
      </div>

      <div style={{ background: '#13111d' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <p style={{ fontFamily: IN, fontSize: '10px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', marginBottom: '16px' }}>
          Decision History
        </p>
        <h2 style={{ fontFamily: PF, fontWeight: 700, fontSize: '32px', color: '#f8f5ee', margin: '0 0 28px' }}>
          The permanent record.
        </h2>

        <div style={{ display: 'flex', gap: '2px', marginBottom: '28px' }}>
          {filters.map(f => (
            <button
              key={f}
              onClick={() => setFilter(f)}
              style={{
                fontFamily: IN,
                fontSize: '10px',
                textTransform: 'uppercase',
                letterSpacing: '0.12em',
                padding: '7px 14px',
                border: filter === f ? '1px solid rgba(232,234,240,0.4)' : '1px solid rgba(232,234,240,0.12)',
                background: filter === f ? 'rgba(232,234,240,0.08)' : 'transparent',
                color: filter === f ? '#b8b1ff' : 'rgba(232,234,240,0.4)',
                cursor: 'pointer',
                borderRadius: 6,
              }}
            >
              {f}
            </button>
          ))}
        </div>

        {filtered === null ? (
          <div style={{ padding: '40px 0', textAlign: 'center' }}>
            <span style={{ fontFamily: IN, fontSize: '12px', color: 'rgba(232,234,240,0.3)' }}>Loading…</span>
          </div>
        ) : filtered.length === 0 ? (
          <div style={{ padding: '40px 0', textAlign: 'center' }}>
            <span style={{ fontFamily: IN, fontSize: '12px', color: 'rgba(232,234,240,0.4)' }}>
              {decisions?.length === 0
                ? 'No decisions recorded yet. The first decision will be published when any material change is made to Vela.'
                : `No ${filter.toLowerCase()} decisions.`}
            </span>
          </div>
        ) : (
          <div style={{ display: 'flex', flexDirection: 'column', gap: '1px', background: 'rgba(232,234,240,0.05)' }}>
            {filtered.map(d => {
              const isExpanded = expandedRationale.has(d.id)
              const statusStyle: Record<string, React.CSSProperties> = {
                PENDING: { color: '#20c98a', border: '1px solid rgba(107,138,90,0.3)', background: 'transparent' },
                ENACTED: { color: 'rgba(232,234,240,0.4)', border: '1px solid rgba(232,234,240,0.1)', background: 'transparent' },
                CANCELLED: { color: '#ef6a5b', border: '1px solid rgba(204,51,51,0.3)', background: 'transparent' },
              }
              return (
                <div key={d.id} style={{ background: '#211e2b', padding: '28px 32px' }}>
                  <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', gap: '16px' }}>
                    <div>
                      <span style={{ fontFamily: IN, fontSize: '8px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.5)', border: '1px solid rgba(232,234,240,0.15)', padding: '3px 8px', display: 'inline-block' }}>
                        {typeLabel(d.decision_type)}
                      </span>
                      <h3 style={{ fontFamily: PF, fontWeight: 700, fontSize: '20px', color: '#f8f5ee', margin: '8px 0 0' }}>
                        {d.title}
                      </h3>
                    </div>
                    <span style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.1em', padding: '4px 10px', whiteSpace: 'nowrap', flexShrink: 0, ...statusStyle[d.status] }}>
                      {d.status === 'PENDING' ? '● PENDING' : d.status}
                    </span>
                  </div>

                  <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '13px', color: 'rgba(232,234,240,0.6)', lineHeight: 1.8, margin: '12px 0' }}>
                    {d.description}
                  </p>

                  <div>
                    <button
                      onClick={() => toggleRationale(d.id)}
                      style={{ fontFamily: IN, fontSize: '10px', color: 'rgba(232,234,240,0.4)', background: 'none', border: 'none', cursor: 'pointer', padding: 0 }}
                    >
                      {isExpanded ? 'Rationale ↑' : 'Rationale →'}
                    </button>
                    {isExpanded && (
                      <p style={{ fontFamily: IN, fontSize: '13px', color: 'rgba(232,234,240,0.5)', lineHeight: 1.8, margin: '8px 0 0' }}>
                        {d.rationale}
                      </p>
                    )}
                  </div>

                  <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-end', marginTop: '16px', paddingTop: '16px', borderTop: '1px solid rgba(232,234,240,0.06)', flexWrap: 'wrap', gap: '12px' }}>
                    <div style={{ display: 'flex', gap: '28px' }}>
                      <div>
                        <span style={{ fontFamily: IN, fontSize: '8px', textTransform: 'uppercase', letterSpacing: '0.12em', color: 'rgba(232,234,240,0.35)', display: 'block' }}>Announced</span>
                        <span style={{ fontFamily: CN, fontSize: '10px', color: 'rgba(232,234,240,0.5)' }}>{formatDate(d.announced_at)}</span>
                      </div>
                      <div>
                        <span style={{ fontFamily: IN, fontSize: '8px', textTransform: 'uppercase', letterSpacing: '0.12em', color: 'rgba(232,234,240,0.35)', display: 'block' }}>Effective</span>
                        <span style={{ fontFamily: CN, fontSize: '10px', color: 'rgba(232,234,240,0.5)' }}>{formatDate(d.effective_date)}</span>
                      </div>
                    </div>
                    <div style={{ textAlign: 'right' }}>
                      <span style={{ fontFamily: IN, fontSize: '8px', textTransform: 'uppercase', letterSpacing: '0.12em', color: 'rgba(232,234,240,0.35)', display: 'block' }}>Operator Signature</span>
                      <span style={{ fontFamily: CN, fontSize: '9px', color: 'rgba(232,234,240,0.4)' }}>
                        {d.operator_signature.slice(0, 18)}…
                      </span>
                    </div>
                  </div>
                </div>
              )
            })}
          </div>
        )}
      </div>

      <div style={{ background: '#15131f', borderTop: '1px solid rgba(232,234,240,0.06)' }} className="px-6 py-6 lg:px-[52px]">
        <div style={{ display: 'flex', gap: '24px', flexWrap: 'wrap' }}>
          {[
            { href: '/', label: 'Home' },
            { href: '/status', label: 'Status' },
            { href: '/transparency', label: 'Transparency' },
            { href: '/decisions', label: 'Decisions' },
            { href: '/operator', label: 'Operator' },
          ].map(({ href, label }) => (
            <a
              key={href}
              href={href}
              style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.35)', textDecoration: 'none', textTransform: 'uppercase', letterSpacing: '0.1em' }}
              onMouseEnter={(e) => (e.currentTarget.style.color = 'rgba(232,234,240,0.7)')}
              onMouseLeave={(e) => (e.currentTarget.style.color = 'rgba(232,234,240,0.35)')}
            >
              {label}
            </a>
          ))}
        </div>
      </div>
    </div>
  )
}
