import { parseUnits } from 'viem'

export function parseWithdrawalAmount(amount: string, asset: 'ETH' | 'USDC'): string {
  const trimmed = amount.trim()
  const maximumDecimals = asset === 'USDC' ? 6 : 8
  const pattern = new RegExp(`^(?:0|[1-9][0-9]*)(?:\\.[0-9]{1,${maximumDecimals}})?$`)
  if (!pattern.test(trimmed)) {
    throw new Error(`${asset} withdrawals support at most ${maximumDecimals} decimal places.`)
  }
  const engineAmount = parseUnits(trimmed, 8)
  if (engineAmount <= 0n || engineAmount > 18_446_744_073_709_551_615n) {
    throw new Error('Withdrawal amount is outside the supported range.')
  }
  return engineAmount.toString()
}

export function withdrawalReservationMessage(input: {
  withdrawalId: string
  customer: string
  destination: string
  asset: 'ETH' | 'USDC'
  amount: string
  nonce: number
}): string {
  return [
    'vela:withdrawal-reservation',
    input.withdrawalId,
    input.customer.toLowerCase(),
    input.destination.toLowerCase(),
    input.asset,
    input.amount,
    input.nonce,
  ].join(':')
}

export function withdrawalCancelMessage(withdrawalId: string, nonce: number): string {
  return `vela:withdrawal-cancel:${withdrawalId}:${nonce}`
}
