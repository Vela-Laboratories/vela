import { describe, expect, it } from 'vitest'
import { resolveVerifiedDepositConfig } from './contract'

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

  it('routes Ethereum Sepolia deposits to its own network and explorer', () => {
    expect(resolveVerifiedDepositConfig(11155111, VAULT, USDC)).toEqual({
      chainId: 11155111,
      chainName: 'Ethereum Sepolia',
      chainIdHex: '0xaa36a7',
      vaultAddress: VAULT,
      usdcAddress: USDC,
      rpcUrl: 'https://ethereum-sepolia-rpc.publicnode.com',
      explorerUrl: 'https://sepolia.etherscan.io',
    })
  })

  it('allows ETH while withholding USDC when its token address is absent', () => {
    const config = resolveVerifiedDepositConfig(8453, VAULT, '')
    expect(config?.chainName).toBe('Base')
    expect(config?.usdcAddress).toBeNull()
  })
})
