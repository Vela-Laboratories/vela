'use client'

import { AnimatePresence, motion } from 'framer-motion'
import { usePathname } from 'next/navigation'
import Providers from '@/components/Providers'
import Nav from '@/components/Nav'
import BetaBanner from '@/components/BetaBanner'
import UnifiedRouteRail from '@/components/UnifiedRouteRail'

/// Client-side wrapper around every route: owns the provider tree
/// (Wagmi + React Query + AuthProvider), the nav, and the framer-motion
/// page-transition animation. The root layout is a server component that
/// mounts this shell once so that only the interactive pieces below
/// force a client boundary.
export default function AppShell({ children }: { children: React.ReactNode }) {
  const pathname = usePathname()
  const isImmersiveRoute = /^\/markets\/[^/]+/.test(pathname) || pathname === '/profile'
  const hasWorkspaceRail = !isImmersiveRoute && !pathname.startsWith('/docs') && pathname !== '/'
  const page = isImmersiveRoute ? children : <AnimatePresence mode="wait">
    <motion.div
      key={pathname}
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -10 }}
      transition={{ duration: 0.22, ease: [0.25, 0.1, 0.25, 1] }}
    >
      {children}
    </motion.div>
  </AnimatePresence>

  return (
    <Providers>
      <div className={`relative z-10 app-shell app-shell--terminal${hasWorkspaceRail ? ' app-shell--workspace' : ''}`}>
        <BetaBanner />
        <Nav />
        <main className="app-main">
          {hasWorkspaceRail ? <div className="unified-app-frame"><UnifiedRouteRail /><div className="unified-app-content">{page}</div></div> : page}
        </main>
      </div>
    </Providers>
  )
}
