const TOKEN_STYLES: Record<string, { background: string; color?: string }> = {
  BTC: { background: 'linear-gradient(145deg, #ffb341, #f7931a)' },
  ETH: { background: 'linear-gradient(145deg, #8996ff, #627eea)' },
  SOL: { background: 'linear-gradient(145deg, #9945ff, #14f195)' },
  AVAX: { background: 'linear-gradient(145deg, #ff6b76, #e84142)' },
  LINK: { background: 'linear-gradient(145deg, #5d7cff, #2a5ada)' },
  ARB: { background: 'linear-gradient(145deg, #28a0f0, #163b67)' },
  OP: { background: 'linear-gradient(145deg, #ff5b65, #ff0420)' },
  UNI: { background: 'linear-gradient(145deg, #ff5fc8, #ff007a)' },
  AAVE: { background: 'linear-gradient(145deg, #b6509e, #2ebac6)' },
  MATIC: { background: 'linear-gradient(145deg, #a777ff, #8247e5)' },
  DOGE: { background: 'linear-gradient(145deg, #d8be61, #a68a2d)' },
  PEPE: { background: 'linear-gradient(145deg, #79c66d, #397640)' },
  WIF: { background: 'linear-gradient(145deg, #d9a777, #795336)' },
  JUP: { background: 'linear-gradient(145deg, #50e3c2, #7b61ff)' },
  PENDLE: { background: 'linear-gradient(145deg, #39d9c8, #0d716e)' },
  EIGEN: { background: 'linear-gradient(145deg, #9b8cff, #493db5)' },
  USDC: { background: 'linear-gradient(145deg, #4fa8e9, #2775ca)' },
}

export default function TokenIcon({ ticker, size = 'md' }: { ticker: string; size?: 'sm' | 'md' | 'lg' }) {
  const symbol = ticker.toUpperCase()
  const style = TOKEN_STYLES[symbol] ?? { background: 'linear-gradient(145deg, #7667ff, #32d6c5)' }

  return (
    <span className={`token-icon token-icon--${size}`} style={style} aria-hidden="true">
      {symbol.slice(0, symbol.length > 4 ? 1 : 2)}
    </span>
  )
}
