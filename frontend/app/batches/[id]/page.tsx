'use client'

import { useState, useEffect, useCallback } from 'react'
import Link from 'next/link'
import { useParams } from 'next/navigation'
import HexCanvas from '@/components/HexCanvas'

interface PublicInputs {
  state_root_before: string
  state_root_after: string
  batch_id: number
  fill_count: number
}

interface BatchProofData {
  batch_id: number
  status: 'proven' | 'pending' | 'skipped' | 'failed'
  prover: string
  public_inputs: PublicInputs | null
  proof_bytes: string | null
  generated_at: number | null
  proving_time_ms: number | null
  proof_size_bytes: number | null
  verification_note: string
}

interface AttestationData {
  batch_id: number
  status: 'attested' | 'simulated' | 'pending' | 'failed'
  platform: string
  binary_hash: string | null
  state_root: string | null
  fill_count: number | null
  operator_address: string | null
  generated_at: number | null
  attestation_time_ms: number | null
  attester_version: string | null
  verification_note: string
  attestation_report: string | null
  vcek_cert: string | null
  measurement: string | null
  etherscan_anchor_tx: string | null
}

const PF = "var(--font-inter-sans), 'Inter', sans-serif"
const IN = 'Inter, sans-serif'
const CN = "'Courier New', monospace"

const API_URL = process.env.NEXT_PUBLIC_API_URL ?? 'https://vela-engine.fly.dev'

interface StoredFill {
  id: string
  market_id: string
  price: number
  quantity: number
  maker_order_id: number
  taker_order_id: number
  maker_address: string
  taker_address: string
  timestamp: number
  side: string
}

interface BatchDetail {
  batch_id: number
  timestamp: number
  fill_count: number
  order_count: number
  markets: string[]
  state_root: string
  operator_signature: string
  fills: StoredFill[]
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

function truncateAddress(addr: string): string {
  if (!addr || addr.length < 12) return addr
  return `${addr.slice(0, 6)}...${addr.slice(-4)}`
}

function formatPrice(price: number): string {
  if (price >= 1000) return price.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 })
  if (price >= 1) return price.toFixed(2)
  return price.toFixed(4)
}

function padBatchId(id: number): string {
  return `#${String(id).padStart(3, '0')}`
}

