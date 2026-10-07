import { encodeFunctionData, encodePacked, getAddress, keccak256, parseAbi, parseUnits } from 'viem'

const CONTRACT_ADDRESS = '0xAa8E680c11a883F9bf6eb980B2D4E9D18DD25686'
const SEPOLIA_CHAIN_ID = 11155111
const ETH_ASSET = '0x0000000000000000000000000000000000000000'
export const SEPOLIA_USDC = '0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238'

const DEPOSIT_CHAIN_ID = Number(process.env.NEXT_PUBLIC_DEPOSIT_CHAIN_ID ?? '84532')
const DEPOSIT_VAULT_ADDRESS = process.env.NEXT_PUBLIC_DEPOSIT_VAULT_ADDRESS ?? ''
const DEPOSIT_USDC_ADDRESS = process.env.NEXT_PUBLIC_DEPOSIT_USDC_ADDRESS ?? ''
const ZERO_ADDRESS = '0x0000000000000000000000000000000000000000'

export interface VerifiedDepositConfig {
  chainId: 8453 | 84532
  chainName: 'Base' | 'Base Sepolia'
  chainIdHex: string
  vaultAddress: string
  usdcAddress: string | null
  rpcUrl: string
  explorerUrl: string
}

function isAddress(value: string): boolean {
  return /^0x[0-9a-fA-F]{40}$/.test(value) && value.toLowerCase() !== ZERO_ADDRESS
}

export function getVerifiedDepositConfig(): VerifiedDepositConfig | null {
  return resolveVerifiedDepositConfig(
    DEPOSIT_CHAIN_ID,
    DEPOSIT_VAULT_ADDRESS,
    DEPOSIT_USDC_ADDRESS,
  )
}

export function resolveVerifiedDepositConfig(
  chainId: number,
  vaultAddress: string,
  usdcAddress: string,
): VerifiedDepositConfig | null {
  if ((chainId !== 8453 && chainId !== 84532) || !isAddress(vaultAddress)) {
    return null
  }
  const isMainnet = chainId === 8453
  return {
    chainId,
    chainName: isMainnet ? 'Base' : 'Base Sepolia',
    chainIdHex: `0x${chainId.toString(16)}`,
    vaultAddress,
    usdcAddress: isAddress(usdcAddress) ? usdcAddress : null,
    rpcUrl: isMainnet ? 'https://mainnet.base.org' : 'https://sepolia.base.org',
    explorerUrl: isMainnet ? 'https://basescan.org' : 'https://sepolia.basescan.org',
  }
}

function requireVerifiedDepositConfig(asset?: 'ETH' | 'USDC'): VerifiedDepositConfig {
  const config = getVerifiedDepositConfig()
  if (!config) {
    throw new Error('Verified deposits are not configured for this deployment.')
  }
  if (asset === 'USDC' && !config.usdcAddress) {
    throw new Error('Verified USDC deposits are not configured for this deployment.')
  }
  return config
}

export const V3_DEPOSIT_ABI = parseAbi([
  'function depositETH(bytes32 accountId) payable',
  'function depositToken(bytes32 accountId,address asset,uint256 amount)',
])

// The account marker is stable audit context. The V3 indexer credits the
// indexed depositor wallet, so this marker cannot redirect beneficial ownership.
export function accountMarkerForWallet(wallet: string, chainId: 8453 | 84532): `0x${string}` {
  const address = getAddress(wallet as `0x${string}`)
  return keccak256(encodePacked(['string', 'uint256', 'address'], ['dippi-account-v1', BigInt(chainId), address]))
}

export function encodeV3DepositETH(wallet: string, chainId: 8453 | 84532): `0x${string}` {
  return encodeFunctionData({
    abi: V3_DEPOSIT_ABI,
    functionName: 'depositETH',
    args: [accountMarkerForWallet(wallet, chainId)],
  })
}

export function encodeV3DepositToken(
  wallet: string,
  chainId: 8453 | 84532,
  asset: string,
  amountWei: string,
): `0x${string}` {
  return encodeFunctionData({
    abi: V3_DEPOSIT_ABI,
    functionName: 'depositToken',
    args: [accountMarkerForWallet(wallet, chainId), getAddress(asset as `0x${string}`), BigInt(amountWei)],
  })
}

