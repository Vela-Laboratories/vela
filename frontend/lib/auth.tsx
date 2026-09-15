'use client'

// ---------------------------------------------------------------------------
// Vela wallet auth context — thin wrapper over wagmi.
//
// wagmi/viem handles account state, connection, and signing. This module
// keeps the historical AuthContext surface (`address`, `isConnected`,
// `isAuthenticated`, `connect`, `signOut`, `signIn`) so pages don't
// need to change, but delegates every wallet interaction to wagmi hooks.
//
// Persistence
// -----------
// wagmi persists the connected account itself (localStorage under
// `wagmi.store`) and auto-reconnects on mount. We only persist the
// address explicitly to preserve the legacy key for any external tool
// that reads it — but wagmi is the source of truth.
//
// isAuthenticated and the bearer token are deliberately NOT persisted.
// A stale bearer that survives a page reload would silently diverge from
// the server view. On refresh, wagmi reconnects the account but the user
// must establish a fresh, short-lived Vela session.
// ---------------------------------------------------------------------------

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react'
import {
  useAccount,
  useConnect,
  useConnectors,
  useDisconnect,
  useSignMessage,
} from 'wagmi'
import type { VelaWsClient } from './ws'
import {
  createAuthSession,
  requestAuthChallenge,
  revokeAuthSession,
  setAuthSessionToken,
} from './api'

interface AuthState {
  address: string | null
  isConnected: boolean
  isAuthenticated: boolean
}

export interface AuthContextValue extends AuthState {
  /** Connect the wallet if needed, then establish a short-lived Vela
   * session. Returns the authenticated address. */
  connect: () => Promise<string>
  /** Disconnect via wagmi + clear local state. */
  signOut: () => void
  /** Establish a short-lived HTTP session and use it to authenticate the
   * private WebSocket. Signs once via wagmi's useSignMessage. */
  signIn: (wsClient: VelaWsClient, addressOverride?: string) => Promise<void>
}

const PERSISTED_ADDRESS_KEY = 'vela.auth.address'
const AuthContext = createContext<AuthContextValue | null>(null)

