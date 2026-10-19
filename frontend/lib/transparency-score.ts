export type CriterionStatus = 'complete' | 'planned' | 'missing'

export interface Criterion {
  id: string
  category: string
  title: string
  description: string
  status: CriterionStatus
  evidence?: string
  plannedIn?: string
}

export const TRANSPARENCY_CRITERIA: Criterion[] = [
  {
    id: 'FT-01',
    category: 'Financial Transparency',
    title: 'Real-time proof of reserves',
    description: 'Contract balance vs. engine credited balances, verifiable by anyone, updated every 60 seconds.',
    status: 'complete',
    evidence: '/transparency',
  },
  {
    id: 'FT-02',
    category: 'Financial Transparency',
    title: 'Live fee revenue disclosure',
    description: 'Total taker fees collected, maker rebates paid, and net exchange revenue published in real time.',
    status: 'complete',
    evidence: '/transparency',
  },
  {
    id: 'FT-03',
    category: 'Financial Transparency',
    title: 'Per-fill fee amounts published',
    description: 'Every fill response includes exact maker_fee and taker_fee amounts. No hidden fee deductions.',
    status: 'complete',
    evidence: 'https://monolithsystematicllc.mintlify.app/market-making/api-guide',
  },
  {
    id: 'FT-04',
    category: 'Financial Transparency',
    title: 'Exchange wallet addresses disclosed',
    description: 'Operator wallet and settlement contract address publicly disclosed with Etherscan links.',
    status: 'complete',
    evidence: '/operator',
  },
  {
    id: 'FT-05',
    category: 'Financial Transparency',
    title: 'Emergency exit guarantee',
    description: 'Users can recover funds directly from the smart contract after a 7-day timelock, no operator required.',
    status: 'complete',
    evidence: 'https://sepolia.etherscan.io/address/0xAa8E680c11a883F9bf6eb980B2D4E9D18DD25686',
  },
  {
    id: 'OT-01',
    category: 'Operational Transparency',
    title: 'Public engine status page',
    description: 'Live engine health, uptime, orders processed, and volume published in real time at /status.',
    status: 'complete',
    evidence: '/status',
  },
  {
    id: 'OT-02',
    category: 'Operational Transparency',
    title: 'Public incident log',
    description: 'Every engine restart, snapshot restore, and degraded performance event logged publicly.',
    status: 'complete',
    evidence: '/status',
  },
  {
    id: 'OT-03',
    category: 'Operational Transparency',
    title: 'Published uptime SLA',
    description: 'Uptime tracked via /status. Published SLA commitment coming in VEL-T2-02b.',
    status: 'planned',
    plannedIn: 'VEL-T2-02b',
  },
  {
    id: 'OT-04',
    category: 'Operational Transparency',
    title: 'Engine version history public',
    description: 'Full git history and release notes published on GitHub. Every change is auditable.',
    status: 'complete',
    evidence: 'https://github.com/Vela-Laboratories/vela/releases',
  },
  {
    id: 'OT-05',
    category: 'Operational Transparency',
    title: 'Spread and slippage analytics',
    description: 'Real-time spread per market, average slippage for standard order sizes, published publicly.',
    status: 'complete',
    evidence: '/analytics',
  },
  {
    id: 'OI-01',
    category: 'Order Integrity',
    title: 'Every order wallet-signed',
    description: "All orders require a cryptographic signature from the user's wallet before the engine will accept them.",
    status: 'complete',
    evidence: 'https://github.com/Vela-Laboratories/vela',
  },
  {
    id: 'OI-02',
    category: 'Order Integrity',
    title: 'Public order audit trail',
    description: "Every order's full lifecycle — signature, timestamps, fills, counterparty — is publicly accessible.",
    status: 'complete',
    evidence: '/transparency',
  },
  {
    id: 'OI-03',
    category: 'Order Integrity',
    title: 'Matching algorithm open source',
    description: 'The complete matching engine source code is published on GitHub under the MIT license.',
    status: 'complete',
    evidence: 'https://github.com/Vela-Laboratories/vela',
  },
  {
    id: 'OI-04',
    category: 'Order Integrity',
    title: 'Price-time priority verifiable',
    description: 'Batch state roots allow anyone to verify that orders were matched in strict price-time priority.',
    status: 'complete',
    evidence: '/batches',
  },
  {
    id: 'OI-05',
    category: 'Order Integrity',
    title: 'No hidden order types or preferences',
    description: 'All supported order types are documented. No special order types exist for privileged users.',
    status: 'complete',
    evidence: 'https://monolithsystematicllc.mintlify.app',
  },
  {
    id: 'GV-01',
    category: 'Governance',
    title: 'Operator identity disclosed',
    description: 'Legal entity name, jurisdiction, founder, and operator wallet address publicly disclosed.',
    status: 'complete',
    evidence: '/operator',
  },
  {
    id: 'GV-02',
    category: 'Governance',
    title: 'Conflict of interest disclosure',
    description: "Operator's other business interests and potential conflicts disclosed and signed.",
    status: 'complete',
    evidence: '/operator',
  },
  {
    id: 'GV-03',
    category: 'Governance',
    title: '14-day advance notice for fee changes',
    description: 'All fee changes announced and signed by the operator wallet at least 14 days before taking effect.',
    status: 'complete',
    evidence: '/decisions',
  },
  {
    id: 'GV-04',
    category: 'Governance',
    title: 'Public decision log with signatures',
    description: 'Every material exchange decision cryptographically signed and permanently recorded.',
    status: 'complete',
    evidence: '/decisions',
  },
  {
    id: 'GV-05',
    category: 'Governance',
    title: 'Operator key rotation schedule',
    description: 'Published key rotation policy with cryptographic proof of each handoff.',
    status: 'complete',
    evidence: '/operator',
  },
  {
    id: 'CV-01',
    category: 'Cryptographic Verification',
    title: 'Batch state roots published',
    description: 'Every 30-second batch of trades produces a keccak256 state root, publicly accessible.',
    status: 'complete',
    evidence: '/batches',
  },
  {
    id: 'CV-02',
    category: 'Cryptographic Verification',
    title: 'Fraud proof submission interface',
    description: 'Any user can download state roots, verify batches, and submit challenges.',
    status: 'complete',
    evidence: '/verify',
  },
  {
    id: 'CV-03',
    category: 'Cryptographic Verification',
    title: 'On-chain state root anchoring',
    description: 'Engine state root anchored to Ethereum every 10 minutes — permanently immutable on-chain.',
    status: 'complete',
    evidence: '/batches',
  },
  {
    id: 'CV-04',
    category: 'Cryptographic Verification',
    title: 'DA layer for state reconstruction',
    description: 'All fills posted to a content-addressed DA layer. Full exchange state reconstructable from DA alone.',
    status: 'complete',
    evidence: 'https://github.com/Vela-Laboratories/vela',
  },
  {
    id: 'CV-05',
    category: 'Cryptographic Verification',
    title: 'ZK proof for every batch',
    description: 'ZK proof infrastructure built. SP1 integration and full per-batch proving ships June 2026 (Stanford AFT Lab).',
    status: 'planned',
    plannedIn: 'VEL-T1-01',
  },
  {
    id: 'CV-06',
    category: 'Cryptographic Verification',
    title: 'TEE hardware attestation',
    description: 'AMD SEV-SNP Trusted Execution Environment attests that the correct matching engine binary ran on tamper-proof hardware after every batch.',
    status: 'planned',
    plannedIn: 'VEL-T1-04',
  },
]

export const TOTAL_CRITERIA = 26

export const COMPLETE_COUNT = TRANSPARENCY_CRITERIA.filter(c => c.status === 'complete').length

export const PLANNED_COUNT = TRANSPARENCY_CRITERIA.filter(c => c.status === 'planned').length

export const CATEGORIES = [
  'Financial Transparency',
  'Operational Transparency',
  'Order Integrity',
  'Governance',
  'Cryptographic Verification',
]
