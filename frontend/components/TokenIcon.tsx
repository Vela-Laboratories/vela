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
  BTC: '₿', OP: 'OP', UNI: 'U', AAVE: 'A', MATIC: 'M', DOGE: 'Ð',
  PEPE: 'P', WIF: 'W', JUP: 'J', PENDLE: 'P', EIGEN: 'E', USDC: '$',
}

function TokenGlyph({ symbol }: { symbol: string }) {
  if (symbol === 'ETH') {
    return <svg className="token-icon__svg" viewBox="0 0 32 32"><path d="m16 3 8 13-8 4.7L8 16 16 3Z" fill="rgba(255,255,255,.94)"/><path d="m8 17.7 8 11.3 8-11.3-8 4.8-8-4.8Z" fill="rgba(255,255,255,.62)"/></svg>
  }
  if (symbol === 'SOL') {
    return <svg className="token-icon__svg" viewBox="0 0 32 32" fill="none"><path d="M8 7h17l-4 4H4l4-4Z" fill="#7fffd4"/><path d="M7 14h17l4 4H11l-4-4Z" fill="#d394ff"/><path d="M8 21h17l-4 4H4l4-4Z" fill="#7fffd4"/></svg>
  }
  if (symbol === 'AVAX') {
    return <svg className="token-icon__svg" viewBox="0 0 32 32"><path d="M15.8 5c1 0 1.5.6 2 1.5l8 14.2c.6 1 .1 2.3-1.1 2.3h-5.1c-.8 0-1.4-.4-1.8-1.1l-2.1-3.7-2.2 3.7c-.4.7-1 1.1-1.8 1.1H6.6c-1.2 0-1.7-1.3-1.1-2.3l8.2-14.2c.5-.9 1.1-1.5 2.1-1.5Z" fill="white"/></svg>
  }
  if (symbol === 'LINK') {
    return <svg className="token-icon__svg" viewBox="0 0 32 32"><path d="m16 5 9.5 5.5v11L16 27l-9.5-5.5v-11L16 5Z" fill="none" stroke="white" strokeWidth="3"/></svg>
  }
  if (symbol === 'ARB') {
    return <svg className="token-icon__svg" viewBox="0 0 32 32"><path d="m16 4 10 6v12l-10 6-10-6V10l10-6Z" fill="none" stroke="white" strokeWidth="2"/><path d="m10 22 7-13h3l-7 15-3-2Zm6 2 6-12 2 3-4 8-4 1Z" fill="white"/></svg>
  }
  return <span className="token-icon__letter">{TOKEN_GLYPHS[symbol] ?? symbol.slice(0, 1)}</span>
}

export default function TokenIcon({ ticker, size = 'md' }: { ticker: string; size?: 'sm' | 'md' | 'lg' }) {
  const symbol = ticker.toUpperCase()
  const style = TOKEN_STYLES[symbol] ?? { background: '#596174' }

  return (
    <span className={`token-icon token-icon--${size}`} style={style} aria-hidden="true">
      <TokenGlyph symbol={symbol} />
    </span>
  )
}
