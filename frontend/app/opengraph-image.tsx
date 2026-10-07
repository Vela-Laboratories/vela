import { ImageResponse } from 'next/og'

export const alt = 'dippi.fun — Dip in. Have fun. Sepolia testnet only.'
export const size = { width: 1200, height: 630 }
export const contentType = 'image/png'

export default function Image() {
  return new ImageResponse(
    <div style={{ width: '100%', height: '100%', display: 'flex', alignItems: 'center', padding: 80, background: '#ffe6a0', color: '#2d1c43', fontFamily: 'sans-serif' }}>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 28, flex: 1 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 16, color: '#5930a5', fontSize: 40, fontWeight: 800 }}><span style={{ fontSize: 46 }}>😁</span>dippi.fun</div>
        <div style={{ display: 'flex', flexDirection: 'column', fontSize: 102, fontWeight: 800, letterSpacing: -6, lineHeight: 1.02 }}><span>Dip in.</span><span>Have fun.</span></div>
        <div style={{ fontSize: 23, color: '#503e5e' }}>Explore crypto markets on the Sepolia testnet.</div>
      </div>
      <div style={{ width: 350, height: 350, display: 'flex', alignItems: 'center', justifyContent: 'center', background: '#e4cffb', border: '2px solid #aa87c9', borderRadius: 30, fontSize: 235, transform: 'rotate(-6deg)' }}>😁</div>
    </div>,
    size,
  )
}
