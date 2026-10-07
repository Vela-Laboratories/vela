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
        poster="/video/dippi-beach-poster.jpg"
        aria-hidden="true"
      >
        <source src="/video/dippi-beach-mobile.mp4" type="video/mp4" media="(max-width: 640px)" />
        <source src="/video/dippi-beach.mp4" type="video/mp4" />
      </video>

      <div className="coming-soon__content">
        <span className="coming-soon__smile" aria-hidden="true">😁</span>
        <h1>dippi.fun is coming soon</h1>
      </div>
    </main>
  )
}
