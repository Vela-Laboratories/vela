'use client'

import HexCanvas from '@/components/HexCanvas'
import Link from 'next/link'

const PF = "var(--font-inter-sans), 'Inter', sans-serif"
const IN = 'Inter, sans-serif'

export default function NotFound() {
  return (
    <div style={{
      position: 'relative',
      width: '100%',
      height: '100vh',
      background: '#0C0C0C',
      overflow: 'hidden',
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
    }}>
      <HexCanvas />

      <div style={{
        position: 'relative',
        zIndex: 2,
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        gap: 24,
        textAlign: 'center',
        padding: 40,
      }}>
        <span style={{
          fontFamily: PF,
          fontWeight: 900,
          fontStyle: 'italic',
          fontSize: 120,
          color: 'rgba(232,234,240,0.06)',
          position: 'absolute',
          userSelect: 'none',
          lineHeight: 1,
        }}>
          404
        </span>

        <div style={{
          position: 'relative',
          zIndex: 3,
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          gap: 16,
        }}>
          <span style={{
            fontFamily: PF,
            fontWeight: 700,
            fontSize: 32,
            color: '#e9eaf0',
          }}>
            Page not found.
          </span>

          <p style={{
            fontFamily: IN,
            fontWeight: 300,
            fontSize: 14,
            color: 'rgba(232,234,240,0.38)',
            maxWidth: 320,
            lineHeight: 1.7,
            margin: 0,
          }}>
            The page you&apos;re looking for doesn&apos;t exist or has been moved.
          </p>

          <div style={{ display: 'flex', gap: 12, marginTop: 8 }}>
            <Link
              href="/"
              style={{
                background: '#090b11',
                color: '#e9eaf0',
                fontFamily: IN,
                fontWeight: 600,
                fontSize: 11,
                textTransform: 'uppercase',
                letterSpacing: '0.12em',
                padding: '12px 28px',
                borderRadius: 0,
                border: 'none',
                textDecoration: 'none',
                display: 'inline-block',
              }}
            >
              Go Home
            </Link>

            <Link
              href="/markets"
              style={{
                background: 'transparent',
                color: 'rgba(232,234,240,0.45)',
                border: '1px solid rgba(232,234,240,0.15)',
                fontFamily: IN,
                fontSize: 11,
                padding: '12px 24px',
                borderRadius: 0,
                textDecoration: 'none',
                display: 'inline-block',
              }}
            >
              View Markets
            </Link>
          </div>
        </div>
      </div>

      <span style={{
        position: 'absolute',
        bottom: 32,
        fontFamily: IN,
        fontSize: 10,
        color: 'rgba(232,234,240,0.15)',
        letterSpacing: '0.1em',
      }}>
        dippi.fun
      </span>
    </div>
  )
}