function encodeGetBalance(userAddress: string): string {
  const selector = '0x5c36b186'
  const paddedUser = userAddress.toLowerCase().replace('0x', '').padStart(64, '0')
  const paddedAsset = ETH_ASSET.replace('0x', '').padStart(64, '0')
  return selector + paddedUser + paddedAsset
}

export async function switchToSepolia(): Promise<void> {
  const ethereum = (window as any).ethereum
  try {
    await ethereum.request({
      method: 'wallet_switchEthereumChain',
      params: [{ chainId: '0xaa36a7' }],
    })
  } catch (err: any) {
    if (err.code === 4902) {
      await ethereum.request({
        method: 'wallet_addEthereumChain',
        params: [
          {
            chainId: '0xaa36a7',
            chainName: 'Sepolia',
            nativeCurrency: { name: 'ETH', symbol: 'ETH', decimals: 18 },
            rpcUrls: ['https://rpc.sepolia.org'],
            blockExplorerUrls: ['https://sepolia.etherscan.io'],
          },
        ],
      })
    } else {
      throw err
    }
  }
}

export async function switchToVerifiedDepositChain(): Promise<void> {
  const config = requireVerifiedDepositConfig()
  const ethereum = (window as any).ethereum
  if (!ethereum) throw new Error('No browser wallet detected.')
  try {
    await ethereum.request({
      method: 'wallet_switchEthereumChain',
      params: [{ chainId: config.chainIdHex }],
    })
  } catch (err: any) {
    if (err.code !== 4902) throw err
    await ethereum.request({
      method: 'wallet_addEthereumChain',
      params: [
        {
          chainId: config.chainIdHex,
          chainName: config.chainName,
          nativeCurrency: { name: 'Ether', symbol: 'ETH', decimals: 18 },
          rpcUrls: [config.rpcUrl],
          blockExplorerUrls: [config.explorerUrl],
        },
      ],
    })
  }
}

export async function depositETH(amountEth: string): Promise<string> {
  const config = requireVerifiedDepositConfig('ETH')
  const ethereum = (window as any).ethereum
  await switchToVerifiedDepositChain()

  const accounts: string[] = await ethereum.request({ method: 'eth_accounts' })
  if (!accounts.length) throw new Error('No connected account')

  const weiHex = `0x${parseUnits(amountEth, 18).toString(16)}`

  const txHash: string = await ethereum.request({
    method: 'eth_sendTransaction',
    params: [
      {
        from: accounts[0],
        to: config.vaultAddress,
        data: encodeV3DepositETH(accounts[0], config.chainId),
        value: weiHex,
      },
    ],
  })

  return txHash
}

function encodeWithdrawCall(
  assetAddress: string,
  amountWei: string,
  nonce: number,
  signature: string
): string {
  const selector = '3ccfd60b'
  const paddedAsset = assetAddress.slice(2).padStart(64, '0')
  const paddedAmount = BigInt(amountWei).toString(16).padStart(64, '0')
  const paddedNonce = BigInt(nonce).toString(16).padStart(64, '0')
  const offset = '0000000000000000000000000000000000000000000000000000000000000080'
  const sigHex = signature.startsWith('0x') ? signature.slice(2) : signature
  const sigLength = BigInt(sigHex.length / 2).toString(16).padStart(64, '0')
  const sigPadded = sigHex.padEnd(Math.ceil(sigHex.length / 64) * 64, '0')
  return '0x' + selector + paddedAsset + paddedAmount + paddedNonce + offset + sigLength + sigPadded
}

export async function withdrawETH(
  amountWei: string,
  nonce: number,
  signature: string
): Promise<string> {
  await switchToSepolia()
  const ethereum = (window as any).ethereum
  const accounts: string[] = await ethereum.request({ method: 'eth_accounts' })
  if (!accounts.length) throw new Error('No connected account')

  const data = encodeWithdrawCall(ETH_ASSET, amountWei, nonce, signature)

  const txHash: string = await ethereum.request({
    method: 'eth_sendTransaction',
    params: [
      {
        from: accounts[0],
        to: CONTRACT_ADDRESS,
        data,
        value: '0x0',
      },
    ],
  })

  return txHash
}

export async function requestWithdrawalSignature(
  user: string,
  asset: string,
  amount: string,
  nonce: number
): Promise<{ signature: string; amountWei: string; assetAddress: string }> {
  const apiUrl = process.env.NEXT_PUBLIC_API_URL ?? 'https://vela-engine.fly.dev'
  const res = await fetch(`${apiUrl}/withdrawal-signature`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ user, asset, amount, nonce }),
    cache: 'no-store',
  })
  const data = await res.json()
  if (!data.ok) throw new Error(data.error ?? 'Failed to get signature')
  return {
    signature: data.data.signature,
    amountWei: data.data.amount_wei,
    assetAddress: data.data.asset,
  }
}

