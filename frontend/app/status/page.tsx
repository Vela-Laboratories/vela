'use client'

import { useState, useEffect, useCallback, useRef } from 'react'
import HexCanvas from '@/components/HexCanvas'
import Skeleton from '@/components/ui/Skeleton'

const PF = "var(--font-inter-sans), 'Inter', sans-serif"
const IN = 'Inter, sans-serif'
const CN = "'Courier New', monospace"

const API_URL = process.env.NEXT_PUBLIC_API_URL ?? 'https://vela-engine.fly.dev'

interface Incident {
  id: number
  incident_type: string
  started_at: number
  resolved_at: number | null
  description: string
  impact: string
}

interface IncidentData {
  incidents: Incident[]
  total: number
  all_clear: boolean
}

interface StatusData {
  status: 'operational' | 'degraded' | 'starting'
  engine_uptime_seconds: number
  engine_version: string
  last_snapshot_timestamp: number
  last_state_root: string
  orders_processed_today: number
  fills_today: number
  volume_today_usdc: string
  active_markets: number
  connected_ws_clients: number
  last_restart_reason: string | null
}

interface WalStatsData {
  current_sequence: number
  current_segment: string
  segment_size_bytes: number
  last_checkpoint_sequence: number
  last_checkpoint_time: number
  entries_since_checkpoint: number
  last_engine_start_reason: string
  wal_enabled: boolean
}

function formatUptime(secs: number): string {
  if (secs < 60) return `${secs}s`
  if (secs < 3600) return `${Math.floor(secs / 60)}m ${secs % 60}s`
  if (secs < 86400) return `${Math.floor(secs / 3600)}h ${Math.floor((secs % 3600) / 60)}m`
  const d = Math.floor(secs / 86400)
  const h = Math.floor((secs % 86400) / 3600)
  return `${d}d ${h}h`
}

function formatIncidentDate(tsMs: number): string {
  const d = new Date(tsMs)
  const months = ['Jan','Feb','Mar','Apr','May','Jun','Jul','Aug','Sep','Oct','Nov','Dec']
  const h = d.getHours().toString().padStart(2, '0')
  const m = d.getMinutes().toString().padStart(2, '0')
  return `${months[d.getMonth()]} ${d.getDate()}, ${d.getFullYear()} ${h}:${m}`
}

function formatResolution(startMs: number, endMs: number): string {
  const diffSecs = Math.floor((endMs - startMs) / 1000)
  if (diffSecs < 60) return `Resolved in ${diffSecs}s`
  const mins = Math.floor(diffSecs / 60)
  const secs = diffSecs % 60
  return `Resolved in ${mins}m ${secs}s`
}

function formatRelativeTime(tsMs: number): string {
  if (!tsMs) return 'Never'
  const diff = Math.floor((Date.now() - tsMs) / 1000)
  if (diff < 5) return 'Just now'
  if (diff < 60) return `${diff} seconds ago`
  if (diff < 3600) return `${Math.floor(diff / 60)} minutes ago`
  if (diff < 86400) return `${Math.floor(diff / 3600)} hours ago`
  return `${Math.floor(diff / 86400)} days ago`
}

