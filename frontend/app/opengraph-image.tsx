import { ImageResponse } from 'next/og'

export const alt = 'dippi.fun is coming soon'
export const size = { width: 1200, height: 630 }
export const contentType = 'image/png'

export default function Image() {
  return new ImageResponse(
    <div style={{ width: '100%', height: '100%', display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', gap: 36, padding: 80, background: 'linear-gradient(135deg, #176f82, #12333f 76%)', color: '#fff', fontFamily: 'sans-serif', textAlign: 'center' }}>
      <div style={{ fontSize: 72 }}>😁</div>
      <div style={{ fontSize: 86, fontWeight: 600, letterSpacing: -5, lineHeight: 1.06 }}>dippi.fun is coming soon</div>
    </div>,
    size,
  )
}
