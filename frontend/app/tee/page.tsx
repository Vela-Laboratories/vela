'use client'

import { useState, useEffect, useCallback } from 'react'
import Link from 'next/link'
import HexCanvas from '@/components/HexCanvas'

const PF = "var(--font-inter-sans), 'Inter', sans-serif"
const IN = 'Inter, sans-serif'
const CN = "'Courier New', monospace"

const API_URL = process.env.NEXT_PUBLIC_API_URL ?? 'https://vela-engine.fly.dev'

interface TeeStats {
  platform: string
  platform_status: string
  binary_hash: string
  total_batches: number
  attested: number
  simulated: number
  pending: number
  failed: number
  attestation_roadmap: {
    current: string
    phase_2: string
    phase_3: string
    reference: string
  }
}

interface AttestationRecord {
  batch_id: number
  status: 'attested' | 'simulated' | 'pending' | 'failed'
  platform: string
  binary_hash: string
  generated_at: number
  fill_count: number
}

function padBatchId(id: number): string {
  return `#${String(id).padStart(3, '0')}`
}

function formatDateTime(ms: number): string {
  return new Date(ms).toLocaleString('en-US', {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  })
}

function StatusBadge({ status }: { status: AttestationRecord['status'] }) {
  const configs: Record<string, { bg: string; color: string; label: string }> = {
    attested: { bg: 'rgba(107,138,90,0.15)', color: '#20c98a', label: 'ATTESTED' },
    simulated: { bg: 'rgba(180,140,60,0.1)', color: 'rgba(180,140,60,0.85)', label: 'SIMULATED' },
    pending: { bg: 'rgba(180,140,60,0.08)', color: 'rgba(180,140,60,0.5)', label: 'PENDING' },
    failed: { bg: 'rgba(204,51,51,0.1)', color: '#ef6a5b', label: 'FAILED' },
  }
  const c = configs[status] ?? configs.pending
  return (
    <span style={{
      fontFamily: IN,
      fontSize: '9px',
      fontWeight: 600,
      textTransform: 'uppercase' as const,
      letterSpacing: '0.1em',
      color: c.color,
      background: c.bg,
      padding: '3px 8px',
    }}>
      {c.label}
    </span>
  )
}

