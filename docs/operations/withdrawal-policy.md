# Withdrawal policy operations

## Scope

EX-025 provides free, provider-neutral controls in PostgreSQL. It does not configure a sanctions, KYT, or blockchain-analytics vendor and it does not enable custody signing or transaction broadcast. A missing policy or a compliance disposition of `reject`, `review`, or `unavailable` cannot produce an authorization. EX-026 adds a separate source- and test-only V2 execution evidence boundary; see [Withdrawal execution observer operations](withdrawal-observer.md).

Policy values are exact asset base units. For example, one USDC is `1000000` and one ETH is `1000000000000000000`. Do not copy limits between assets without converting units and completing a risk review.

## Publish a policy version

Connect with the migrator credential after schema migration and asset bootstrap. Insert one reviewed row for each supported asset. Rows are immutable; changing any value requires a new version with a later `effective_at`.

```sql
INSERT INTO ledger_withdrawal_policy_limits (
    policy_version,
    asset_id,
    max_single_amount,
    max_customer_window_amount,
    max_customer_window_count,
    customer_window_seconds,
    max_platform_window_amount,
    platform_window_seconds,
    effective_at,
    configured_by,
    evidence
)
SELECT
    'withdrawal-policy.reviewed-version',
    id,
    1,
    1,
    1,
    86400,
    1,
    86400,
    TIMESTAMPTZ '2099-01-01 00:00:00+00',
    'operator:replace-me',
    '{"change_ticket":"replace-me"}'::jsonb
FROM ledger_assets
WHERE code = 'USDC';
```

The deliberately future-dated template fails closed instead of becoming active if copied unchanged. Replace every amount, duration, effective time, operator, version, and evidence field through a reviewed change. Confirm the effective rows before enabling any policy worker:

```sql
SELECT asset.code, policy.*
FROM ledger_withdrawal_policy_limits policy
JOIN ledger_assets asset ON asset.id = policy.asset_id
ORDER BY asset.code, policy.effective_at DESC;
```

## Compliance adapter contract

The future policy worker supplies a provider name, an immutable provider evidence reference, and one of four dispositions: `clear`, `reject`, `review`, or `unavailable`. Only `clear` can pass the compliance portion of policy. Local numeric limits and halt state are always evaluated independently, so a provider cannot override them.

The provider response itself should remain in the provider's access-controlled evidence store. Vela persists its reference, the normalized disposition, all numeric inputs and observations, reason codes, evaluator identity, and a SHA-256 evidence hash. Never put sensitive customer screening data in the generic policy JSON.

## Emergency halt

Call `Ledger::activate_global_withdrawal_halt` from an authenticated operations surface with an incident reference and operator identity. Activation is immediate and blocks new reservations, approval, signing, and signature storage. Safe pre-signing cancellation and finalized expiry releases remain available. If two providers prove an exact finalized execution that already moved custody, record its completion even while halted; suppressing that journal would leave the ledger inconsistent with chain state.

Resolution uses `Ledger::resolve_global_withdrawal_halt`. PostgreSQL requires three different identities across activation, resolution initiation, and final resolution. Retain the incident and remediation evidence outside the database and store only stable references and non-sensitive summary evidence in the command.

An asset-wide reconciliation halt remains separate and continues to block the affected asset even when the global halt is resolved.
