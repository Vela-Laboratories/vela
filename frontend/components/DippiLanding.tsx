import Image from 'next/image'
import Link from 'next/link'
import HomeMarkets from '@/components/HomeMarkets'

export default function HomePage() {
  return (
    <div className="dip-landing">
      <section className="dip-hero" aria-labelledby="dip-hero-title">
        <div className="dip-hero__copy">
          <span className="dip-eyebrow">THE FUN SIDE OF THE MARKET</span>
          <h1 id="dip-hero-title">go on.<br /><em>dip in.</em></h1>
          <p>See what&apos;s moving, follow your curiosity, and try a wallet-signed trade on Sepolia.</p>
          <div className="dip-hero__actions">
            <Link className="dip-button dip-button--sun" href="/markets">Explore markets <span aria-hidden="true">↗</span></Link>
            <Link className="dip-button dip-button--ghost" href="/transparency">How it works <span aria-hidden="true">→</span></Link>
          </div>
          <span className="dip-hero__note">Public beta · Testnet only · No real funds</span>
        </div>
        <div className="dip-hero__photo">
          <Image
            src="/images/dippi-dive.jpg"
            alt="A swimmer diving into a bright turquoise pool, photographed from above"
            fill
            priority
            sizes="(max-width: 900px) 100vw, 48vw"
          />
          <span className="dip-hero__photo-tag">TAKE THE PLUNGE / STAY CURIOUS</span>
        </div>
      </section>

      <section className="dip-intro" aria-labelledby="dip-intro-title">
        <span className="dip-kicker">WELCOME TO VELA</span>
        <div>
          <h2 id="dip-intro-title">A little curiosity<br />goes a long way.</h2>
          <p>Markets can be serious. Exploring them doesn&apos;t have to be. Vela gives you a place to look around, learn the rhythm, and try the mechanics on testnet.</p>
        </div>
      </section>

      <section className="dip-radar" aria-labelledby="dip-radar-title">
        <div className="dip-section-head">
          <div><span className="dip-kicker">THE MARKET MENU</span><h2 id="dip-radar-title">What&apos;s on the radar?</h2></div>
          <Link href="/markets">All markets <span aria-hidden="true">↗</span></Link>
        </div>
        <HomeMarkets />
        <p className="dip-radar__note">Prices shown are Binance Spot references. Vela orders use a separate synthetic Sepolia testnet book.</p>
      </section>

      <section className="dip-story" aria-labelledby="dip-story-title">
        <div className="dip-story__photo">
          <Image
            src="/images/dippi-together.jpg"
            alt="Two friends jumping together into a lake on a sunny day"
            fill
            sizes="(max-width: 900px) 100vw, 50vw"
          />
        </div>
        <div className="dip-story__copy">
          <span className="dip-kicker">COME AS YOU ARE</span>
          <h2 id="dip-story-title">Less pressure.<br /><em>More possibility.</em></h2>
          <p>From Bitcoin to the meme coins, pick a market and see where it takes you. The charts and spot price references are real. The trading environment is a Sepolia demo.</p>
          <Link href="/markets/BTC-USDC">Take a look at trading <span aria-hidden="true">↗</span></Link>
        </div>
      </section>

      <section className="dip-steps" aria-labelledby="dip-steps-title">
        <div className="dip-steps__inner">
          <div className="dip-section-head">
            <div><span className="dip-kicker">YOUR WAY IN</span><h2 id="dip-steps-title">Easy does it.</h2></div>
            <p>No jargon marathon. Just a few ways to get your bearings.</p>
          </div>
          <div className="dip-steps__grid">
            <Link href="/markets" className="dip-step"><span>01</span><h3>Find your market.</h3><p>Browse live spot price references and find a pair worth watching.</p><b>Explore markets ↗</b></Link>
            <Link href="/markets/BTC-USDC" className="dip-step"><span>02</span><h3>Follow the action.</h3><p>Open the chart and order book. Know what&apos;s real and what&apos;s demo.</p><b>Open trading ↗</b></Link>
            <Link href="/profile" className="dip-step"><span>03</span><h3>Make a move.</h3><p>Connect a wallet to try signed orders with testnet balances.</p><b>View your account ↗</b></Link>
          </div>
        </div>
      </section>

      <section className="dip-close" aria-labelledby="dip-close-title">
        <div className="dip-close__photo" aria-hidden="true">
          <Image src="/images/dippi-water.jpg" alt="" fill sizes="100vw" />
        </div>
        <div className="dip-close__content">
          <span className="dip-kicker">GOOD THINGS START WITH A DIP</span>
          <h2 id="dip-close-title">ready when<br />you are.</h2>
          <Link className="dip-button dip-button--sun" href="/markets">Dive into markets <span aria-hidden="true">↗</span></Link>
        </div>
      </section>

      <footer className="dip-footer">
        <div><strong>Vela Exchange</strong><span>Explore freely. Trade on testnet.</span></div>
        <nav aria-label="Footer"><Link href="/docs">Docs</Link><Link href="/status">Status</Link><Link href="/transparency">Transparency</Link></nav>
        <p>Photos: <a href="https://unsplash.com/photos/person-about-to-dive-on-swimming-pool-eQfMpQ3WcVY" target="_blank" rel="noopener noreferrer">Emilio Garcia</a>, <a href="https://unsplash.com/photos/two-friends-jump-into-the-lake-on-a-sunny-day-qPSpg2YHL9o" target="_blank" rel="noopener noreferrer">Bailey Burton</a>, and <a href="https://unsplash.com/photos/sunlight-reflects-on-the-surface-of-a-blue-swimming-pool-HltUVQgeaZ0" target="_blank" rel="noopener noreferrer">Cuvii</a> / Unsplash.</p>
      </footer>
    </div>
  )
}
