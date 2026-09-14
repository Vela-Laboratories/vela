import { describe, expect, it } from 'vitest'
import {
  parseWithdrawalAmount,
  withdrawalCancelMessage,
  withdrawalReservationMessage,
} from './withdrawal'

describe('authoritative withdrawal requests', () => {
  it('converts supported amounts into exact 8-decimal request units', () => {
    expect(parseWithdrawalAmount('1.000001', 'USDC')).toBe('100000100')
    expect(parseWithdrawalAmount('0.12345678', 'ETH')).toBe('12345678')
    expect(() => parseWithdrawalAmount('1.0000001', 'USDC')).toThrow(/6 decimal/)
    expect(() => parseWithdrawalAmount('0', 'ETH')).toThrow(/supported range/)
  })

  it('binds customer, destination, amount, nonce, and request ID', () => {
    expect(withdrawalReservationMessage({
      withdrawalId: '11111111-1111-4111-8111-111111111111',
      customer: '0xAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA',
      destination: '0xBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB',
      asset: 'USDC',
      amount: '100000000',
      nonce: 42,
    })).toBe(
      'vela:withdrawal-reservation:11111111-1111-4111-8111-111111111111:0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb:USDC:100000000:42',
    )
    expect(withdrawalCancelMessage('request-id', 42)).toBe(
      'vela:withdrawal-cancel:request-id:42',
    )
  })
})
