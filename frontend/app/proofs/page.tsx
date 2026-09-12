'use client'

import { useState, useEffect, useCallback } from 'react'
import Link from 'next/link'
import HexCanvas from '@/components/HexCanvas'

const PF = "'Playfair Display', serif"
const IN = 'Inter, sans-serif'
const CN = "'Courier New', monospace"

const API_URL = process.env.NEXT_PUBLIC_API_URL ?? 'https://vela-engine.fly.dev'

interface ProofStats {
  total_batches: number
  proven: number
  skipped: number
  pending: number
  failed: number
  prover_mode: string
  prover_version: string
  sp1_integration_status: string
  note: string
}

interface ProofRecord {
  batch_id: number
  status: 'proven' | 'pending' | 'skipped' | 'failed'
  prover: string
  generated_at: number | null
  proving_time_ms: number | null
  proof_size_bytes: number | null
}

function formatDateTime(ms: number): string {
  return new Date(ms).toLocaleString('en-US', {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  })
}

function padBatchId(id: number): string {
  return `#${String(id).padStart(3, '0')}`
}

function StatusBadge({ status }: { status: ProofRecord['status'] }) {
  const configs: Record<string, { bg: string; color: string; label: string }> = {
    proven: { bg: 'rgba(107,138,90,0.15)', color: '#6B8A5A', label: 'PROVEN' },
    skipped: { bg: 'rgba(180,140,60,0.1)', color: 'rgba(180,140,60,0.85)', label: 'OPTIMISTIC' },
    pending: { bg: 'rgba(180,140,60,0.08)', color: 'rgba(180,140,60,0.5)', label: 'PENDING' },
    failed: { bg: 'rgba(204,51,51,0.1)', color: '#CC3333', label: 'FAILED' },
  }
  const c = configs[status] ?? configs.pending
  return (
    <span style={{
      fontFamily: IN,
      fontSize: '9px',
      fontWeight: 600,
      textTransform: 'uppercase',
      letterSpacing: '0.1em',
      color: c.color,
      background: c.bg,
      padding: '3px 8px',
    }}>
      {c.label}
    </span>
  )
}