export default function TeePage() {
  const [stats, setStats] = useState<TeeStats | null>(null)
  const [attestations, setAttestations] = useState<AttestationRecord[]>([])
  const [loadingStats, setLoadingStats] = useState(true)
  const [loadingAttestations, setLoadingAttestations] = useState(true)

  const fetchStats = useCallback(async () => {
    try {
      const res = await fetch(`${API_URL}/tee/stats`)
      const data = await res.json()
      if (data.ok && data.data) setStats(data.data)
    } catch {
      // silently ignore
    } finally {
      setLoadingStats(false)
    }
  }, [])

  const fetchAttestations = useCallback(async () => {
    try {
      const res = await fetch(`${API_URL}/attestations?limit=50`)
      const data = await res.json()
      if (data.ok && data.data?.attestations) setAttestations(data.data.attestations)
    } catch {
      // silently ignore
    } finally {
      setLoadingAttestations(false)
    }
  }, [])

  useEffect(() => {
    fetchStats()
    fetchAttestations()
  }, [fetchStats, fetchAttestations])

  useEffect(() => {
    const interval = setInterval(() => {
      fetchStats()
      fetchAttestations()
    }, 30_000)
    return () => clearInterval(interval)
  }, [fetchStats, fetchAttestations])

  return (
    <div style={{ background: '#0C0C0C', minHeight: '100vh' }}>
      {/* Page Header */}
      <div style={{ position: 'relative', background: '#0C0C0C', overflow: 'hidden' }} className="px-6 pt-12 pb-16 lg:px-[52px] lg:pt-[60px]">
        <HexCanvas />
        <div style={{ position: 'relative', zIndex: 1, maxWidth: '640px' }}>
          <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.25)', margin: '0 0 24px' }}>
            Vela Exchange — TEE Verification
          </p>
          <h1 style={{ fontFamily: PF, fontWeight: 400, fontStyle: 'italic', fontSize: 'clamp(32px, 5vw, 52px)', color: '#e9eaf0', margin: '0 0 4px', lineHeight: 1.15 }}>
            Trusted hardware.
          </h1>
          <h2 style={{ fontFamily: PF, fontWeight: 400, fontStyle: 'italic', fontSize: 'clamp(32px, 5vw, 52px)', color: 'rgba(232,234,240,0.4)', margin: '0 0 24px', lineHeight: 1.15 }}>
            Verification research.
          </h2>
          <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '14px', color: 'rgba(232,234,240,0.4)', lineHeight: 1.8, maxWidth: '560px', margin: 0 }}>
            Vela is researching hardware-based attestation using AMD SEV-SNP Trusted Execution Environments. The current demo emits simulated records; no production hardware attestation or independently verified deployment is active.
          </p>
        </div>
      </div>

      {/* TEE vs ZK Comparison */}
      <div style={{ background: '#090b11' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', margin: '0 0 16px' }}>
          Two Verification Paths
        </p>
        <h2 style={{ fontFamily: PF, fontWeight: 700, fontSize: '32px', color: '#e9eaf0', margin: '0 0 40px', lineHeight: 1.15 }}>
          Complementary, not competing.
        </h2>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-[1px]" style={{ background: 'rgba(232,234,240,0.05)' }}>
          {/* ZK Proofs column */}
          <div style={{ background: '#0d1018', padding: '28px 32px' }}>
            <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.3)', margin: '0 0 12px' }}>ZK Proofs (VEL-T1-01)</p>
            <h3 style={{ fontFamily: PF, fontWeight: 700, fontSize: '20px', color: '#e9eaf0', margin: '0 0 16px' }}>Trustless cryptography</h3>
            <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '13px', color: 'rgba(232,234,240,0.55)', lineHeight: 1.7, margin: '0 0 12px' }}>
              The matching engine computed the correct output — mathematically, without trusting any hardware or operator.
            </p>
            <p style={{ fontFamily: IN, fontSize: '12px', color: '#20c98a', margin: '0 0 16px' }}>
              Trust model: No hardware trust required. Pure math.
            </p>
            <p style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.4)', margin: '0 0 20px' }}>
              Status: Research design only. The current default prover is a placeholder.
            </p>
            <div style={{ display: 'flex', flexDirection: 'column', gap: '6px' }}>
              {[
                ['○', 'Target: no hardware dependency'],
                ['○', 'Target: publicly verifiable inputs and proofs'],
                ['✗', 'Computationally expensive to generate'],
                ['✗', 'Requires ZK-compatible circuit design'],
              ].map(([mark, text]) => (
                <p key={text} style={{ fontFamily: IN, fontSize: '11px', color: mark === '✓' ? 'rgba(232,234,240,0.55)' : 'rgba(232,234,240,0.3)', margin: 0 }}>
                  {mark} {text}
                </p>
              ))}
            </div>
          </div>

          {/* TEE Attestation column */}
          <div style={{ background: '#0d1018', padding: '28px 32px' }}>
            <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.3)', margin: '0 0 12px' }}>TEE Attestation (VEL-T1-04)</p>
            <h3 style={{ fontFamily: PF, fontWeight: 700, fontSize: '20px', color: '#e9eaf0', margin: '0 0 16px' }}>Hardware trust</h3>
            <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '13px', color: 'rgba(232,234,240,0.55)', lineHeight: 1.7, margin: '0 0 12px' }}>
              Target design: an attestation would bind a measured binary to supported AMD SEV-SNP hardware. This is not active in the deployed demo and would still depend on hardware, firmware, cloud, and verification assumptions.
            </p>
            <p style={{ fontFamily: IN, fontSize: '12px', color: 'rgba(232,234,240,0.45)', margin: '0 0 16px' }}>
              Trust model: Requires trusting AMD/Intel hardware. Stronger operational trust than software alone.
            </p>
            <p style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.4)', margin: '0 0 20px' }}>
              Status: Simulated records only. No hardware-backed deployment is active.
            </p>
            <div style={{ display: 'flex', flexDirection: 'column', gap: '6px' }}>
              {[
                ['○', 'Target: low-latency attestation'],
                ['○', 'Requires a supported confidential-compute VM'],
                ['○', 'Measurement can bind a binary hash to a report'],
                ['✗', 'Requires trusting hardware manufacturer (AMD/Intel)'],
              ].map(([mark, text]) => (
                <p key={text} style={{ fontFamily: IN, fontSize: '11px', color: mark === '✓' ? 'rgba(232,234,240,0.55)' : 'rgba(232,234,240,0.3)', margin: 0 }}>
                  {mark} {text}
                </p>
              ))}
            </div>
          </div>
        </div>
      </div>

      {/* How TEE Works */}
      <div style={{ position: 'relative', background: '#0C0C0C', overflow: 'hidden' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <HexCanvas />
        <div style={{ position: 'relative', zIndex: 1, maxWidth: '720px' }}>
          <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.2)', margin: '0 0 16px' }}>
            Architecture
          </p>
          <h2 style={{ fontFamily: PF, fontWeight: 700, fontSize: '32px', color: '#e9eaf0', margin: '0 0 40px', lineHeight: 1.15 }}>
            AMD SEV-SNP + Vela.
          </h2>
          <div style={{ display: 'flex', flexDirection: 'column', gap: '1px' }}>
            {[
              ['01', 'Confidential VM', 'The Vela matching engine runs inside an AMD SEV-SNP Confidential Virtual Machine. The hardware encrypts all memory and protects execution from the hypervisor, cloud provider, and operator.'],
              ['02', 'Measurement', 'At startup, the AMD hardware measures the exact binary that loaded into the TEE. This measurement is a cryptographic hash of the matching engine code — anyone can verify it matches the published open-source binary.'],
              ['03', 'Attestation report', 'After each batch of trades, the TEE hardware generates an attestation report signed by the AMD VCEK key. This report includes the measurement, the state root, and a timestamp — proving the correct code processed the correct data.'],
              ['04', 'On-chain publication', 'The attestation report is published alongside each state root anchor on Ethereum. Anyone can verify the AMD certificate chain and confirm the report is genuine.'],
            ].map(([step, title, desc]) => (
              <div key={step} style={{ background: 'rgba(232,234,240,0.04)', padding: '24px 28px', display: 'flex', gap: '24px' }}>
                <span style={{ fontFamily: CN, fontSize: '10px', color: 'rgba(232,234,240,0.2)', flexShrink: 0, paddingTop: '3px', minWidth: '24px' }}>
                  {step}
                </span>
                <div>
                  <p style={{ fontFamily: IN, fontWeight: 600, fontSize: '13px', color: '#e9eaf0', margin: '0 0 8px' }}>{title}</p>
                  <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '12px', color: 'rgba(232,234,240,0.4)', margin: 0, lineHeight: 1.7 }}>{desc}</p>
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>

      {/* Current Status */}
      <div style={{ background: '#090b11' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', margin: '0 0 32px' }}>
          Current Status
        </p>

        <div style={{ borderLeft: '3px solid rgba(180,140,60,0.7)', paddingLeft: '24px', marginBottom: '48px' }}>
          <p style={{ fontFamily: IN, fontWeight: 600, fontSize: '14px', color: '#e9eaf0', margin: '0 0 12px' }}>DEVELOPMENT MODE</p>
          <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '13px', color: 'rgba(232,234,240,0.55)', lineHeight: 1.7, maxWidth: '480px', margin: '0 0 24px' }}>
            TEE hardware deployment requires AMD SEV-SNP confidential VMs on Azure, AWS, or GCP. Current deployment (fly.io) uses standard VMs without TEE hardware.
          </p>

          {loadingStats ? (
            <p style={{ fontFamily: IN, fontSize: '12px', color: 'rgba(232,234,240,0.3)' }}>Loading binary hash…</p>
          ) : stats && (
            <div style={{ marginBottom: '24px' }}>
              <p style={{ fontFamily: IN, fontSize: '8px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.3)', margin: '0 0 8px' }}>Current Binary Hash</p>
              <p style={{ fontFamily: CN, fontSize: '9px', color: '#e9eaf0', margin: '0 0 6px', wordBreak: 'break-all' }}>{stats.binary_hash}</p>
              <p style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.4)', margin: 0 }}>
                This is the SHA-256 of the current Vela matching engine binary. In TEE mode, this hash is embedded in the hardware attestation report.
              </p>
            </div>
          )}

          <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
            <p style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(180,140,60,0.8)', margin: 0 }}>● NOW — Placeholder attestation (binary hash computed)</p>
            <p style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.35)', margin: 0 }}>○ JUNE 2026 — AMD SEV-SNP deployment on Azure Confidential VMs</p>
            <p style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.35)', margin: 0 }}>○ POST-MAINNET — NVIDIA H100 GPU attestation for ZK acceleration</p>
            <p style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.35)', margin: 0 }}>○ FUTURE — Intel TDX support via ROFL framework</p>
          </div>
        </div>

        {!loadingStats && stats && (
          <div className="grid grid-cols-1 sm:grid-cols-3 gap-[1px]" style={{ background: 'rgba(232,234,240,0.07)' }}>
            {[
              ['TOTAL BATCHES', String(stats.total_batches)],
              ['SIMULATED', String(stats.simulated)],
              ['ATTESTED', String(stats.attested)],
            ].map(([label, value]) => (
              <div key={label} style={{ background: '#0d1018', padding: '20px 24px' }}>
                <p style={{ fontFamily: IN, fontSize: '8px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.3)', margin: '0 0 6px' }}>{label}</p>
                <p style={{ fontFamily: CN, fontSize: '20px', color: '#e9eaf0', margin: 0, fontWeight: 600 }}>{value}</p>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Attestation History */}
      <div style={{ position: 'relative', background: '#0C0C0C', overflow: 'hidden' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <HexCanvas />
        <div style={{ position: 'relative', zIndex: 1 }}>
          <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.2)', margin: '0 0 24px' }}>
            Attestation History
          </p>

          {loadingAttestations ? (
            <p style={{ fontFamily: IN, fontSize: '12px', color: 'rgba(232,234,240,0.3)' }}>Loading attestations…</p>
          ) : attestations.length === 0 ? (
            <p style={{ fontFamily: IN, fontSize: '13px', color: 'rgba(232,234,240,0.2)', padding: '32px 0' }}>
              No attestations yet. Attestations are generated as batches execute.
            </p>
          ) : (
            <>
              <div style={{ padding: '8px 0', borderBottom: '1px solid rgba(232,234,240,0.06)', display: 'grid', gridTemplateColumns: '80px 120px 100px 100px 1fr' }}>
                {['BATCH', 'TIMESTAMP', 'STATUS', 'PLATFORM', 'DETAILS'].map((h) => (
                  <span key={h} style={{ fontFamily: IN, fontSize: '8px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.2)' }}>{h}</span>
                ))}
              </div>
              {attestations.map((record) => (
                <div
                  key={record.batch_id}
                  style={{ borderBottom: '1px solid rgba(232,234,240,0.04)', padding: '10px 0' }}
                >
                  <div style={{ display: 'grid', gridTemplateColumns: '80px 120px 100px 100px 1fr', alignItems: 'center' }}>
                    <span style={{ fontFamily: IN, fontWeight: 600, fontSize: '11px', color: '#e9eaf0' }}>
                      {padBatchId(record.batch_id)}
                    </span>
                    <span style={{ fontFamily: CN, fontSize: '10px', color: 'rgba(232,234,240,0.3)' }}>
                      {record.generated_at ? formatDateTime(record.generated_at) : '—'}
                    </span>
                    <StatusBadge status={record.status} />
                    <span style={{ fontFamily: CN, fontSize: '9px', color: 'rgba(232,234,240,0.25)' }}>
                      {record.platform}
                    </span>
                    <Link
                      href={`/batches/${record.batch_id}`}
                      style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.3)', textDecoration: 'none' }}
                      onMouseEnter={(e) => ((e.target as HTMLElement).style.color = '#e9eaf0')}
                      onMouseLeave={(e) => ((e.target as HTMLElement).style.color = 'rgba(232,234,240,0.3)')}
                    >
                      View →
                    </Link>
                  </div>
                </div>
              ))}
            </>
          )}
        </div>
      </div>

      <div style={{ background: '#0C0C0C', borderTop: '1px solid rgba(232,234,240,0.06)', padding: '20px 52px', display: 'flex', gap: '24px', flexWrap: 'wrap' }}>
        <Link href="/proofs" style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.4)', textDecoration: 'underline' }}>
          ← ZK Proof System
        </Link>
        <Link href="/batches" style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.4)', textDecoration: 'underline' }}>
          Batch Explorer →
        </Link>
      </div>
    </div>
  )
}
