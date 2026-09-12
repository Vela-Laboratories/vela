'use client'

import { useState, useEffect, useCallback } from 'react'
import Link from 'next/link'
import HexCanvas from '@/components/HexCanvas'

const PF = "'Playfair Display', serif"
const IN = 'Inter, sans-serif'
const CN = "'Courier New', monospace"

const API_URL = process.env.NEXT_PUBLIC_API_URL ?? 'https://vela-engine.fly.dev'

interface StateRoot {
  state_root: string
  timestamp: number
  order_count: number
  user_count: number
  block_number: number | null
}

const STEPS = [
  {
    num: '01',
    title: 'Download the state',
    desc: 'Get the state root for any batch from the batch explorer. This is a cryptographic commitment to every trade in the batch.',
  },
  {
    num: '02',
    title: 'Verify locally',
    desc: 'Recompute the state root from the fills data. If your computation matches, the batch is correct. If it doesn\'t match, you\'ve found a discrepancy.',
  },
  {
    num: '03',
    title: 'Report a discrepancy',
    desc: 'Send the batch ID and your reproduction details to the operator. The beta has no on-chain challenge, slashing, or make-whole mechanism.',
  },
]

export default function VerifyPage() {
  const [stateRoot, setStateRoot] = useState<StateRoot | null>(null)
  const [batchId, setBatchId] = useState('')
  const [description, setDescription] = useState('')
  const [wallet, setWallet] = useState('')
  const [submitted, setSubmitted] = useState(false)

  const fetchStateRoot = useCallback(async () => {
    try {
      const res = await fetch(`${API_URL}/state-root`)
      const data = await res.json()
      if (data.ok && data.data) {
        setStateRoot(data.data)
      }
    } catch {
      // silently ignore
    }
  }, [])

  useEffect(() => {
    fetchStateRoot()
    const t = setInterval(fetchStateRoot, 60_000)
    return () => clearInterval(t)
  }, [fetchStateRoot])

  function downloadSnapshot() {
    if (!stateRoot) return
    const payload = {
      stateRoot: stateRoot.state_root,
      timestamp: stateRoot.timestamp,
      orderCount: stateRoot.order_count,
      userCount: stateRoot.user_count,
      message: 'Download fills from /batches to verify',
    }
    const blob = new Blob([JSON.stringify(payload, null, 2)], { type: 'application/json' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = 'vela-state-snapshot.json'
    a.click()
    URL.revokeObjectURL(url)
  }

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    const subject = 'Vela+Fraud+Proof+Challenge'
    const body = encodeURIComponent(
      `Batch ID: ${batchId}\nWallet: ${wallet}\n\nDiscrepancy:\n${description}`
    )
    window.location.href = `mailto:arya@monolithsystematic.com?subject=${subject}&body=${body}`
    setSubmitted(true)
  }

  const inputStyle: React.CSSProperties = {
    fontFamily: CN,
    fontSize: '12px',
    color: '#0C0C0C',
    background: 'white',
    border: '1px solid rgba(12,12,12,0.15)',
    padding: '10px 14px',
    outline: 'none',
    width: '100%',
    boxSizing: 'border-box',
  }

  const labelStyle: React.CSSProperties = {
    fontFamily: IN,
    fontSize: '8px',
    textTransform: 'uppercase' as const,
    letterSpacing: '0.15em',
    color: 'rgba(12,12,12,0.4)',
    display: 'block',
    marginBottom: '6px',
  }

  return (
    <div style={{ background: '#0C0C0C', minHeight: '100vh' }}>
      <div style={{ position: 'relative', background: '#0C0C0C', overflow: 'hidden' }} className="px-6 pt-12 pb-12 lg:px-[52px] lg:pt-[60px]">
        <HexCanvas />
        <div style={{ position: 'relative', zIndex: 1 }}>
          <p style={{ fontFamily: IN, fontSize: '10px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,228,216,0.3)', marginBottom: '16px' }}>
            Fraud Proof System
          </p>
          <div>
            <span style={{ fontFamily: PF, fontWeight: 900, fontSize: '48px', color: '#E8E4D8', display: 'block', lineHeight: 1.1 }}>
              Inspect any batch.
            </span>
            <span style={{ fontFamily: PF, fontWeight: 400, fontStyle: 'italic', fontSize: '48px', color: 'rgba(232,228,216,0.3)', display: 'block', lineHeight: 1.1 }}>
              Report discrepancies.
            </span>
          </div>
          <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '15px', lineHeight: 1.75, color: 'rgba(232,228,216,0.4)', maxWidth: '560px', marginTop: '20px' }}>
            The beta publishes local state commitments and includes deterministic replay code. SP1 proofs, an on-chain verifier, enforceable challenges, and operator slashing are not active.
          </p>
        </div>
      </div>

      <div style={{ background: '#E8E4D8' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <p style={{ fontFamily: IN, fontSize: '10px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(12,12,12,0.3)', marginBottom: '24px' }}>
          How It Works
        </p>

        <div className="grid grid-cols-1 lg:grid-cols-3 gap-[1px]" style={{ background: 'rgba(12,12,12,0.07)' }}>
          {STEPS.map((step) => (
            <div key={step.num} style={{ background: 'white', padding: '28px 28px' }}>
              <p style={{ fontFamily: CN, fontSize: '11px', color: 'rgba(12,12,12,0.2)', margin: '0 0 12px' }}>Step {step.num}</p>
              <p style={{ fontFamily: IN, fontWeight: 600, fontSize: '15px', color: '#0C0C0C', margin: '0 0 10px' }}>{step.title}</p>
              <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '13px', color: 'rgba(12,12,12,0.55)', lineHeight: 1.8, margin: 0 }}>{step.desc}</p>
            </div>
          ))}
        </div>
      </div>

      <div style={{ position: 'relative', background: '#0C0C0C', overflow: 'hidden' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <HexCanvas />
        <div style={{ position: 'relative', zIndex: 1 }}>
          <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,228,216,0.25)', marginBottom: '12px' }}>
            Current State Root
          </p>
          <div style={{ background: 'rgba(232,228,216,0.03)', border: '1px solid rgba(232,228,216,0.07)', padding: '20px 24px', marginBottom: '24px' }}>
            <p style={{ fontFamily: CN, fontWeight: 700, fontSize: '14px', color: '#E8E4D8', margin: 0, wordBreak: 'break-all', lineHeight: 1.6 }}>
              {stateRoot ? stateRoot.state_root : '—'}
            </p>
          </div>

          <div style={{ display: 'flex', gap: '40px', marginBottom: '28px' }}>
            <div>
              <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,228,216,0.25)', margin: '0 0 4px' }}>Order Count</p>
              <p style={{ fontFamily: CN, fontSize: '14px', color: '#E8E4D8', margin: 0 }}>{stateRoot?.order_count ?? '—'}</p>
            </div>
            <div>
              <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,228,216,0.25)', margin: '0 0 4px' }}>User Count</p>
              <p style={{ fontFamily: CN, fontSize: '14px', color: '#E8E4D8', margin: 0 }}>{stateRoot?.user_count ?? '—'}</p>
            </div>
            <div>
              <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,228,216,0.25)', margin: '0 0 4px' }}>Timestamp</p>
              <p style={{ fontFamily: CN, fontSize: '12px', color: 'rgba(232,228,216,0.5)', margin: 0 }}>
                {stateRoot ? new Date(stateRoot.timestamp).toLocaleTimeString() : '—'}
              </p>
            </div>
          </div>

          <button
            onClick={downloadSnapshot}
            disabled={!stateRoot}
            style={{ fontFamily: IN, fontSize: '10px', color: stateRoot ? '#E8E4D8' : 'rgba(232,228,216,0.3)', background: 'transparent', border: `1px solid ${stateRoot ? 'rgba(232,228,216,0.25)' : 'rgba(232,228,216,0.08)'}`, padding: '8px 16px', cursor: stateRoot ? 'pointer' : 'not-allowed' }}
          >
            Download state snapshot
          </button>
        </div>
      </div>

      <div style={{ background: '#E8E4D8' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <h2 style={{ fontFamily: PF, fontWeight: 700, fontSize: '32px', color: '#0C0C0C', margin: '0 0 12px' }}>
          Found a discrepancy?
        </h2>
        <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '14px', lineHeight: 1.8, color: 'rgba(12,12,12,0.5)', maxWidth: '520px', marginBottom: '36px' }}>
          If you believe a batch was processed incorrectly, send a report with the batch ID and reproduction details. This form opens an email; it does not submit an on-chain challenge.
        </p>

        {submitted ? (
          <div style={{ background: 'white', border: '1px solid rgba(12,12,12,0.1)', padding: '28px', maxWidth: '520px' }}>
            <p style={{ fontFamily: IN, fontWeight: 600, fontSize: '14px', color: '#0C0C0C', margin: '0 0 8px' }}>Report prepared.</p>
            <p style={{ fontFamily: IN, fontSize: '13px', color: 'rgba(12,12,12,0.55)', margin: 0 }}>
              Your email client was opened for you to send the report. This is not an on-chain challenge and no response time is guaranteed.
            </p>
          </div>
        ) : (
          <form onSubmit={handleSubmit} style={{ display: 'flex', flexDirection: 'column', gap: '20px' }} className="w-full max-w-[520px]">
            <div>
              <label style={labelStyle}>Batch ID</label>
              <input
                type="text"
                value={batchId}
                onChange={(e) => setBatchId(e.target.value)}
                placeholder="#001"
                required
                style={inputStyle}
              />
            </div>
            <div>
              <label style={labelStyle}>Discrepancy Description</label>
              <textarea
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                placeholder="Describe what you believe is incorrect and how you verified it…"
                required
                style={{ ...inputStyle, fontFamily: IN, fontSize: '13px', minHeight: '120px', resize: 'vertical' }}
              />
            </div>
            <div>
              <label style={labelStyle}>Your Wallet Address</label>
              <input
                type="text"
                value={wallet}
                onChange={(e) => setWallet(e.target.value)}
                placeholder="0x..."
                required
                style={inputStyle}
              />
            </div>
            <button
              type="submit"
              style={{ fontFamily: IN, fontWeight: 600, fontSize: '11px', textTransform: 'uppercase', letterSpacing: '0.1em', color: '#E8E4D8', background: '#0C0C0C', border: 'none', padding: '12px 24px', cursor: 'pointer', alignSelf: 'flex-start' }}
            >
              Submit Challenge →
            </button>
          </form>
        )}
      </div>

      <div style={{ position: 'relative', background: '#0C0C0C', overflow: 'hidden' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <HexCanvas />
        <div style={{ position: 'relative', zIndex: 1 }}>
          <h2 style={{ fontFamily: PF, fontWeight: 700, fontSize: '24px', color: '#E8E4D8', margin: '0 0 24px' }}>
            Challenge history
          </h2>
          <p style={{ fontFamily: IN, fontSize: '12px', color: 'rgba(232,228,216,0.25)', textAlign: 'center', padding: '32px 0' }}>
            No challenges submitted yet.
          </p>
          <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '12px', color: 'rgba(232,228,216,0.2)', maxWidth: '480px' }}>
            All submitted challenges and their resolutions will be published here publicly.
          </p>
          <div style={{ marginTop: '32px' }}>
            <Link href="/proofs" style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,228,216,0.3)', textDecoration: 'underline' }}>
              View ZK proof system →
            </Link>
          </div>
        </div>
      </div>
    </div>
  )
}