export default function ProofsPage() {
  const [stats, setStats] = useState<ProofStats | null>(null)
  const [proofs, setProofs] = useState<ProofRecord[]>([])
  const [loadingStats, setLoadingStats] = useState(true)
  const [loadingProofs, setLoadingProofs] = useState(true)

  const fetchStats = useCallback(async () => {
    try {
      const res = await fetch(`${API_URL}/proofs/stats`)
      const data = await res.json()
      if (data.ok && data.data) setStats(data.data)
    } catch {
      // silently ignore
    } finally {
      setLoadingStats(false)
    }
  }, [])

  const fetchProofs = useCallback(async () => {
    try {
      const res = await fetch(`${API_URL}/proofs?limit=50`)
      const data = await res.json()
      if (data.ok && data.data?.proofs) setProofs(data.data.proofs)
    } catch {
      // silently ignore
    } finally {
      setLoadingProofs(false)
    }
  }, [])

  useEffect(() => {
    fetchStats()
    fetchProofs()
  }, [fetchStats, fetchProofs])

  useEffect(() => {
    const interval = setInterval(() => {
      fetchStats()
      fetchProofs()
    }, 30_000)
    return () => clearInterval(interval)
  }, [fetchStats, fetchProofs])

  return (
    <div style={{ background: '#0C0C0C', minHeight: '100vh' }}>
      <div style={{ position: 'relative', background: '#0C0C0C', overflow: 'hidden' }} className="px-6 pt-12 pb-16 lg:px-[52px] lg:pt-[60px]">
        <HexCanvas />
        <div style={{ position: 'relative', zIndex: 1, maxWidth: '640px' }}>
          <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,228,216,0.25)', margin: '0 0 24px' }}>
            Vela Exchange — ZK Proof System
          </p>
          <h1 style={{ fontFamily: PF, fontWeight: 400, fontStyle: 'italic', fontSize: 'clamp(32px, 5vw, 52px)', color: '#E8E4D8', margin: '0 0 4px', lineHeight: 1.15 }}>
            Proof pipeline status.
          </h1>
          <h2 style={{ fontFamily: PF, fontWeight: 400, fontStyle: 'italic', fontSize: 'clamp(32px, 5vw, 52px)', color: 'rgba(232,228,216,0.4)', margin: '0 0 24px', lineHeight: 1.15 }}>
            Testnet research preview.
          </h2>
          <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '14px', color: 'rgba(232,228,216,0.4)', lineHeight: 1.8, maxWidth: '540px', margin: 0 }}>
            The current beta defaults to a placeholder prover and optimistic skipped records. Real SP1 proofs require separate provider infrastructure, and no on-chain verifier currently enforces them.
          </p>
        </div>
      </div>

      <div style={{ background: '#E8E4D8' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(12,12,12,0.3)', margin: '0 0 32px' }}>
          Current Prover Mode
        </p>

        <div style={{ borderLeft: '3px solid rgba(180,140,60,0.7)', paddingLeft: '24px', marginBottom: '48px' }}>
          <p style={{ fontFamily: IN, fontSize: '8px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(12,12,12,0.3)', margin: '0 0 8px' }}>CURRENT PROVER MODE</p>
          <p style={{ fontFamily: PF, fontWeight: 700, fontSize: '28px', color: '#0C0C0C', margin: '0 0 12px' }}>OPTIMISTIC</p>
          <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '13px', color: 'rgba(12,12,12,0.55)', lineHeight: 1.7, maxWidth: '480px', margin: '0 0 24px' }}>
            Placeholder records are emitted by default. SP1 integration exists behind explicit provider configuration and is not part of the public beta deployment.
          </p>
          <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
            <p style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(180,140,60,0.8)', margin: 0 }}>● NOW — Optimistic mode (challenge → prove)</p>
            <p style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(12,12,12,0.35)', margin: 0 }}>○ JUNE 2026 — SP1 integration (prove every batch)</p>
            <p style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(12,12,12,0.35)', margin: 0 }}>○ MAINNET — On-chain proof verification</p>
          </div>
        </div>

        {loadingStats ? (
          <p style={{ fontFamily: IN, fontSize: '12px', color: 'rgba(12,12,12,0.3)' }}>Loading stats…</p>
        ) : stats && (
          <div className="grid grid-cols-1 sm:grid-cols-3 gap-[1px]" style={{ background: 'rgba(12,12,12,0.07)' }}>
            {[
              ['TOTAL BATCHES', String(stats.total_batches)],
              ['SKIPPED (OPTIMISTIC)', String(stats.skipped)],
              ['PROVEN', String(stats.proven)],
            ].map(([label, value]) => (
              <div key={label} style={{ background: 'white', padding: '20px 24px' }}>
                <p style={{ fontFamily: IN, fontSize: '8px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(12,12,12,0.3)', margin: '0 0 6px' }}>{label}</p>
                <p style={{ fontFamily: CN, fontSize: '20px', color: '#0C0C0C', margin: 0, fontWeight: 600 }}>{value}</p>
              </div>
            ))}
          </div>
        )}
      </div>

      <div style={{ position: 'relative', background: '#0C0C0C', overflow: 'hidden' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <HexCanvas />
        <div style={{ position: 'relative', zIndex: 1, maxWidth: '720px' }}>
          <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,228,216,0.2)', margin: '0 0 16px' }}>
            Architecture
          </p>
          <h2 style={{ fontFamily: PF, fontWeight: 700, fontSize: '32px', color: '#E8E4D8', margin: '0 0 40px', lineHeight: 1.15 }}>
            Optimistic-ZK verification.
          </h2>
          <div style={{ display: 'flex', flexDirection: 'column', gap: '1px' }}>
            {[
              ['01', 'Batch execution', 'Every 30 seconds, the matching engine produces a batch of fills and computes a state root — a cryptographic commitment to the result.'],
              ['02', 'Optimistic record', 'The beta records the state root without an enforceable on-chain challenge window. The /verify page is an operator contact flow.'],
              ['03', 'ZK proving (coming soon)', 'With SP1 integration, a ZK proof of correct execution is generated for every batch. The proof is published publicly and verifiable by anyone, eliminating the need for the challenge period.'],
            ].map(([step, title, desc]) => (
              <div key={step} style={{ background: 'rgba(232,228,216,0.04)', padding: '24px 28px', display: 'flex', gap: '24px' }}>
                <span style={{ fontFamily: CN, fontSize: '10px', color: 'rgba(232,228,216,0.2)', flexShrink: 0, paddingTop: '3px', minWidth: '24px' }}>
                  {step}
                </span>
                <div>
                  <p style={{ fontFamily: IN, fontWeight: 600, fontSize: '13px', color: '#E8E4D8', margin: '0 0 8px' }}>{title}</p>
                  <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '12px', color: 'rgba(232,228,216,0.4)', margin: 0, lineHeight: 1.7 }}>{desc}</p>
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>

      <div style={{ background: '#E8E4D8' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(12,12,12,0.3)', margin: '0 0 24px' }}>
          Proof History
        </p>

        {loadingProofs ? (
          <p style={{ fontFamily: IN, fontSize: '12px', color: 'rgba(12,12,12,0.3)' }}>Loading proofs…</p>
        ) : proofs.length === 0 ? (
          <p style={{ fontFamily: IN, fontSize: '13px', color: 'rgba(12,12,12,0.35)', padding: '32px 0' }}>
            No proof records yet. In placeholder mode, batches may be recorded as skipped rather than cryptographically proven.
          </p>
        ) : (
          <>
            <div style={{ padding: '8px 0', borderBottom: '1px solid rgba(12,12,12,0.08)', display: 'grid', gridTemplateColumns: '80px 120px 100px 100px 1fr' }}>
              {['BATCH', 'TIMESTAMP', 'STATUS', 'PROVER', 'DETAILS'].map((h) => (
                <span key={h} style={{ fontFamily: IN, fontSize: '8px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(12,12,12,0.3)' }}>{h}</span>
              ))}
            </div>
            {proofs.map((proof) => (
              <div
                key={proof.batch_id}
                style={{ borderBottom: '1px solid rgba(12,12,12,0.05)', padding: '10px 0' }}
              >
                <div style={{ display: 'grid', gridTemplateColumns: '80px 120px 100px 100px 1fr', alignItems: 'center' }}>
                  <span style={{ fontFamily: IN, fontWeight: 600, fontSize: '11px', color: '#0C0C0C' }}>
                    {padBatchId(proof.batch_id)}
                  </span>
                  <span style={{ fontFamily: CN, fontSize: '10px', color: 'rgba(12,12,12,0.4)' }}>
                    {proof.generated_at ? formatDateTime(proof.generated_at) : '—'}
                  </span>
                  <StatusBadge status={proof.status} />
                  <span style={{ fontFamily: CN, fontSize: '9px', color: 'rgba(12,12,12,0.35)' }}>
                    {proof.prover}
                  </span>
                  <Link
                    href={`/batches/${proof.batch_id}`}
                    style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(12,12,12,0.4)', textDecoration: 'none' }}
                    onMouseEnter={(e) => ((e.target as HTMLElement).style.color = '#0C0C0C')}
                    onMouseLeave={(e) => ((e.target as HTMLElement).style.color = 'rgba(12,12,12,0.4)')}
                  >
                    View →
                  </Link>
                </div>
              </div>
            ))}
          </>
        )}
      </div>

      <div style={{ background: '#E8E4D8', borderTop: '1px solid rgba(12,12,12,0.07)', padding: '20px 52px', display: 'flex', gap: '24px', flexWrap: 'wrap' }}>
        <Link href="/batches" style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(12,12,12,0.5)', textDecoration: 'underline' }}>
          ← Batch Explorer
        </Link>
        <Link href="/verify" style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(12,12,12,0.5)', textDecoration: 'underline' }}>
          Submit a challenge →
        </Link>
        <Link href="/tee" style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(12,12,12,0.5)', textDecoration: 'underline' }}>
          TEE Attestation →
        </Link>
      </div>
    </div>
  )
}
