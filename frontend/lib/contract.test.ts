import { describe, expect, it } from 'vitest'
import { decodeFunctionData } from 'viem'
import {
  accountMarkerForWallet,
  encodeV3DepositETH,
  encodeV3DepositToken,
  resolveVerifiedDepositConfig,
  V3_DEPOSIT_ABI,
} from './contract'

const VAULT = '0x1111111111111111111111111111111111111111'
const USDC = '0x2222222222222222222222222222222222222222'

describe('verified deposit configuration', () => {
  it('accepts only supported deployments with an explicit vault address', () => {
    expect(resolveVerifiedDepositConfig(1, VAULT, USDC)).toBeNull()
    expect(resolveVerifiedDepositConfig(84532, '', USDC)).toBeNull()
    expect(
      resolveVerifiedDepositConfig(
        84532,
        '0x0000000000000000000000000000000000000000',
        USDC,
      ),
    ).toBeNull()

    expect(resolveVerifiedDepositConfig(84532, VAULT, USDC)).toEqual({
      chainId: 84532,
      chainName: 'Base Sepolia',
      chainIdHex: '0x14a34',
      vaultAddress: VAULT,
      usdcAddress: USDC,
      rpcUrl: 'https://sepolia.base.org',
      explorerUrl: 'https://sepolia.basescan.org',
    })
  })

  it('rejects the incompatible legacy Ethereum Sepolia vault', () => {
    expect(resolveVerifiedDepositConfig(11155111, VAULT, USDC)).toBeNull()
  })

  it('allows ETH while withholding USDC when its token address is absent', () => {
    const config = resolveVerifiedDepositConfig(8453, VAULT, '')
    expect(config?.chainName).toBe('Base')
    expect(config?.usdcAddress).toBeNull()
  })
})

describe('V3 deposit calldata', () => {
  it('binds ETH deposits to a nonzero account marker', () => {
    const marker = accountMarkerForWallet(VAULT, 84532)
    expect(marker).not.toBe(`0x${'0'.repeat(64)}`)
    expect(marker).not.toBe(accountMarkerForWallet(VAULT, 8453))
    expect(decodeFunctionData({ abi: V3_DEPOSIT_ABI, data: encodeV3DepositETH(VAULT, 84532) })).toEqual({
      functionName: 'depositETH',
      args: [marker],
    })
  })

  it('encodes six-decimal USDC amount and vault account marker', () => {
    expect(decodeFunctionData({
      abi: V3_DEPOSIT_ABI,
      data: encodeV3DepositToken(VAULT, 84532, USDC, '1500000'),
    })).toEqual({
      functionName: 'depositToken',
      args: [accountMarkerForWallet(VAULT, 84532), USDC, 1_500_000n],
    })
  })
})
