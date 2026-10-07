export default function ComingSoon() {
  return (
    <main className="coming-soon">
      <video
        className="coming-soon__video"
        autoPlay
        loop
        muted
        playsInline
        preload="metadata"
        poster="/images/dippi-water.jpg"
        aria-hidden="true"
      >
        <source src="/video/dippi-waves-mobile.mp4" type="video/mp4" media="(max-width: 640px)" />
        <source src="/video/dippi-waves.mp4" type="video/mp4" />
      </video>

      <div className="coming-soon__content">
        <span className="coming-soon__smile" aria-hidden="true">😁</span>
        <h1>dippi.fun is coming soon</h1>
        <nav className="coming-soon__links" aria-label="Arya Somu social links">
          <a href="https://aryasomu.com/" target="_blank" rel="noopener noreferrer" aria-label="Arya Somu website">
            <span className="coming-soon__web-icon" aria-hidden="true">🌐</span>
            <span>Website</span>
          </a>
          <a href="https://x.com/arrrrrryaa" target="_blank" rel="noopener noreferrer" aria-label="Arya Somu on X">
            <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
              <path d="M18.244 2.25h3.308l-7.227 8.26 8.502 11.24H16.17l-5.214-6.817L4.99 21.75H1.68l7.73-8.835L1.254 2.25H8.08l4.713 6.231Zm-1.161 17.52h1.833L7.084 4.126H5.117Z" />
            </svg>
            <span>Twitter</span>
          </a>
        </nav>
      </div>

      <a className="coming-soon__credit" href="https://mixkit.co/free-stock-video/turquoise-ocean-background-with-foaming-waves-2091/" target="_blank" rel="noopener noreferrer">Footage: Mixkit</a>
    </main>
  )
}
