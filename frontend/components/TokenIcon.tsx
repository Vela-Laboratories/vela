const TOKEN_STYLES: Record<string, { background: string; color?: string }> = {
  BTC: { background: '#f29b38' },
  ETH: { background: '#6976d9' },
  SOL: { background: '#7057c9' },
  AVAX: { background: '#d84d55' },
  LINK: { background: '#375fd2' },
  ARB: { background: '#3979a9' },
  OP: { background: '#d93d4d' },
  UNI: { background: '#d94f9f' },
  AAVE: { background: '#5965a9' },
  MATIC: { background: '#7656c5' },
  DOGE: { background: '#a88c3c' },
  PEPE: { background: '#4f8654' },
  WIF: { background: '#8d6748' },
  JUP: { background: '#447f78' },
  PENDLE: { background: '#337f78' },
  EIGEN: { background: '#5c53a1' },
  USDC: { background: '#327db4' },
}

const TOKEN_GLYPHS: Record<string, string> = {
  BTC: '₿', ETH: '◆', SOL: 'S', AVAX: 'A', LINK: 'L', ARB: 'A', OP: 'OP',
  UNI: 'U', AAVE: 'A', MATIC: 'M', DOGE: 'Ð', PEPE: 'P', WIF: 'W',
  JUP: 'J', PENDLE: 'P', EIGEN: 'E', USDC: '$',
}

export default function TokenIcon({ ticker, size = 'md' }: { ticker: string; size?: 'sm' | 'md' | 'lg' }) {
  const symbol = ticker.toUpperCase()
  const style = TOKEN_STYLES[symbol] ?? { background: '#596174' }

  return (
    <span className={`token-icon token-icon--${size}`} style={style} aria-hidden="true">
      {TOKEN_GLYPHS[symbol] ?? symbol.slice(0, 1)}
    </span>
  )
}