export function AuthProvider({ children }: { children: ReactNode }) {
  const { address: wagmiAddress, isConnected: wagmiConnected } = useAccount()
  const { connectAsync } = useConnect()
  const connectors = useConnectors()
  const { disconnect } = useDisconnect()
  const { signMessageAsync } = useSignMessage()

  const [isAuthenticated, setIsAuthenticated] = useState(false)
  const sessionTokenRef = useRef<string | null>(null)
  const sessionAddressRef = useRef<string | null>(null)

  const address = useMemo(
    () => (wagmiAddress ? wagmiAddress.toLowerCase() : null),
    [wagmiAddress],
  )

  // Mirror the connected address into the legacy localStorage key so
  // external tools that used to read it (e.g. curl helpers, older
  // frontend versions) keep working. wagmi already persists its own
  // state — this is best-effort compat only.
  useEffect(() => {
    if (typeof window === 'undefined') return
    try {
      if (address) {
        window.localStorage.setItem(PERSISTED_ADDRESS_KEY, address)
      } else {
        window.localStorage.removeItem(PERSISTED_ADDRESS_KEY)
      }
    } catch {
      /* private mode / quota — best effort */
    }
  }, [address])

  // Any account transition invalidates the in-memory bearer. Sessions are
  // account-bound and are deliberately re-established after wallet changes.
  useEffect(() => {
    if (
      !wagmiConnected
      || !address
      || (sessionAddressRef.current !== null && sessionAddressRef.current !== address)
    ) {
      if (sessionTokenRef.current) void revokeAuthSession()
      sessionTokenRef.current = null
      sessionAddressRef.current = null
      setAuthSessionToken(null)
      setIsAuthenticated(false)
    }
  }, [address, wagmiConnected])

  useEffect(() => {
    const expireSession = () => {
      sessionTokenRef.current = null
      sessionAddressRef.current = null
      setAuthSessionToken(null)
      setIsAuthenticated(false)
    }
    window.addEventListener('vela:auth-expired', expireSession)
    return () => window.removeEventListener('vela:auth-expired', expireSession)
  }, [])

  const establishHttpSession = useCallback(
    async (sessionAddress: string): Promise<string> => {
      const normalizedAddress = sessionAddress.toLowerCase()
      const challenge = await requestAuthChallenge(normalizedAddress)
      if (!challenge.ok || !challenge.data) {
        throw new Error(challenge.error ?? 'Unable to request an authentication challenge.')
      }
      const signature = await signMessageAsync({ message: challenge.data.message })
      const session = await createAuthSession(
        normalizedAddress,
        challenge.data.nonce,
        signature,
      )
      if (!session.ok || !session.data) {
        throw new Error(session.error ?? 'Unable to create an authenticated session.')
      }
      sessionTokenRef.current = session.data.token
      sessionAddressRef.current = normalizedAddress
      setAuthSessionToken(session.data.token)
      setIsAuthenticated(true)
      return session.data.token
    },
    [signMessageAsync],
  )

  const connect = useCallback(async (): Promise<string> => {
    if (wagmiConnected && address) {
      await establishHttpSession(address)
      return address
    }
    const connector = connectors[0]
    if (!connector) {
      throw new Error(
        'No browser wallet detected. Install MetaMask, Rabby, Coinbase Wallet, Brave, or Frame.',
      )
    }
    const result = await connectAsync({ connector })
    const next = result.accounts[0]
    if (!next) throw new Error('Wallet returned no accounts.')
    const normalizedAddress = next.toLowerCase()
    await establishHttpSession(normalizedAddress)
    return normalizedAddress
  }, [address, connectAsync, connectors, establishHttpSession, wagmiConnected])

  const signOut = useCallback(() => {
    void revokeAuthSession()
    sessionTokenRef.current = null
    sessionAddressRef.current = null
    setAuthSessionToken(null)
    disconnect()
    setIsAuthenticated(false)
  }, [disconnect])

  const signIn = useCallback(
    async (wsClient: VelaWsClient, addressOverride?: string): Promise<void> => {
      const sessionAddress = (addressOverride ?? address)?.toLowerCase()
      if (!sessionAddress) {
        throw new Error('Wallet not connected.')
      }
      const token = sessionAddressRef.current === sessionAddress
        ? sessionTokenRef.current ?? await establishHttpSession(sessionAddress)
        : await establishHttpSession(sessionAddress)

      return new Promise<void>((resolve, reject) => {
        let settled = false
        const timeout = setTimeout(() => {
          if (settled) return
          settled = true
          unsubscribeMessage()
          unsubscribeStatus()
          reject(new Error('Private stream authentication timed out.'))
        }, 10_000)
        const finish = (error?: Error) => {
          if (settled) return
          settled = true
          clearTimeout(timeout)
          unsubscribeMessage()
          unsubscribeStatus()
          if (error) {
            reject(error)
          } else {
            setIsAuthenticated(true)
            resolve()
          }
        }
        const authenticateSocket = () => {
          wsClient.authSession(sessionAddress, token)
        }
        const unsubscribeMessage = wsClient.onMessage((msg) => {
          if (settled) return
          if (msg.type === 'authenticated') {
            finish()
            return
          }
          if (msg.type === 'error') {
            finish(new Error(msg.message))
          }
        })
        const unsubscribeStatus = wsClient.onStatus((status) => {
          if (status === 'connected') authenticateSocket()
        })
        if (wsClient.status === 'connected') {
          authenticateSocket()
        } else {
          wsClient.connect()
        }
      })
    },
    [address, establishHttpSession],
  )

  const value: AuthContextValue = {
    address,
    isConnected: wagmiConnected,
    isAuthenticated,
    connect,
    signOut,
    signIn,
  }

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>
}

export function useAuth(): AuthContextValue {
  const ctx = useContext(AuthContext)
  if (!ctx) throw new Error('useAuth must be used within <AuthProvider>.')
  return ctx
}