export default function BatchDetailPage() {
  const { id } = useParams<{ id: string }>()
  const [batch, setBatch] = useState<BatchDetail | null>(null)
  const [loading, setLoading] = useState(true)
  const [notFound, setNotFound] = useState(false)
  const [showModal, setShowModal] = useState(false)
  const [coveringAnchor, setCoveringAnchor] = useState<AnchorRecord | null | undefined>(undefined)
  const [proofData, setProofData] = useState<BatchProofData | null>(null)
  const [attestationData, setAttestationData] = useState<AttestationData | null>(null)

  const fetchBatch = useCallback(async () => {
    try {
      const res = await fetch(`${API_URL}/batches/${id}`)
      if (res.status === 404) { setNotFound(true); return }
      const data = await res.json()
      if (data.ok && data.data) {
        setBatch(data.data)
      }
    } catch {
      // silently ignore
    } finally {
      setLoading(false)
    }
  }, [id])

  const fetchAnchors = useCallback(async (batchTimestamp: number) => {
    try {
      const res = await fetch(`${API_URL}/anchors`)
      const data = await res.json()
      if (data.ok && Array.isArray(data.data?.anchors)) {
        const sorted: AnchorRecord[] = data.data.anchors.slice().sort(
          (a: AnchorRecord, b: AnchorRecord) => a.timestamp - b.timestamp
        )
        const covering = sorted.find((a) => a.timestamp >= batchTimestamp) ?? null
        setCoveringAnchor(covering)
      } else {
        setCoveringAnchor(null)
      }
    } catch {
      setCoveringAnchor(null)
    }
  }, [])

  const fetchProof = useCallback(async () => {
    try {
      const res = await fetch(`${API_URL}/batches/${id}/proof`)
      const data = await res.json()
      if (data.ok && data.data) setProofData(data.data)
    } catch {
      // silently ignore
    }
  }, [id])

  const fetchAttestation = useCallback(async () => {
    try {
      const res = await fetch(`${API_URL}/batches/${id}/attestation`)
      const data = await res.json()
      if (data.ok && data.data) setAttestationData(data.data)
    } catch {
      // silently ignore
    }
  }, [id])

  useEffect(() => { fetchBatch() }, [fetchBatch])

  useEffect(() => {
    if (batch) {
      fetchAnchors(batch.timestamp)
      fetchProof()
      fetchAttestation()
    }
  }, [batch, fetchAnchors, fetchProof, fetchAttestation])

  function downloadFills() {
    if (!batch) return
    const blob = new Blob([JSON.stringify(batch.fills, null, 2)], { type: 'application/json' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = `vela-batch-${batch.batch_id}-fills.json`
    a.click()
    URL.revokeObjectURL(url)
  }

  if (loading) {
    return (
      <div style={{ background: '#15131f', minHeight: '100vh', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
        <p style={{ fontFamily: IN, fontSize: '12px', color: 'rgba(232,234,240,0.3)' }}>Loading…</p>
      </div>
    )
  }

  if (notFound || !batch) {
    return (
      <div style={{ background: '#15131f', minHeight: '100vh', display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', gap: '16px' }}>
        <p style={{ fontFamily: IN, fontSize: '14px', color: 'rgba(232,234,240,0.4)' }}>Batch {id} not found.</p>
        <Link href="/batches" style={{ fontFamily: IN, fontSize: '11px', color: '#f8f5ee', textDecoration: 'underline' }}>← Back to batch explorer</Link>
      </div>
    )
  }

  return (
    <div style={{ background: '#15131f', minHeight: '100vh' }}>
      <div style={{ position: 'relative', background: '#15131f', overflow: 'hidden' }} className="px-6 pt-12 pb-12 lg:px-[52px] lg:pt-[60px]">
        <HexCanvas />
        <div style={{ position: 'relative', zIndex: 1 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: '16px', marginBottom: '16px' }}>
            <p style={{ fontFamily: IN, fontSize: '10px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', margin: 0 }}>
              Vela — Batch Explorer
            </p>
            <span style={{ fontFamily: IN, fontSize: '9px', color: '#20c98a', border: '1px solid #20c98a', padding: '2px 8px', textTransform: 'uppercase', letterSpacing: '0.1em' }}>
              Finalized
            </span>
          </div>
          <h1 style={{ fontFamily: PF, fontWeight: 900, fontSize: '48px', color: '#f8f5ee', margin: '0 0 8px', lineHeight: 1.1 }}>
            Batch {padBatchId(batch.batch_id)}
          </h1>
          <p style={{ fontFamily: IN, fontSize: '13px', color: 'rgba(232,234,240,0.35)', margin: '0 0 4px' }}>
            {new Date(batch.timestamp).toLocaleString()} · {batch.fill_count} fills
          </p>
        </div>
      </div>

      <div style={{ background: '#13111d' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <p style={{ fontFamily: IN, fontSize: '10px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', marginBottom: '24px' }}>
          Batch Details
        </p>

        <div className="grid grid-cols-1 sm:grid-cols-3 lg:grid-cols-5 gap-[1px] mb-10" style={{ background: 'rgba(232,234,240,0.07)' }}>
          {[
            ['BATCH ID', padBatchId(batch.batch_id)],
            ['TIMESTAMP', new Date(batch.timestamp).toLocaleString()],
            ['FILL COUNT', String(batch.fill_count)],
            ['MARKETS', batch.markets.join(', ')],
            ['ORDER COUNT', String(batch.order_count)],
          ].map(([label, value]) => (
            <div key={label} style={{ background: '#211e2b', padding: '20px 24px' }}>
              <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.3)', margin: '0 0 6px' }}>{label}</p>
              <p style={{ fontFamily: CN, fontSize: '12px', color: '#f8f5ee', margin: 0 }}>{value}</p>
            </div>
          ))}
        </div>

        <div style={{ marginBottom: '36px' }}>
          <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.3)', marginBottom: '10px' }}>
            State Root
          </p>
          <div style={{ background: '#15131f', padding: '16px 20px', marginBottom: '12px' }}>
            <p style={{ fontFamily: CN, fontSize: '11px', color: '#f8f5ee', margin: 0, wordBreak: 'break-all' }}>{batch.state_root}</p>
          </div>
          <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '13px', color: 'rgba(232,234,240,0.5)', lineHeight: 1.8, maxWidth: '560px', marginBottom: '16px' }}>
            The state root is a cryptographic hash of all trades in this batch. Anyone can recompute this hash from the fills below and verify it matches — proving the batch was not tampered with.
          </p>

          {coveringAnchor !== undefined && (
            <div style={{ marginBottom: '16px' }}>
              {coveringAnchor ? (
                <div>
                  <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.12em', color: '#20c98a', margin: '0 0 6px' }}>
                    ● ANCHORED ON-CHAIN
                  </p>
                  <p style={{ fontFamily: IN, fontSize: '12px', color: 'rgba(232,234,240,0.55)', margin: '0 0 4px' }}>
                    State root anchored in Ethereum tx:{' '}
                    <span style={{ fontFamily: CN, fontSize: '11px' }}>
                      {coveringAnchor.tx_hash.slice(0, 10)}…{coveringAnchor.tx_hash.slice(-6)}
                    </span>
                  </p>
                  <a
                    href={coveringAnchor.etherscan_url}
                    target="_blank"
                    rel="noopener noreferrer"
                    style={{ fontFamily: IN, fontSize: '11px', color: '#f8f5ee', textDecoration: 'underline' }}
                  >
                    View on Etherscan →
                  </a>
                </div>
              ) : (
                <div>
                  <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.12em', color: 'rgba(232,234,240,0.3)', margin: '0 0 6px' }}>
                    ○ PENDING ANCHOR
                  </p>
                  <p style={{ fontFamily: IN, fontSize: '12px', color: 'rgba(232,234,240,0.45)', margin: 0 }}>
                    This batch will be included in the next on-chain anchor (every 10 minutes).
                  </p>
                </div>
              )}
            </div>
          )}

          <button
            onClick={() => setShowModal(true)}
            style={{ fontFamily: IN, fontSize: '10px', color: '#f8f5ee', background: 'transparent', border: '1px solid rgba(232,234,240,0.15)', padding: '8px 16px', cursor: 'pointer' }}
          >
            Verify this batch
          </button>
        </div>

        <div>
          <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.3)', marginBottom: '10px' }}>
            Operator Signature
          </p>
          <div style={{ background: '#15131f', padding: '12px 16px', marginBottom: '10px' }}>
            <p style={{ fontFamily: CN, fontSize: '9px', color: 'rgba(232,234,240,0.3)', margin: 0, wordBreak: 'break-all' }}>{batch.operator_signature}</p>
          </div>
          <p style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.35)' }}>
            Real cryptographic signatures will be published at mainnet.
          </p>
        </div>

        <div style={{ background: '#15131f', padding: '20px 24px', marginTop: '36px' }}>
          <p style={{ fontFamily: IN, fontSize: '8px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.2)', margin: '0 0 14px' }}>
            ZK Proof Status
          </p>
          {!proofData ? (
            <p style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.3)', margin: 0 }}>Loading proof status…</p>
          ) : proofData.status === 'skipped' ? (
            <div>
              <div style={{ display: 'flex', alignItems: 'center', gap: '8px', marginBottom: '8px' }}>
                <div style={{ width: '8px', height: '8px', borderRadius: '50%', background: 'rgba(180,140,60,0.7)', flexShrink: 0 }} />
                <span style={{ fontFamily: IN, fontWeight: 600, fontSize: '12px', color: 'rgba(180,140,60,0.9)' }}>OPTIMISTIC MODE</span>
              </div>
              <p style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.35)', margin: '0 0 14px', lineHeight: 1.7 }}>
                This batch is secured by optimistic verification. A ZK proof will be generated if this batch is challenged during the 7-day challenge window.
              </p>
              {proofData.public_inputs && (
                <div style={{ display: 'flex', flexDirection: 'column', gap: '6px', marginBottom: '12px' }}>
                  {[
                    ['STATE ROOT BEFORE', proofData.public_inputs.state_root_before.slice(0, 18) + '…'],
                    ['STATE ROOT AFTER', proofData.public_inputs.state_root_after.slice(0, 18) + '…'],
                    ['FILL COUNT', String(proofData.public_inputs.fill_count)],
                  ].map(([label, value]) => (
                    <div key={label} style={{ display: 'flex', gap: '12px' }}>
                      <span style={{ fontFamily: IN, fontSize: '8px', textTransform: 'uppercase', letterSpacing: '0.12em', color: 'rgba(232,234,240,0.2)', minWidth: '130px' }}>{label}</span>
                      <span style={{ fontFamily: CN, fontSize: '10px', color: 'rgba(232,234,240,0.45)' }}>{value}</span>
                    </div>
                  ))}
                </div>
              )}
            </div>
          ) : proofData.status === 'proven' ? (
            <div>
              <div style={{ display: 'flex', alignItems: 'center', gap: '8px', marginBottom: '8px' }}>
                <div style={{ width: '8px', height: '8px', borderRadius: '50%', background: '#20c98a', flexShrink: 0 }} />
                <span style={{ fontFamily: IN, fontWeight: 600, fontSize: '12px', color: '#20c98a' }}>PROVEN</span>
              </div>
              <p style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.35)', margin: '0 0 10px' }}>
                Prover: {proofData.prover} · {proofData.proving_time_ms}ms · {proofData.proof_size_bytes ? `${proofData.proof_size_bytes} bytes` : 'size unknown'}
              </p>
              <button
                disabled
                title="On-chain verification coming at mainnet"
                style={{ fontFamily: IN, fontSize: '10px', color: 'rgba(232,234,240,0.3)', background: 'transparent', border: '1px solid rgba(232,234,240,0.1)', padding: '6px 14px', cursor: 'not-allowed' }}
              >
                VERIFY
              </button>
            </div>
          ) : proofData.status === 'pending' ? (
            <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
              <div style={{ width: '8px', height: '8px', borderRadius: '50%', background: 'rgba(180,140,60,0.5)', flexShrink: 0, animation: 'pulse 1.5s ease-in-out infinite' }} />
              <span style={{ fontFamily: IN, fontSize: '12px', color: 'rgba(232,234,240,0.3)' }}>PROOF PENDING — Check back shortly.</span>
            </div>
          ) : (
            <div>
              <div style={{ display: 'flex', alignItems: 'center', gap: '8px', marginBottom: '6px' }}>
                <div style={{ width: '8px', height: '8px', borderRadius: '50%', background: '#ef6a5b', flexShrink: 0 }} />
                <span style={{ fontFamily: IN, fontWeight: 600, fontSize: '12px', color: '#ef6a5b' }}>PROOF FAILED</span>
              </div>
              <p style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.3)', margin: 0 }}>
                {proofData.verification_note}
              </p>
            </div>
          )}
          <p style={{ fontFamily: IN, fontSize: '9px', color: 'rgba(232,234,240,0.15)', margin: '14px 0 0', lineHeight: 1.6 }}>
            Full ZK proof generation for every batch ships post-Stanford AFT Lab (June 2026). Powered by SP1.{' '}
            <a
              href="https://github.com/succinctlabs/sp1"
              target="_blank"
              rel="noopener noreferrer"
              style={{ color: 'rgba(232,234,240,0.3)', textDecoration: 'underline' }}
            >
              Learn more about SP1 →
            </a>
          </p>
        </div>

        <div style={{ borderTop: '1px solid rgba(232,234,240,0.06)', marginTop: '16px', paddingTop: '16px' }}>
          <p style={{ fontFamily: IN, fontSize: '8px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.2)', margin: '0 0 14px' }}>
            TEE Attestation
          </p>
          {!attestationData ? (
            <p style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.3)', margin: 0 }}>Loading attestation status…</p>
          ) : attestationData.status === 'simulated' ? (
            <div>
              <div style={{ display: 'flex', alignItems: 'center', gap: '8px', marginBottom: '8px' }}>
                <div style={{ width: '8px', height: '8px', borderRadius: '50%', background: 'rgba(180,140,60,0.7)', flexShrink: 0 }} />
                <span style={{ fontFamily: IN, fontWeight: 600, fontSize: '12px', color: 'rgba(180,140,60,0.9)' }}>SIMULATED</span>
              </div>
              <p style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.35)', margin: '0 0 12px', lineHeight: 1.7 }}>
                Hardware attestation requires AMD SEV-SNP deployment. This record confirms the structural integrity of the attestation pipeline.
              </p>
              {attestationData.binary_hash && (
                <div style={{ display: 'flex', flexDirection: 'column', gap: '6px', marginBottom: '12px' }}>
                  {[
                    ['BINARY HASH', attestationData.binary_hash.slice(7, 23) + '…'],
                    ['PLATFORM', 'Placeholder'],
                    ['STATE ROOT', attestationData.state_root ? attestationData.state_root.slice(0, 10) + '…' : '—'],
                    ['FILL COUNT', String(attestationData.fill_count ?? '—')],
                  ].map(([label, value]) => (
                    <div key={label} style={{ display: 'flex', gap: '12px' }}>
                      <span style={{ fontFamily: IN, fontSize: '8px', textTransform: 'uppercase', letterSpacing: '0.12em', color: 'rgba(232,234,240,0.2)', minWidth: '100px' }}>{label}</span>
                      <span style={{ fontFamily: CN, fontSize: '10px', color: 'rgba(232,234,240,0.45)' }}>{value}</span>
                    </div>
                  ))}
                </div>
              )}
              <p style={{ fontFamily: IN, fontSize: '9px', color: 'rgba(232,234,240,0.15)', margin: '8px 0 0', lineHeight: 1.6 }}>
                Real TEE attestation ships June 2026. Platform: AMD SEV-SNP + NVIDIA H100
              </p>
            </div>
          ) : attestationData.status === 'attested' ? (
            <div>
              <div style={{ display: 'flex', alignItems: 'center', gap: '8px', marginBottom: '8px' }}>
                <div style={{ width: '8px', height: '8px', borderRadius: '50%', background: '#20c98a', flexShrink: 0 }} />
                <span style={{ fontFamily: IN, fontWeight: 600, fontSize: '12px', color: '#20c98a' }}>ATTESTED — AMD SEV-SNP</span>
              </div>
              <div style={{ display: 'flex', flexDirection: 'column', gap: '6px', marginBottom: '12px' }}>
                {[
                  ['BINARY HASH', attestationData.binary_hash ?? '—'],
                  ['MEASUREMENT', attestationData.measurement ?? '—'],
                  ['VCEK CERT', attestationData.vcek_cert ? 'View certificate chain →' : '—'],
                  ['PLATFORM', 'AMD SEV-SNP'],
                ].map(([label, value]) => (
                  <div key={label} style={{ display: 'flex', gap: '12px' }}>
                    <span style={{ fontFamily: IN, fontSize: '8px', textTransform: 'uppercase', letterSpacing: '0.12em', color: 'rgba(232,234,240,0.2)', minWidth: '100px' }}>{label}</span>
                    <span style={{ fontFamily: CN, fontSize: '10px', color: 'rgba(232,234,240,0.45)' }}>{value}</span>
                  </div>
                ))}
              </div>
              <button
                disabled
                title="On-chain verification coming at mainnet"
                style={{ fontFamily: IN, fontSize: '10px', color: 'rgba(232,234,240,0.3)', background: 'transparent', border: '1px solid rgba(232,234,240,0.1)', padding: '6px 14px', cursor: 'not-allowed' }}
              >
                VERIFY ATTESTATION
              </button>
            </div>
          ) : attestationData.status === 'pending' ? (
            <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
              <div style={{ width: '8px', height: '8px', borderRadius: '50%', background: 'rgba(180,140,60,0.5)', flexShrink: 0, animation: 'pulse 1.5s ease-in-out infinite' }} />
              <span style={{ fontFamily: IN, fontSize: '12px', color: 'rgba(232,234,240,0.3)' }}>ATTESTATION PENDING — Check back shortly.</span>
            </div>
          ) : (
            <div>
              <div style={{ display: 'flex', alignItems: 'center', gap: '8px', marginBottom: '6px' }}>
                <div style={{ width: '8px', height: '8px', borderRadius: '50%', background: '#ef6a5b', flexShrink: 0 }} />
                <span style={{ fontFamily: IN, fontWeight: 600, fontSize: '12px', color: '#ef6a5b' }}>ATTESTATION FAILED</span>
              </div>
              <p style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.3)', margin: 0 }}>
                {attestationData.verification_note}
              </p>
            </div>
          )}
        </div>
      </div>

      <div style={{ position: 'relative', background: '#15131f', overflow: 'hidden' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <HexCanvas />
        <div style={{ position: 'relative', zIndex: 1 }}>
          <h2 style={{ fontFamily: PF, fontWeight: 700, fontSize: '24px', color: '#f8f5ee', margin: '0 0 32px' }}>
            Fills in this batch
          </h2>

          <div style={{ padding: '8px 0', borderBottom: '1px solid rgba(232,234,240,0.06)' }} className="grid grid-cols-[1fr_80px_100px_80px] lg:grid-cols-[1fr_100px_120px_100px_180px_180px]">
            {['Time', 'Market', 'Price', 'Size', 'Maker', 'Taker'].map((h, i) => (
              <span key={h} style={{ fontFamily: IN, fontSize: '8px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.2)' }} className={i >= 4 ? 'hidden lg:block' : ''}>{h}</span>
            ))}
          </div>

          {batch.fills.length === 0 ? (
            <p style={{ fontFamily: IN, fontSize: '12px', color: 'rgba(232,234,240,0.2)', textAlign: 'center', padding: '48px 0' }}>No fills in this batch.</p>
          ) : (
            batch.fills.map((fill) => {
              const priceNum = fill.price / 1e8
              const sizeNum = fill.quantity / 1e8
              return (
                <div
                  key={fill.id}
                  style={{ padding: '9px 0', borderBottom: '1px solid rgba(232,234,240,0.04)', alignItems: 'center' }}
                  className="grid grid-cols-[1fr_80px_100px_80px] lg:grid-cols-[1fr_100px_120px_100px_180px_180px]"
                >
                  <span style={{ fontFamily: CN, fontSize: '10px', color: 'rgba(232,234,240,0.3)' }}>
                    {new Date(fill.timestamp / 1000).toLocaleTimeString()}
                  </span>
                  <span style={{ fontFamily: IN, fontWeight: 500, fontSize: '11px', color: '#f8f5ee' }}>{fill.market_id}</span>
                  <span style={{ fontFamily: CN, fontSize: '11px', color: fill.side === 'bid' ? '#20c98a' : '#ef6a5b' }}>
                    {formatPrice(priceNum)}
                  </span>
                  <span style={{ fontFamily: CN, fontSize: '10px', color: 'rgba(232,234,240,0.6)' }}>{sizeNum.toFixed(4)}</span>
                  <a
                    href={`/orders/${fill.maker_order_id}`}
                    style={{ fontFamily: CN, fontSize: '10px', color: 'rgba(232,234,240,0.4)', textDecoration: 'none' }}
                    onMouseEnter={(e) => (e.currentTarget.style.color = 'rgba(232,234,240,0.7)')}
                    onMouseLeave={(e) => (e.currentTarget.style.color = 'rgba(232,234,240,0.4)')}
                    className="hidden lg:block"
                  >
                    {truncateAddress(fill.maker_address)}
                  </a>
                  <a
                    href={`/orders/${fill.taker_order_id}`}
                    style={{ fontFamily: CN, fontSize: '10px', color: 'rgba(232,234,240,0.4)', textDecoration: 'none' }}
                    onMouseEnter={(e) => (e.currentTarget.style.color = 'rgba(232,234,240,0.7)')}
                    onMouseLeave={(e) => (e.currentTarget.style.color = 'rgba(232,234,240,0.4)')}
                    className="hidden lg:block"
                  >
                    {truncateAddress(fill.taker_address)}
                  </a>
                </div>
              )
            })
          )}
        </div>
      </div>

      <div style={{ background: '#13111d', padding: '24px 52px', display: 'flex', gap: '24px', flexWrap: 'wrap' }}>
        <Link href="/batches" style={{ fontFamily: IN, fontSize: '11px', color: '#f8f5ee', textDecoration: 'underline' }}>
          ← Back to batch explorer
        </Link>
        <Link href="/proofs" style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.5)', textDecoration: 'underline' }}>
          ZK Proof System →
        </Link>
        <Link href="/tee" style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.5)', textDecoration: 'underline' }}>
          TEE Attestation →
        </Link>
      </div>

      {showModal && (
        <div
          style={{ position: 'fixed', inset: 0, background: 'rgba(232,234,240,0.75)', display: 'flex', alignItems: 'center', justifyContent: 'center', zIndex: 100 }}
          onClick={() => setShowModal(false)}
        >
          <div
            style={{ background: '#13111d', padding: '40px', maxWidth: '480px', width: '90%' }}
            onClick={(e) => e.stopPropagation()}
          >
            <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.3)', marginBottom: '16px' }}>
              Verify Batch {padBatchId(batch.batch_id)}
            </p>
            <h3 style={{ fontFamily: PF, fontWeight: 700, fontSize: '22px', color: '#f8f5ee', margin: '0 0 24px' }}>
              How to verify this batch
            </h3>
            {[
              ['1', 'Download fills data as JSON', 'Get the raw fill records from this batch.'],
              ['2', 'Compute keccak256 of fill IDs concatenated', 'Hash all fill ID strings in order. The result must match the state root above.'],
              ['3', 'Compare to state root', `Expected: ${batch.state_root.slice(0, 18)}…`],
            ].map(([num, title, desc]) => (
              <div key={num} style={{ display: 'flex', gap: '16px', marginBottom: '20px' }}>
                <span style={{ fontFamily: CN, fontSize: '11px', color: 'rgba(232,234,240,0.3)', flexShrink: 0, paddingTop: '2px' }}>{num}.</span>
                <div>
                  <p style={{ fontFamily: IN, fontWeight: 600, fontSize: '13px', color: '#f8f5ee', margin: '0 0 4px' }}>{title}</p>
                  <p style={{ fontFamily: IN, fontSize: '12px', color: 'rgba(232,234,240,0.5)', margin: 0 }}>{desc}</p>
                </div>
              </div>
            ))}
            <div style={{ display: 'flex', gap: '12px', marginTop: '24px' }}>
              <button
                onClick={downloadFills}
                style={{ fontFamily: IN, fontWeight: 600, fontSize: '11px', color: '#f8f5ee', background: '#15131f', border: 'none', padding: '10px 20px', cursor: 'pointer' }}
              >
                Download fills JSON
              </button>
              <button
                onClick={() => setShowModal(false)}
                style={{ fontFamily: IN, fontSize: '11px', color: '#f8f5ee', background: 'transparent', border: '1px solid rgba(232,234,240,0.15)', padding: '10px 20px', cursor: 'pointer' }}
              >
                Close
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
