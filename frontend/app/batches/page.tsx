'use client'

import { useState, useEffect, useCallback } from 'react'
import Link from 'next/link'
import HexCanvas from '@/components/HexCanvas'

const PF = "var(--font-inter-sans), 'Inter', sans-serif"
const IN = 'Inter, sans-serif'
const CN = "'Courier New', monospace"

const API_URL = process.env.NEXT_PUBLIC_API_URL ?? 'https://vela-engine.fly.dev'

interface Batch {
  batch_id: number
  timestamp: number
  fill_count: number
  order_count: number
  markets: string[]
  state_root: string
  operator_signature: string
  fills: string[]
}

interface AnchorRecord {
  anchor_id: number
  state_root: string
  tx_hash: string
  timestamp: number
  orders_processed: number
  block_number: number | null
  etherscan_url: string
}

function formatDateTime(ms: number): string {
  return new Date(ms).toLocaleString('en-US', {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  })
}

function padBatchId(id: number): string {
  return `#${String(id).padStart(3, '0')}`
}

function formatAnchorId(id: number): string {
  return `#${String(id).padStart(3, '0')}`
}

function formatNumber(n: number): string {
  return n.toLocaleString('en-US')
}

export default function BatchesPage() {
  const [batches, setBatches] = useState<Batch[]>([])
  const [loading, setLoading] = useState(true)
  const [anchors, setAnchors] = useState<AnchorRecord[]>([])

  const fetchBatches = useCallback(async () => {
    try {
      const res = await fetch(`${API_URL}/batches`)
      const data = await res.json()
      if (data.ok && Array.isArray(data.data)) {
        setBatches(data.data.slice().reverse())
      }
    } catch {
      // silently ignore
    } finally {
      setLoading(false)
    }
  }, [])

  const fetchAnchors = useCallback(async () => {
    try {
      const res = await fetch(`${API_URL}/anchors`)
      const data = await res.json()
      if (data.ok && data.data?.anchors) {
        setAnchors(data.data.anchors)
      }
    } catch {
      // silently ignore
    }
  }, [])

  useEffect(() => {
    fetchBatches()
    fetchAnchors()
    const t1 = setInterval(fetchBatches, 10_000)
    const t2 = setInterval(fetchAnchors, 60_000)
    return () => {
      clearInterval(t1)
      clearInterval(t2)
    }
  }, [fetchBatches, fetchAnchors])

  const totalFills = batches.reduce((s, b) => s + b.fill_count, 0)
  const avgFills = batches.length > 0 ? (totalFills / batches.length).toFixed(1) : '—'

  return (
    <div style={{ background: '#0C0C0C', minHeight: '100vh' }}>
      <div style={{ position: 'relative', background: '#0C0C0C', overflow: 'hidden' }} className="px-6 pt-12 pb-12 lg:px-[52px] lg:pt-[60px]">
        <HexCanvas />
        <div style={{ position: 'relative', zIndex: 1 }}>
          <p style={{ fontFamily: IN, fontSize: '10px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', marginBottom: '16px' }}>
            Vela Exchange — Batch Explorer
          </p>
          <div>
            <span style={{ fontFamily: PF, fontWeight: 900, fontSize: '48px', color: '#e9eaf0', display: 'block', lineHeight: 1.1 }}>
              Every batch.
            </span>
            <span style={{ fontFamily: PF, fontWeight: 400, fontStyle: 'italic', fontSize: '48px', color: 'rgba(232,234,240,0.3)', display: 'block', lineHeight: 1.1 }}>
              Every trade. Verifiable.
            </span>
          </div>
          <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '15px', lineHeight: 1.75, color: 'rgba(232,234,240,0.4)', maxWidth: '560px', marginTop: '20px' }}>
            Vela processes trades in batches. Each batch has a state root that can be used to verify the integrity of every trade. Browse the complete history below.
          </p>
        </div>
      </div>

      <div style={{ background: '#090b11' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <p style={{ fontFamily: IN, fontSize: '10px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', marginBottom: '24px' }}>
          Batch History
        </p>

        <div style={{ padding: '6px 0', borderBottom: '1px solid rgba(232,234,240,0.1)' }} className="grid grid-cols-[80px_80px_1fr] lg:grid-cols-[80px_1fr_80px_80px_140px_1fr]">
          {(['BATCH', 'FILLS', 'ACTION', 'TIMESTAMP', 'ORDERS', 'STATE ROOT'] as const).map((h, i) => (
            <span key={h} style={{ fontFamily: IN, fontSize: '8px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.3)' }} className={i >= 3 ? 'hidden lg:block' : ''}>{h}</span>
          ))}
        </div>

        {loading ? (
          <p style={{ fontFamily: IN, fontSize: '12px', color: 'rgba(232,234,240,0.3)', padding: '48px 0', textAlign: 'center' }}>
            Loading batches…
          </p>
        ) : batches.length === 0 ? (
          <p style={{ fontFamily: IN, fontSize: '12px', color: 'rgba(232,234,240,0.3)', padding: '48px 0', textAlign: 'center' }}>
            No batches yet. Trades will appear here as they execute.
          </p>
        ) : (
          batches.map((batch) => (
            <div
              key={batch.batch_id}
              style={{ padding: '12px 0', borderBottom: '1px solid rgba(232,234,240,0.05)', alignItems: 'center' }}
              className="grid grid-cols-[80px_80px_1fr] lg:grid-cols-[80px_1fr_80px_80px_140px_1fr]"
            >
              <span style={{ fontFamily: IN, fontWeight: 600, fontSize: '12px', color: '#e9eaf0' }}>{padBatchId(batch.batch_id)}</span>
              <span style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.45)' }} className="hidden lg:block">{formatDateTime(batch.timestamp)}</span>
              <span style={{ fontFamily: CN, fontSize: '12px', color: '#e9eaf0' }}>{batch.fill_count}</span>
              <span style={{ fontFamily: CN, fontSize: '12px', color: 'rgba(232,234,240,0.5)' }} className="hidden lg:block">{batch.order_count}</span>
              <span style={{ fontFamily: CN, fontSize: '10px', color: 'rgba(232,234,240,0.4)' }} className="hidden lg:block">{batch.state_root.slice(0, 10)}…</span>
              <Link
                href={`/batches/${batch.batch_id}`}
                style={{ fontFamily: IN, fontSize: '10px', color: '#e9eaf0', textDecoration: 'none' }}
                onMouseEnter={(e) => (e.currentTarget.style.textDecoration = 'underline')}
                onMouseLeave={(e) => (e.currentTarget.style.textDecoration = 'none')}
              >
                View →
              </Link>
            </div>
          ))
        )}

        <div style={{ display: 'flex', gap: '48px', marginTop: '32px', paddingTop: '24px', borderTop: '1px solid rgba(232,234,240,0.08)' }}>
          <div>
            <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.3)', margin: '0 0 4px' }}>Total Batches</p>
            <p style={{ fontFamily: CN, fontSize: '16px', color: '#e9eaf0', margin: 0 }}>{batches.length}</p>
          </div>
          <div>
            <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.3)', margin: '0 0 4px' }}>Total Fills</p>
            <p style={{ fontFamily: CN, fontSize: '16px', color: '#e9eaf0', margin: 0 }}>{totalFills}</p>
          </div>
          <div>
            <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.3)', margin: '0 0 4px' }}>Avg Fills / Batch</p>
            <p style={{ fontFamily: CN, fontSize: '16px', color: '#e9eaf0', margin: 0 }}>{avgFills}</p>
          </div>
        </div>
      </div>

      <div style={{ background: '#090b11', borderTop: '1px solid rgba(232,234,240,0.08)' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', marginBottom: '8px' }}>
          On-Chain Anchors
        </p>
        <p style={{ fontFamily: IN, fontSize: '12px', color: 'rgba(232,234,240,0.45)', lineHeight: 1.8, maxWidth: '600px', marginBottom: '20px' }}>
          The engine state root is anchored to Ethereum every 10 minutes. Each anchor is a permanent, immutable record of exchange state that cannot be retroactively altered.
        </p>

        <div style={{ padding: '6px 0', borderBottom: '1px solid rgba(232,234,240,0.1)' }} className="grid grid-cols-[80px_1fr_180px_120px_100px]">
          {(['ANCHOR', 'STATE ROOT', 'TIMESTAMP', 'ORDERS', 'TX'] as const).map((h) => (
            <span key={h} style={{ fontFamily: IN, fontSize: '8px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.3)' }}>{h}</span>
          ))}
        </div>

        {anchors.length === 0 ? (
          <p style={{ fontFamily: IN, fontSize: '12px', color: 'rgba(232,234,240,0.35)', padding: '32px 0' }}>
            No anchors yet. First anchor will publish in less than 10 minutes.
          </p>
        ) : (
          anchors.map((anchor) => (
            <div
              key={anchor.anchor_id}
              style={{ padding: '11px 0', borderBottom: '1px solid rgba(232,234,240,0.05)', alignItems: 'center' }}
              className="grid grid-cols-[80px_1fr_180px_120px_100px]"
            >
              <span style={{ fontFamily: IN, fontWeight: 600, fontSize: '11px', color: '#e9eaf0' }}>{formatAnchorId(anchor.anchor_id)}</span>
              <span style={{ fontFamily: CN, fontSize: '10px', color: 'rgba(232,234,240,0.4)' }}>{anchor.state_root.slice(0, 10)}…</span>
              <span style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.45)' }}>{formatDateTime(anchor.timestamp)}</span>
              <span style={{ fontFamily: CN, fontSize: '11px', color: '#e9eaf0' }}>{formatNumber(anchor.orders_processed)}</span>
              <a
                href={anchor.etherscan_url}
                target="_blank"
                rel="noopener noreferrer"
                style={{ fontFamily: IN, fontSize: '10px', color: 'rgba(232,234,240,0.4)', textDecoration: 'none' }}
                onMouseEnter={(e) => (e.currentTarget.style.color = '#b8b1ff')}
                onMouseLeave={(e) => (e.currentTarget.style.color = 'rgba(232,234,240,0.4)')}
              >
                View →
              </a>
            </div>
          ))
        )}
      </div>
      <div style={{ background: '#090b11', borderTop: '1px solid rgba(232,234,240,0.07)', padding: '20px 52px', display: 'flex', gap: '24px', flexWrap: 'wrap' }}>
        <Link href="/proofs" style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.5)', textDecoration: 'underline' }}>
          ZK Proof System →
        </Link>
        <Link href="/tee" style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.5)', textDecoration: 'underline' }}>
          TEE Attestation →
        </Link>
      </div>
    </div>
  )
}
