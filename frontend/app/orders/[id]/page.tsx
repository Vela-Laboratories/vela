'use client'

import { useState, useEffect } from 'react'
import { useParams } from 'next/navigation'
import HexCanvas from '@/components/HexCanvas'
import Skeleton from '@/components/ui/Skeleton'

const PF = "var(--font-inter-sans), 'Inter', sans-serif"
const IN = 'Inter, sans-serif'
const CN = "'Courier New', monospace"

const API_URL = process.env.NEXT_PUBLIC_API_URL ?? 'https://vela-engine.fly.dev'

interface OrderFillRecord {
  fill_id: string
  counterparty_order_id: number
  counterparty_address: string
  price: number
  quantity: number
  timestamp: number
}

interface StoredOrder {
  id: number
  market_id: string
  user: string
  side: string
  price: number
  quantity: number
  filled_quantity: number
  status: string
  order_type: string
  time_in_force: string
  nonce: number
  signature: string
  created_at: number
  updated_at: number
  fills: OrderFillRecord[]
}

function formatPrice(price: number): string {
  if (price >= 1000) return price.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 })
  if (price >= 1) return price.toFixed(2)
  return price.toFixed(4)
}

function formatTimestamp(ts: number): string {
  const d = new Date(ts / 1000)
  return d.toLocaleString('en-US', {
    month: 'short',
    day: 'numeric',
    year: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    timeZone: 'UTC',
    timeZoneName: 'short',
  })
}

function truncateAddress(addr: string): string {
  if (!addr || addr.length < 12) return addr
  return `${addr.slice(0, 6)}...${addr.slice(-4)}`
}

function StatusBadge({ status }: { status: string }) {
  const map: Record<string, { bg: string; color: string; border: string; label: string }> = {
    filled: { bg: 'rgba(107,138,90,0.15)', color: '#20c98a', border: 'rgba(107,138,90,0.3)', label: 'FILLED' },
    open: { bg: 'rgba(232,234,240,0.1)', color: 'rgba(232,234,240,0.5)', border: 'rgba(232,234,240,0.2)', label: 'OPEN' },
    partially_filled: { bg: 'rgba(200,170,80,0.1)', color: 'rgba(200,170,80,0.8)', border: 'rgba(200,170,80,0.25)', label: 'PARTIAL' },
    cancelled: { bg: 'rgba(204,51,51,0.1)', color: '#ef6a5b', border: 'rgba(204,51,51,0.3)', label: 'CANCELLED' },
    rejected: { bg: 'rgba(204,51,51,0.1)', color: '#ef6a5b', border: 'rgba(204,51,51,0.3)', label: 'REJECTED' },
  }
  const s = map[status] ?? { bg: 'rgba(232,234,240,0.07)', color: 'rgba(232,234,240,0.4)', border: 'rgba(232,234,240,0.15)', label: status.toUpperCase() }
  return (
    <span style={{ fontFamily: IN, fontSize: '9px', background: s.bg, color: s.color, border: `1px solid ${s.border}`, padding: '3px 10px', letterSpacing: '0.08em' }}>
      {s.label}
    </span>
  )
}

function DetailCard({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div style={{ padding: '20px 24px', background: 'rgba(232,234,240,0.04)', borderLeft: '2px solid rgba(232,234,240,0.08)' }}>
      <p style={{ fontFamily: IN, fontSize: '8px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.35)', margin: '0 0 6px' }}>{label}</p>
      {children}
    </div>
  )
}

