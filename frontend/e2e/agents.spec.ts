import { expect, test } from '@playwright/test'
import { hexToString, type Hex } from 'viem'
import { privateKeyToAccount } from 'viem/accounts'
import { orderSigningMessage } from '../lib/signing'

// Public deterministic test fixtures, never a user's wallet.
const master = privateKeyToAccount(`0x${'11'.repeat(32)}`)
const agent = privateKeyToAccount(`0x${'22'.repeat(32)}`)

test('register, use, and revoke an external agent through the browser', async ({ page, request }) => {
  const pageErrors: string[] = []
  page.on('pageerror', (error) => pageErrors.push(error.message))
  await page.exposeFunction('fixtureWalletRequest', async ({ method, params }: { method: string; params?: unknown[] }) => {
    if (method === 'eth_requestAccounts' || method === 'eth_accounts') return [master.address]
    if (method === 'eth_chainId') return '0xaa36a7'
    if (method === 'wallet_requestPermissions') return [{ parentCapability: 'eth_accounts' }]
    if (method === 'personal_sign') {
      const message = String(params?.[0])
      return master.signMessage({ message: message.startsWith('0x') ? hexToString(message as Hex) : message })
    }
    throw new Error(`Unsupported fixture wallet method: ${method}`)
  })
  await page.addInitScript(() => {
    const bridge = window as unknown as { fixtureWalletRequest: (args: object) => Promise<unknown>; ethereum: object }
    bridge.ethereum = {
      isMetaMask: true,
      request: (args: object) => bridge.fixtureWalletRequest(args),
      on: () => {},
      removeListener: () => {},
    }
  })
  const deposit = await request.post('http://localhost:3001/deposit', {
    data: { user: master.address, asset: 'USDC', amount: '1000' },
  })
  expect(deposit.ok()).toBeTruthy()
  await page.goto('/agents')
  await page.getByRole('button', { name: 'Connect wallet', exact: true }).last().click()
  await expect(page.getByRole('heading', { name: 'Register an existing agent' })).toBeVisible()
  await page.getByLabel('Agent public address').fill(agent.address)
  await page.getByLabel('Allowed markets (optional)').fill('BTC-USDC')
  await page.getByLabel('Allowed side').selectOption('Bid')
  await page.getByRole('button', { name: 'Sign and register' }).click()
  await expect(page.getByRole('button', { name: 'Revoke', exact: true })).toBeVisible()
  await page.screenshot({ path: 'test-results/agents-registered.png', fullPage: true })

  const order = {
    address: master.address,
    market: 'BTC-USDC',
    side: 'Bid' as const,
    order_type: 'PostOnly' as const,
    price: 1_000_000,
    quantity: 1_000_000,
    nonce: Date.now(),
    signature_version: 2 as const,
  }
  const signature = await agent.signMessage({ message: orderSigningMessage(order) })
  const placed = await request.post('http://localhost:3001/v1/orders', { data: { ...order, signature } })
  expect(placed.ok()).toBeTruthy()
  expect((await placed.json()).ok).toBe(true)

  await page.getByRole('button', { name: 'Revoke', exact: true }).click()
  await expect(page.getByText(/Revoked · Expires/)).toBeVisible()
  const after = { ...order, nonce: order.nonce + 1 }
  const revokedSignature = await agent.signMessage({ message: orderSigningMessage(after) })
  const rejected = await request.post('http://localhost:3001/v1/orders', { data: { ...after, signature: revokedSignature } })
  expect(rejected.status()).toBe(401)
  expect(pageErrors).toEqual([])
})
