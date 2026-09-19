# M8 evidence record: <exercise or review name>

- **Status:** `DRAFT` | `PASS` | `FAIL` | `BLOCKED`
- **Evidence ID:** `<stable-id>`
- **Scope:** `adversarial-testnet` | `incident-rehearsal` | `fork` | `deploy-day` | `external-review`
- **Reviewed commit:** `<git SHA>`
- **Environment:** `<named non-production environment>`
- **Operator and independent reviewer:** `<identities>`
- **Started / completed (UTC):** `<timestamps>`

## Preconditions

- [ ] V3 artifact manifest and source commit are pinned.
- [ ] No real funds, production credentials, or customer data are in scope.
- [ ] Required external dependencies and their evidence owners are named.
- [ ] Abort, pause, and customer-communication criteria are agreed.

## Scenario and expected safety properties

Describe the injected condition, exact commands or runbook references, and
expected fail-closed behavior. Include request IDs, deployment IDs, block
ranges, ledger transaction IDs, and evidence hashes where applicable, but not
secrets.

## Observed evidence

| Artifact | Location or hash | Owner | Result |
| --- | --- | --- | --- |
| Logs/trace | `<path or SHA256>` | `<name>` | `PASS/FAIL` |
| Ledger/journal evidence | `<IDs>` | `<name>` | `PASS/FAIL` |
| Chain/provider evidence | `<manifest reference>` | `<name>` | `PASS/FAIL` |
| Customer communication | `<ticket/status-page reference>` | `<name>` | `PASS/FAIL` |

## Disposition

- **Failures and residual risk:** `<none or exact description>`
- **Follow-up owner and due date:** `<name/date>`
- **Independent review completed:** `[ ]`
- **Go/no-go decision reference:** `<ticket or decision record>`