export async function getUSDCBalance(address: string): Promise<string> {
  const config = requireVerifiedDepositConfig('USDC')
  const ethereum = (window as any).ethereum
  const currentChainId: string = await ethereum.request({ method: 'eth_chainId' })
  if (currentChainId.toLowerCase() !== config.chainIdHex) {
    throw new Error(`Switch to ${config.chainName} to view the USDC balance.`)
  }
  const paddedAddr = address.toLowerCase().replace('0x', '').padStart(64, '0')
  const result: string = await ethereum.request({
    method: 'eth_call',
    params: [{ to: config.usdcAddress, data: '0x70a08231' + paddedAddr }, 'latest'],
  })
  if (!result || result === '0x') return '0'
  return (Number(BigInt(result)) / 1e6).toString()
}

export async function approveUSDC(amount: string): Promise<string> {
  const config = requireVerifiedDepositConfig('USDC')
  await switchToVerifiedDepositChain()
  const ethereum = (window as any).ethereum
  const accounts: string[] = await ethereum.request({ method: 'eth_accounts' })
  if (!accounts.length) throw new Error('No connected account')

  const amountWei = parseUnits(amount, 6)
  const paddedSpender = config.vaultAddress.toLowerCase().replace('0x', '').padStart(64, '0')
  const paddedAmount = amountWei.toString(16).padStart(64, '0')
  const data = '0x095ea7b3' + paddedSpender + paddedAmount

  const txHash: string = await ethereum.request({
    method: 'eth_sendTransaction',
    params: [{ from: accounts[0], to: config.usdcAddress, data }],
  })
  return txHash
}

export async function depositToken(amountWei: string): Promise<string> {
  const config = requireVerifiedDepositConfig('USDC')
  await switchToVerifiedDepositChain()
  const ethereum = (window as any).ethereum
  const accounts: string[] = await ethereum.request({ method: 'eth_accounts' })
  if (!accounts.length) throw new Error('No connected account')

  const data = encodeV3DepositToken(accounts[0], config.chainId, config.usdcAddress!, amountWei)

  const txHash: string = await ethereum.request({
    method: 'eth_sendTransaction',
    params: [{ from: accounts[0], to: config.vaultAddress, data, value: '0x0' }],
  })
  return txHash
}

export async function checkUSDCAllowance(ownerAddress: string): Promise<bigint> {
  const config = requireVerifiedDepositConfig('USDC')
  await switchToVerifiedDepositChain()
  const ethereum = (window as any).ethereum
  const paddedOwner = ownerAddress.toLowerCase().replace('0x', '').padStart(64, '0')
  const paddedSpender = config.vaultAddress.toLowerCase().replace('0x', '').padStart(64, '0')
  const data = '0xdd62ed3e' + paddedOwner + paddedSpender

  const result: string = await ethereum.request({
    method: 'eth_call',
    params: [{ to: config.usdcAddress, data }, 'latest'],
  })
  if (!result || result === '0x') return 0n
  return BigInt(result)
}

export async function waitForTransactionReceipt(
  transactionHash: string,
  timeoutMs = 180_000,
): Promise<void> {
  const ethereum = (window as any).ethereum
  if (!ethereum) throw new Error('No browser wallet detected.')
  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
    const receipt = await ethereum.request({
      method: 'eth_getTransactionReceipt',
      params: [transactionHash],
    }) as { status?: string } | null
    if (receipt) {
      if (receipt.status === '0x0') throw new Error('The deposit transaction reverted.')
      return
    }
    await new Promise((resolve) => window.setTimeout(resolve, 2_000))
  }
  throw new Error('The deposit transaction is still pending. Check your wallet or block explorer.')
}

export async function getOnChainBalance(userAddress: string): Promise<string> {
  const ethereum = (window as any).ethereum

  const result: string = await ethereum.request({
    method: 'eth_call',
    params: [
      {
        to: CONTRACT_ADDRESS,
        data: encodeGetBalance(userAddress),
      },
      'latest',
    ],
  })

  if (!result || result === '0x') return '0'
  const wei = BigInt(result)
  return (Number(wei) / 1e18).toString()
}