export default function StatusPage() {
  const [data, setData] = useState<StatusData | null>(null)
  const [loading, setLoading] = useState(true)
  const [lastUpdated, setLastUpdated] = useState<Date | null>(null)
  const [incidentData, setIncidentData] = useState<IncidentData | null>(null)
  const [walStats, setWalStats] = useState<WalStatsData | null>(null)
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null)
  const incidentIntervalRef = useRef<ReturnType<typeof setInterval> | null>(null)

  const fetchStatus = useCallback(async () => {
    try {
      const [statusRes, walRes] = await Promise.all([
        fetch(`${API_URL}/status`),
        fetch(`${API_URL}/wal/stats`),
      ])
      const [statusJson, walJson] = await Promise.all([statusRes.json(), walRes.json()])
      if (statusJson.ok) {
        setData(statusJson.data)
        setLastUpdated(new Date())
      }
      if (walJson.ok) {
        setWalStats(walJson.data)
      }
    } catch {
      // silently ignore
    } finally {
      setLoading(false)
    }
  }, [])

  const fetchIncidents = useCallback(async () => {
    try {
      const res = await fetch(`${API_URL}/incidents`)
      const json = await res.json()
      if (json.ok) setIncidentData(json.data)
    } catch {
      // silently ignore
    }
  }, [])

  useEffect(() => {
    fetchStatus()
    intervalRef.current = setInterval(fetchStatus, 30_000)
    return () => { if (intervalRef.current) clearInterval(intervalRef.current) }
  }, [fetchStatus])

  useEffect(() => {
    fetchIncidents()
    incidentIntervalRef.current = setInterval(fetchIncidents, 60_000)
    return () => { if (incidentIntervalRef.current) clearInterval(incidentIntervalRef.current) }
  }, [fetchIncidents])

  const walDegraded = walStats != null && walStats.entries_since_checkpoint > 5000
  const effectiveStatus = walDegraded && data?.status === 'operational' ? 'degraded' : (data?.status ?? null)

  const dotColor = effectiveStatus === 'operational'
    ? '#20c98a'
    : effectiveStatus === 'degraded'
    ? '#ef6a5b'
    : 'rgba(232,234,240,0.4)'

  const statusLabel = effectiveStatus === 'operational'
    ? 'OPERATIONAL'
    : effectiveStatus === 'degraded'
    ? 'DEGRADED'
    : 'STARTING'

  const statusColor = effectiveStatus === 'operational'
    ? '#20c98a'
    : effectiveStatus === 'degraded'
    ? '#ef6a5b'
    : 'rgba(232,234,240,0.4)'

  const walSeqColor = '#f8f5ee'
  const walEntriesColor = walStats == null
    ? 'rgba(232,234,240,0.6)'
    : walStats.entries_since_checkpoint > 5000
    ? '#ef6a5b'
    : walStats.entries_since_checkpoint > 1000
    ? 'rgba(180,140,60,0.8)'
    : 'rgba(232,234,240,0.6)'

  const metrics = data ? [
    { label: 'ENGINE UPTIME', value: formatUptime(data.engine_uptime_seconds) },
    { label: 'ORDERS TODAY', value: data.orders_processed_today.toLocaleString() },
    { label: 'FILLS TODAY', value: data.fills_today.toString() },
    { label: 'VOLUME TODAY', value: `$${parseFloat(data.volume_today_usdc).toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 })} USDC` },
    { label: 'ACTIVE MARKETS', value: data.active_markets.toString() },
    { label: 'ENGINE VERSION', value: `v${data.engine_version}` },
    { label: 'LAST SNAPSHOT', value: formatRelativeTime(data.last_snapshot_timestamp) },
    { label: 'WS CLIENTS', value: data.connected_ws_clients.toString() },
  ] : []

  return (
    <div style={{ background: '#15131f', minHeight: '100vh' }}>
      <div style={{ position: 'relative', background: '#15131f', overflow: 'hidden' }} className="px-6 pt-12 pb-10 lg:px-[52px] lg:pt-[60px] lg:pb-[40px]">
        <HexCanvas />
        <div style={{ position: 'relative', zIndex: 1 }}>
          <p style={{ fontFamily: IN, fontSize: '10px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', marginBottom: '16px' }}>
            Vela — System Status
          </p>
          <div>
            <span style={{ fontFamily: PF, fontWeight: 900, fontSize: '52px', color: '#f8f5ee', display: 'block', lineHeight: 1.1 }}>
              Status
            </span>
            <span style={{ fontFamily: PF, fontWeight: 400, fontStyle: 'italic', fontSize: '52px', color: 'rgba(232,234,240,0.3)', display: 'block', lineHeight: 1.1 }}>
              Always published.
            </span>
          </div>
          <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '14px', lineHeight: 1.75, color: 'rgba(232,234,240,0.4)', maxWidth: '520px', marginTop: '20px' }}>
            Engine health, uptime, and incident history. Published in real time. Exchanges hide downtime. Vela doesn&apos;t.
          </p>
        </div>
      </div>

      <div style={{ background: '#13111d' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', marginBottom: '40px' }}>
          {loading ? (
            <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: '12px' }}>
              <Skeleton className="w-4 h-4 rounded-full" />
              <Skeleton className="w-32 h-5" />
            </div>
          ) : (
            <div style={{ display: 'flex', alignItems: 'center', gap: '12px' }}>
              <div style={{
                width: '16px',
                height: '16px',
                background: dotColor,
                borderRadius: '50%',
                boxShadow: status === 'operational' ? '0 0 12px rgba(107,138,90,0.4)' : 'none',
                animation: status === 'starting' ? 'pulse 2s infinite' : 'none',
              }} />
              <span style={{
                fontFamily: IN,
                fontWeight: 700,
                fontSize: '18px',
                textTransform: 'uppercase',
                letterSpacing: '0.2em',
                color: statusColor,
              }}>
                {statusLabel}
              </span>
            </div>
          )}
        </div>

        {loading ? (
          <div className="grid grid-cols-2 lg:grid-cols-4 gap-[1px]" style={{ background: 'rgba(232,234,240,0.05)' }}>
            {Array.from({ length: 8 }).map((_, i) => (
              <div key={i} style={{ background: '#13111d', padding: '24px', borderLeft: '2px solid rgba(232,234,240,0.08)' }}>
                <Skeleton className="w-24 h-2 mb-3" />
                <Skeleton className="w-16 h-8" />
              </div>
            ))}
          </div>
        ) : data ? (
          <div className="grid grid-cols-2 lg:grid-cols-4 gap-[1px]" style={{ background: 'rgba(232,234,240,0.05)' }}>
            {metrics.map((m) => (
              <div key={m.label} style={{ background: 'rgba(232,234,240,0.04)', padding: '24px', borderLeft: '2px solid rgba(232,234,240,0.08)' }}>
                <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.35)', margin: '0 0 10px' }}>
                  {m.label}
                </p>
                <p style={{ fontFamily: IN, fontWeight: 600, fontSize: '20px', color: '#f8f5ee', margin: 0 }}>
                  {m.value}
                </p>
              </div>
            ))}
            <div style={{ background: 'rgba(232,234,240,0.04)', padding: '24px', borderLeft: '2px solid rgba(232,234,240,0.08)' }}>
              <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.35)', margin: '0 0 10px' }}>
                WAL SEQUENCE
              </p>
              <p style={{ fontFamily: CN, fontWeight: 600, fontSize: '14px', color: walSeqColor, margin: 0 }}>
                {walStats != null ? walStats.current_sequence.toLocaleString() : '—'}
              </p>
            </div>
            <div style={{ background: 'rgba(232,234,240,0.04)', padding: '24px', borderLeft: '2px solid rgba(232,234,240,0.08)' }}>
              <p style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.35)', margin: '0 0 10px' }}>
                ENTRIES SINCE CHECKPOINT
              </p>
              <p style={{ fontFamily: CN, fontWeight: 600, fontSize: '14px', color: walEntriesColor, margin: 0 }}>
                {walStats != null ? walStats.entries_since_checkpoint.toLocaleString() : '—'}
              </p>
            </div>
          </div>
        ) : (
          <p style={{ fontFamily: IN, fontSize: '13px', color: 'rgba(232,234,240,0.4)', textAlign: 'center', padding: '40px 0' }}>
            Unable to load status. The engine may be offline.
          </p>
        )}

        {lastUpdated && (
          <p style={{ fontFamily: IN, fontSize: '10px', color: 'rgba(232,234,240,0.35)', marginTop: '16px' }}>
            Updated {lastUpdated.toLocaleTimeString()} · Auto-refreshes every 30s
          </p>
        )}
      </div>

      <div style={{ position: 'relative', background: '#15131f', overflow: 'hidden' }} className="px-6 py-12 lg:px-[52px] lg:py-[52px]">
        <HexCanvas />
        <div style={{ position: 'relative', zIndex: 1 }}>
          <p style={{ fontFamily: IN, fontSize: '10px', textTransform: 'uppercase', letterSpacing: '0.2em', color: 'rgba(232,234,240,0.3)', marginBottom: '20px' }}>
            Incident History
          </p>
          <h2 style={{ fontFamily: PF, fontWeight: 700, fontSize: '32px', color: '#f8f5ee', margin: '0 0 16px' }}>
            The complete record.
          </h2>
          <p style={{ fontFamily: IN, fontWeight: 300, fontSize: '14px', lineHeight: 1.8, color: 'rgba(232,234,240,0.35)', maxWidth: '500px', marginBottom: '32px' }}>
            Every engine restart, snapshot restore, and degraded performance event is logged here permanently. This record starts from Vela&apos;s first deployment.
          </p>

          {incidentData === null ? (
            <div style={{ padding: '40px 0' }}>
              <Skeleton className="w-48 h-4" />
            </div>
          ) : incidentData.all_clear ? (
            <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: '8px', padding: '40px 0' }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                <div style={{ width: '8px', height: '8px', background: '#20c98a', borderRadius: '50%' }} />
                <span style={{ fontFamily: IN, fontWeight: 600, fontSize: '14px', color: '#20c98a' }}>All Systems Operational</span>
              </div>
              <span style={{ fontFamily: IN, fontSize: '12px', color: 'rgba(232,234,240,0.3)', marginTop: '8px' }}>
                No incidents recorded in the last 30 days.
              </span>
            </div>
          ) : (
            <div>
              <div style={{ display: 'grid', gridTemplateColumns: '140px 120px 1fr 1fr', gap: '0 16px', padding: '0 0 8px', borderBottom: '1px solid rgba(232,234,240,0.08)', marginBottom: '4px' }}>
                {['STARTED', 'TYPE', 'DESCRIPTION', 'IMPACT'].map(h => (
                  <span key={h} style={{ fontFamily: IN, fontSize: '8px', textTransform: 'uppercase', letterSpacing: '0.15em', color: 'rgba(232,234,240,0.25)' }}>{h}</span>
                ))}
              </div>
              {incidentData.incidents.map(inc => {
                const typeColor = inc.incident_type === 'RESTART'
                  ? { bg: 'rgba(204,51,51,0.1)', border: 'rgba(204,51,51,0.3)', text: 'rgba(204,51,51,0.7)' }
                  : { bg: 'rgba(180,140,60,0.1)', border: 'rgba(180,140,60,0.3)', text: 'rgba(180,140,60,0.7)' }
                return (
                  <div key={inc.id} style={{ display: 'grid', gridTemplateColumns: '140px 120px 1fr 1fr', gap: '0 16px', padding: '10px 0', borderBottom: '1px solid rgba(232,234,240,0.05)', alignItems: 'start' }}>
                    <span style={{ fontFamily: CN, fontSize: '10px', color: 'rgba(232,234,240,0.35)' }}>
                      {formatIncidentDate(inc.started_at)}
                    </span>
                    <span style={{ fontFamily: IN, fontSize: '9px', textTransform: 'uppercase', letterSpacing: '0.1em', color: typeColor.text, background: typeColor.bg, border: `1px solid ${typeColor.border}`, padding: '2px 6px', display: 'inline-block' }}>
                      {inc.incident_type.replace('_', ' ')}
                    </span>
                    <span style={{ fontFamily: IN, fontSize: '11px', color: 'rgba(232,234,240,0.7)', lineHeight: 1.5 }}>
                      {inc.description}
                    </span>
                    <span style={{ fontFamily: IN, fontSize: '11px', color: inc.resolved_at ? 'rgba(232,234,240,0.35)' : '#ef6a5b', lineHeight: 1.5 }}>
                      {inc.resolved_at
                        ? formatResolution(inc.started_at, inc.resolved_at)
                        : '● Ongoing'}
                    </span>
                  </div>
                )
              })}
            </div>
          )}
        </div>
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
