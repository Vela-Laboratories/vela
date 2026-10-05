import assert from 'node:assert/strict'
import test from 'node:test'
import { VelaClient, VelaApiError, twapSigningMessage, algoCancelSigningMessage } from '../src/index.ts'

const input = { address: `0x${'aB'.repeat(20)}`, market: 'ETH-USDC', side: 'Bid' as const, quantity: 12, duration_secs: 60, price_limit: 100, nonce: 1 }

test('TWAP canonical vector and safe integer limits', () => {
  const message = twapSigningMessage(input)
  assert.equal(message, JSON.stringify(['vela:algo:twap:v2', input.address.toLowerCase(), 'ETH-USDC', 'Bid', 12, 60, 100, 12, 1]))
  assert.notEqual(message, twapSigningMessage({ ...input, side: 'Ask' }))
  assert.notEqual(message, twapSigningMessage({ ...input, price_limit: 101 }))
  for (const quantity of [-1, 1.1, Number.MAX_SAFE_INTEGER + 1]) assert.throws(() => twapSigningMessage({ ...input, quantity }))
  assert.throws(() => twapSigningMessage({ ...input, nonce: Number.MAX_SAFE_INTEGER }))
  assert.equal(algoCancelSigningMessage(3, 4), 'vela:algo:cancel:3:4')
})

test('signed TWAP writes explicit defaults and version without retries', async () => {
  const calls: { url: string; init?: RequestInit }[] = []
  const client = new VelaClient({ baseUrl: 'https://example.test/', fetch: async (url, init) => {
    calls.push({ url: String(url), init })
    return Response.json({ ok: true, data: { parent_id: 1, status: 'Running', slice_count: 12 } })
  } })
  await client.placeTwap(input, 'test-signature')
  assert.equal(calls.length, 1)
  assert.equal(calls[0].url, 'https://example.test/v1/orders/algo/twap')
  const body = JSON.parse(String(calls[0].init?.body))
  assert.equal(body.signature_version, 2)
  assert.equal(body.slices, 12)
  assert.equal(body.signature, 'test-signature')
})

test('private history encodes paths and attaches wallet session', async () => {
  const calls: string[] = []
  const client = new VelaClient({ bearerToken: 'test-session', fetch: async (url, init) => {
    calls.push(String(url))
    assert.equal(new Headers(init?.headers).get('Authorization'), 'Bearer test-session')
    assert.equal(init?.method, 'GET')
    return Response.json({ ok: true, data: [] })
  } })
  await client.orders('0xabc/?')
  await client.fills('0xabc/?')
  await client.algos('0xabc/?')
  assert.ok(calls.every(url => url.includes('0xabc%2F%3F')))
})

test('HTTP errors retain status and retry timing without retrying', async () => {
  let calls = 0
  const client = new VelaClient({ fetch: async () => {
    calls++
    return Response.json({ ok: false, error: 'rate limited' }, { status: 429, headers: { 'Retry-After': '7' } })
  } })
  await assert.rejects(client.markets(), (error: unknown) => error instanceof VelaApiError && error.status === 429 && error.retryAfterSeconds === 7 && error.message === 'rate limited')
  assert.equal(calls, 1)
})

test('malformed and unsuccessful successful-status envelopes reject', async () => {
  for (const payload of [{}, { ok: false }, { ok: true }]) {
    const client = new VelaClient({ fetch: async () => Response.json(payload) })
    await assert.rejects(client.markets(), VelaApiError)
  }
})

test('request timeout aborts once and reports indeterminate write outcome', async () => {
  let calls = 0
  const client = new VelaClient({ timeoutMs: 5, fetch: async (_url, init) => {
    calls++
    return new Promise<Response>((_resolve, reject) => init?.signal?.addEventListener('abort', () => reject(new Error('aborted')), { once: true }))
  } })
  await assert.rejects(client.placeTwap(input, 'test-signature'), /timed out; write outcome may be indeterminate/)
  assert.equal(calls, 1)
})

test('invalid connection options reject before transport', () => {
  for (const baseUrl of ['file:///tmp/example', 'https://user:password@example.test', 'https://example.test?token=test']) assert.throws(() => new VelaClient({ baseUrl }))
  for (const timeoutMs of [0, -1, Infinity]) assert.throws(() => new VelaClient({ timeoutMs }))
})
