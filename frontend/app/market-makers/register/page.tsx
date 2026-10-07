'use client'

import { useState } from 'react'
import HexCanvas from '@/components/HexCanvas'
import { useAuth } from '@/lib/auth'

const PF = "var(--font-inter-sans), 'Inter', sans-serif"
const IN = 'Inter, sans-serif'
const CN = "'Courier New', monospace"

const API_URL = process.env.NEXT_PUBLIC_API_URL ?? 'https://vela-engine.fly.dev'

export default function RegisterMMPage() {
  const { address, isConnected, connect } = useAuth()
  const [displayName, setDisplayName] = useState('')
  const [status, setStatus] = useState<'idle' | 'signing' | 'submitting' | 'success' | 'error'>('idle')
  const [errorMsg, setErrorMsg] = useState('')

  async function handleRegister() {
    if (!address) return
    setStatus('signing')
    setErrorMsg('')
    try {
      const nonce = Date.now()
      const message = `vela:mm-register:${address.toLowerCase()}:${nonce}`
      const msgHex = '0x' + Array.from(new TextEncoder().encode(message))
        .map((b: number) => b.toString(16).padStart(2, '0'))
        .join('')

      const ethereum = (window as Window & typeof globalThis & { ethereum?: { request: (args: { method: string; params: string[] }) => Promise<unknown> } }).ethereum
      if (!ethereum) throw new Error('No wallet detected')
      const signature = String(await ethereum.request({
        method: 'personal_sign',
        params: [msgHex, address],
      }))

      setStatus('submitting')
      const res = await fetch(`${API_URL}/market-makers/register`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          address,
          display_name: displayName.trim() || null,
          signature,
          nonce,
        }),
      })
      const json = await res.json()
      if (json.ok) {
        setStatus('success')
      } else {
        setErrorMsg(json.error ?? 'Registration failed.')
        setStatus('error')
      }
    } catch (e) {
      setErrorMsg(e instanceof Error ? e.message : 'Unknown error')
      setStatus('error')
    }
  }

  return (
    <div style={{ background: '#0C0C0C', minHeight: '100vh' }}>
      <div style={{ position: 'relative', background: '#0C0C0C', overflow: 'hidden' }} className="px-6 pt-12 pb-12 lg:px-[52px] lg:pt-[60px] lg:pb-[48px]">
        <HexCanvas />
        <div style={{ position: 'relative', zIndex: 1 }}>
          <p style={{ fontFamily: IN, fontSize: '10px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', marginBottom: '16px' }}>
            Vela Exchange — Market Makers
          </p>
          <span style={{ fontFamily: PF, fontWeight: 700, fontSize: '40px', color: '#e9eaf0', display: 'block', lineHeight: 1.1 }}>
            Register as a Market Maker
          </span>
          <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '14px', lineHeight: 1.75, color: 'rgba(232,234,240,0.4)', maxWidth: '480px', marginTop: '16px' }}>
            Add your wallet to Vela&apos;s public market maker disclosure list.
          </p>
        </div>
      </div>

      <div style={{ background: '#090b11' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        {status === 'success' ? (
          <div style={{ maxWidth: '480px' }}>
            <div style={{ background: 'rgba(107,138,90,0.08)', border: '1px solid rgba(107,138,90,0.25)', borderLeft: '3px solid #20c98a', padding: '20px 24px' }}>
              <p style={{ fontFamily: IN, fontWeight: 600, fontSize: '14px', color: '#e9eaf0', margin: '0 0 8px' }}>Registered.</p>
              <p style={{ fontFamily: IN, fontSize: '13px', color: 'rgba(232,234,240,0.6)', margin: 0 }}>
                You will appear on{' '}
                <a href="/operator" style={{ color: '#e9eaf0' }}>dippi.fun/operator</a>
              </p>
            </div>
          </div>
        ) : (
          <div style={{ maxWidth: '480px', display: 'flex', flexDirection: 'column', gap: '20px' }}>
            <div>
              <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.35)', margin: '0 0 8px' }}>
                Wallet Address
              </p>
              {isConnected && address ? (
                <div style={{ background: '#0C0C0C', padding: '12px 16px' }}>
                  <span style={{ fontFamily: CN, fontSize: '12px', color: '#e9eaf0' }}>{address}</span>
                </div>
              ) : (
                <button
                  onClick={() => connect()}
                  style={{ fontFamily: IN, fontWeight: 600, fontSize: '13px', color: '#e9eaf0', background: '#0C0C0C', padding: '12px 24px', border: 'none', cursor: 'pointer', borderRadius: 0 }}
                >
                  Connect wallet to continue
                </button>
              )}
            </div>

            {isConnected && (
              <>
                <div>
                  <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.35)', margin: '0 0 8px' }}>
                    Display Name (optional)
                  </p>
                  <input
                    type="text"
                    value={displayName}
                    onChange={e => setDisplayName(e.target.value.slice(0, 64))}
                    placeholder="Your firm name (optional)"
                    maxLength={64}
                    style={{
                      width: '100%',
                      fontFamily: IN,
                      fontSize: '13px',
                      color: '#e9eaf0',
                      background: '#0d1018',
                      border: '1px solid rgba(232,234,240,0.15)',
                      padding: '12px 14px',
                      outline: 'none',
                      borderRadius: 0,
                      boxSizing: 'border-box',
                    }}
                  />
                  <p style={{ fontFamily: IN, fontSize: '10px', color: 'rgba(232,234,240,0.35)', margin: '4px 0 0' }}>
                    {displayName.length}/64
                  </p>
                </div>

                <div style={{ background: 'rgba(232,234,240,0.04)', border: '1px solid rgba(232,234,240,0.08)', padding: '14px 16px' }}>
                  <p style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.5)', lineHeight: 1.7, margin: 0 }}>
                    Your wallet address will always be shown. Display name is optional. Registration requires a wallet signature.
                  </p>
                </div>

                {status === 'error' && (
                  <p style={{ fontFamily: IN, fontSize: '12px', color: '#ef6a5b', margin: 0 }}>{errorMsg}</p>
                )}

                <button
                  onClick={handleRegister}
                  disabled={status === 'signing' || status === 'submitting'}
                  style={{
                    fontFamily: IN,
                    fontWeight: 600,
                    fontSize: '12px',
                    textTransform: 'uppercase',
                    letterSpacing: '0.15em',
                    color: '#e9eaf0',
                    background: '#0C0C0C',
                    padding: '14px 28px',
                    border: 'none',
                    cursor: status === 'signing' || status === 'submitting' ? 'wait' : 'pointer',
                    borderRadius: 0,
                    opacity: status === 'signing' || status === 'submitting' ? 0.6 : 1,
                    alignSelf: 'flex-start',
                  }}
                >
                  {status === 'signing' ? 'Waiting for signature…' : status === 'submitting' ? 'Registering…' : 'Register'}
                </button>
              </>
            )}
          </div>
        )}
      </div>
    </div>
  )
}
