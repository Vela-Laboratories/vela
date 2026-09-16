'use client'

import { useState, useEffect } from 'react'
import { motion } from 'framer-motion'

const STATUS_API_URL = process.env.NEXT_PUBLIC_API_URL ?? 'https://vela-engine.fly.dev'

type EngineStatus = 'operational' | 'degraded' | 'starting' | null

export default function BetaBanner() {
  const [engineStatus, setEngineStatus] = useState<EngineStatus>(null)

  useEffect(() => {
    let mounted = true

    async function fetchEngineStatus() {
      try {
        const res = await fetch(`${STATUS_API_URL}/status`)
        const data = await res.json()
        if (mounted && data.ok) setEngineStatus(data.data.status)
      } catch {
        // silently ignore
      }
    }

    fetchEngineStatus()
    const t = setInterval(fetchEngineStatus, 60_000)
    return () => {
      mounted = false
      clearInterval(t)
    }
  }, [])

  const dotColor = engineStatus === 'operational'
    ? '#6B8A5A'
    : engineStatus === 'degraded'
    ? '#CC3333'
    : 'rgba(232,228,216,0.5)'

  const statusText = engineStatus === 'degraded' ? 'DEGRADED' : 'PUBLIC BETA'

  return (
    <div className="beta-banner">
      <span style={{ display: 'flex', alignItems: 'center', gap: 5, color: engineStatus === 'degraded' ? '#ff5c7c' : '#8c99b3' }}>
        {engineStatus === 'starting' ? (
          <motion.span
            style={{ color: dotColor }}
            animate={{ opacity: [1, 0.15, 1] }}
            transition={{ duration: 2, repeat: Infinity, ease: 'easeInOut' }}
          >
            ●
          </motion.span>
        ) : engineStatus === null ? (
          <motion.span
            style={{ color: '#8c99b3' }}
            animate={{ opacity: [1, 0.15, 1] }}
            transition={{ duration: 2, repeat: Infinity, ease: 'easeInOut' }}
          >
            ●
          </motion.span>
        ) : (
          <span style={{ color: dotColor }}>●</span>
        )}
        {statusText}
      </span>
      <span className="beta-banner__detail">
        SEPOLIA TESTNET · NO REAL FUNDS · NOT PRODUCTION CUSTODY
      </span>
      <a
        href="https://monolithsystematicllc.mintlify.app/introduction/what-is-vela"
        target="_blank"
        rel="noopener noreferrer"
        className="beta-banner__link"
      >
        Learn more →
      </a>
    </div>
  )
}
