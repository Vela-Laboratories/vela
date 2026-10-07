import type { Metadata } from 'next'
import { IBM_Plex_Mono, Playfair_Display, Inter, DM_Sans } from 'next/font/google'
import './globals.css'
import './dippi.css'
import './dippi-bright.css'
import './dippi-landing.css'
import './coming-soon.css'
import AppShell from '@/components/AppShell'

const ibmPlexMono = IBM_Plex_Mono({
  subsets: ['latin'],
  variable: '--font-mono',
  weight: ['400', '600', '700'],
  display: 'swap',
})

const playfairDisplay = Playfair_Display({
  subsets: ['latin'],
  weight: ['400', '700', '900'],
  style: ['normal', 'italic'],
  variable: '--font-playfair',
  display: 'swap',
})

const inter = Inter({
  subsets: ['latin'],
  weight: ['300', '400', '500', '600'],
  variable: '--font-inter-sans',
  display: 'swap',
})

const dmSans = DM_Sans({
  subsets: ['latin'],
  variable: '--font-dm-sans',
  display: 'swap',
})

export const metadata: Metadata = {
  metadataBase: new URL('https://dippi.fun'),
  title: { default: 'dippi.fun is coming soon', template: '%s | dippi.fun' },
  description: 'dippi.fun is coming soon.',
  icons: { icon: '/icon.svg' },
  openGraph: {
    title: 'dippi.fun is coming soon',
    description: 'dippi.fun is coming soon.',
    url: 'https://dippi.fun',
    siteName: 'dippi.fun',
    type: 'website',
  },
  twitter: { card: 'summary_large_image' },
}

export default function RootLayout({
  children,
}: {
  children: React.ReactNode
}) {
  return (
    <html lang="en" className={`${ibmPlexMono.variable} ${playfairDisplay.variable} ${inter.variable} ${dmSans.variable}`}>
      <body className="min-h-screen bg-parchment text-ink font-sans">
        <AppShell>{children}</AppShell>
      </body>
    </html>
  )
}