export default function OrderAuditPage() {
  const params = useParams()
  const id = params.id as string
  const [order, setOrder] = useState<StoredOrder | null>(null)
  const [loading, setLoading] = useState(true)
  const [notFound, setNotFound] = useState(false)

  useEffect(() => {
    if (!id) return
    setLoading(true)
    fetch(`${API_URL}/orders/${id}`)
      .then((r) => r.json())
      .then((data) => {
        if (data.ok && data.data) {
          setOrder(data.data)
        } else {
          setNotFound(true)
        }
      })
      .catch(() => setNotFound(true))
      .finally(() => setLoading(false))
  }, [id])

  if (loading) {
    return (
      <div style={{ background: '#15131f', minHeight: '100vh' }}>
        <div style={{ position: 'relative', background: '#15131f', overflow: 'hidden' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
          <HexCanvas />
          <div style={{ position: 'relative', zIndex: 1 }}>
            <Skeleton className="w-24 h-3 mb-6" />
            <Skeleton className="w-48 h-5" />
          </div>
        </div>
        <div style={{ background: '#13111d' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
            {Array.from({ length: 9 }).map((_, i) => (
              <div key={i} style={{ padding: '20px 24px', background: 'rgba(232,234,240,0.04)', borderLeft: '2px solid rgba(232,234,240,0.08)' }}>
                <Skeleton className="w-16 h-2" />
                <Skeleton className="w-24 h-4 mt-2" />
              </div>
            ))}
          </div>
        </div>
      </div>
    )
  }

  if (notFound || !order) {
    return (
      <div style={{ background: '#15131f', minHeight: '100vh', display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', gap: '12px' }}>
        <p style={{ fontFamily: PF, fontStyle: 'italic', fontSize: '24px', color: 'rgba(232,234,240,0.5)', margin: 0 }}>Order not found.</p>
        <p style={{ fontFamily: IN, fontSize: '13px', color: 'rgba(232,234,240,0.25)', margin: 0 }}>This order ID doesn&apos;t exist or hasn&apos;t been indexed yet.</p>
        <a href="/transparency" style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.3)', textDecoration: 'underline', marginTop: '8px' }}>← Back to transparency</a>
      </div>
    )
  }

  const priceNum = order.price / 1e8
  const qtyNum = order.quantity / 1e8
  const filledNum = order.filled_quantity / 1e8
  const filledPct = qtyNum > 0 ? ((filledNum / qtyNum) * 100).toFixed(1) : '0.0'
  const msgSigned = `vela:order:${order.market_id}:${order.side}:${order.price}:${order.quantity}:${order.nonce}`

  return (
    <div style={{ background: '#15131f', minHeight: '100vh' }}>
      <div style={{ position: 'relative', background: '#15131f', overflow: 'hidden' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <HexCanvas />
        <div style={{ position: 'relative', zIndex: 1 }}>
          <a href="/transparency" style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.3)', textDecoration: 'none', display: 'inline-block', marginBottom: '24px' }}>
            ← Back to transparency
          </a>
          <p style={{ fontFamily: IN, fontSize: '10px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', margin: '0 0 12px' }}>
            Order Audit Trail
          </p>
          <div style={{ display: 'flex', alignItems: 'center', gap: '16px', flexWrap: 'wrap' }}>
            <span style={{ fontFamily: CN, fontWeight: 700, fontSize: '18px', color: '#f8f5ee' }}>
              Order #{order.id}
            </span>
            <StatusBadge status={order.status} />
          </div>
        </div>
      </div>

      <div style={{ background: '#13111d' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
          <DetailCard label="Market">
            <span style={{ fontFamily: IN, fontWeight: 600, fontSize: '16px', color: '#f8f5ee' }}>{order.market_id}</span>
          </DetailCard>
          <DetailCard label="Side">
            <span style={{ fontFamily: IN, fontWeight: 600, fontSize: '16px', color: order.side === 'bid' ? '#20c98a' : '#ef6a5b' }}>
              {order.side === 'bid' ? 'BUY' : 'SELL'}
            </span>
          </DetailCard>
          <DetailCard label="Type">
            <span style={{ fontFamily: IN, fontSize: '14px', color: '#f8f5ee' }}>
              {order.order_type.toUpperCase()} {order.time_in_force.toUpperCase()}
            </span>
          </DetailCard>
          <DetailCard label="Price">
            <span style={{ fontFamily: CN, fontSize: '16px', color: '#f8f5ee' }}>{formatPrice(priceNum)}</span>
          </DetailCard>
          <DetailCard label="Size">
            <span style={{ fontFamily: CN, fontSize: '16px', color: '#f8f5ee' }}>{qtyNum.toFixed(6)}</span>
          </DetailCard>
          <DetailCard label="Filled">
            <span style={{ fontFamily: CN, fontSize: '14px', color: '#f8f5ee' }}>
              {filledNum.toFixed(6)} / {qtyNum.toFixed(6)}
              <span style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.4)', marginLeft: '8px' }}>{filledPct}%</span>
            </span>
          </DetailCard>
          <DetailCard label="Submitted">
            <span style={{ fontFamily: CN, fontSize: '12px', color: '#f8f5ee' }}>{formatTimestamp(order.created_at)}</span>
          </DetailCard>
          <DetailCard label="Wallet">
            <span title={order.user} style={{ fontFamily: CN, fontSize: '12px', color: '#f8f5ee' }}>{truncateAddress(order.user)}</span>
          </DetailCard>
          <DetailCard label="Nonce">
            <span style={{ fontFamily: CN, fontSize: '12px', color: 'rgba(232,234,240,0.5)' }}>{order.nonce}</span>
          </DetailCard>
        </div>
      </div>

      <div style={{ position: 'relative', background: '#15131f', overflow: 'hidden' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <HexCanvas />
        <div style={{ position: 'relative', zIndex: 1 }}>
          <h2 style={{ fontFamily: PF, fontWeight: 700, fontSize: '24px', color: '#f8f5ee', margin: '0 0 10px' }}>
            Cryptographic proof of intent
          </h2>
          <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '13px', color: 'rgba(232,234,240,0.38)', maxWidth: '560px', marginBottom: '28px' }}>
            This order was signed by wallet {truncateAddress(order.user)} before submission. The signature proves the wallet owner authorized this exact order.
          </p>

          <div style={{ marginBottom: '20px' }}>
            <p style={{ fontFamily: IN, fontSize: '8px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.25)', margin: '0 0 8px' }}>
              Wallet Signature
            </p>
            <div style={{ fontFamily: CN, fontSize: '10px', color: 'rgba(232,234,240,0.5)', wordBreak: 'break-all', background: 'rgba(232,234,240,0.03)', padding: '12px 16px', border: '1px solid rgba(232,234,240,0.07)' }}>
              {order.signature || '(signature not captured)'}
            </div>
          </div>

          <div>
            <p style={{ fontFamily: IN, fontSize: '8px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.25)', margin: '0 0 8px' }}>
              Message Signed
            </p>
            <div style={{ fontFamily: CN, fontSize: '10px', color: 'rgba(232,234,240,0.5)', wordBreak: 'break-all', background: 'rgba(232,234,240,0.03)', padding: '12px 16px', border: '1px solid rgba(232,234,240,0.07)' }}>
              {msgSigned}
            </div>
          </div>
        </div>
      </div>

      <div style={{ background: '#13111d' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <h2 style={{ fontFamily: PF, fontWeight: 700, fontSize: '24px', color: '#f8f5ee', margin: '0 0 20px' }}>
          Fill history
        </h2>

        {order.fills.length === 0 ? (
          <p style={{ fontFamily: IN, fontSize: '13px', color: 'rgba(232,234,240,0.35)' }}>This order has not been filled yet.</p>
        ) : (
          <div>
            <div style={{ display: 'grid', gridTemplateColumns: '1.5fr 1fr 1fr 1fr 1fr', padding: '8px 0', borderBottom: '1px solid rgba(232,234,240,0.1)' }}>
              {['Fill ID', 'Counterparty', 'Price', 'Size', 'Time'].map((h) => (
                <span key={h} style={{ fontFamily: IN, fontSize: '8px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.3)' }}>{h}</span>
              ))}
            </div>
            {order.fills.map((fill) => (
              <div key={fill.fill_id} style={{ display: 'grid', gridTemplateColumns: '1.5fr 1fr 1fr 1fr 1fr', padding: '10px 0', borderBottom: '1px solid rgba(232,234,240,0.06)', alignItems: 'center' }}>
                <span style={{ fontFamily: CN, fontSize: '10px', color: 'rgba(232,234,240,0.45)' }}>{fill.fill_id}</span>
                <a
                  href={`/orders/${fill.counterparty_order_id}`}
                  style={{ fontFamily: CN, fontSize: '10px', color: 'rgba(232,234,240,0.6)', textDecoration: 'underline' }}
                >
                  {truncateAddress(fill.counterparty_address)}
                </a>
                <span style={{ fontFamily: CN, fontSize: '11px', color: '#f8f5ee' }}>{formatPrice(fill.price / 1e8)}</span>
                <span style={{ fontFamily: CN, fontSize: '10px', color: 'rgba(232,234,240,0.6)' }}>{(fill.quantity / 1e8).toFixed(6)}</span>
                <span style={{ fontFamily: CN, fontSize: '10px', color: 'rgba(232,234,240,0.4)' }}>{new Date(fill.timestamp / 1000).toLocaleTimeString()}</span>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}
