# Execution and SDK batch

Released 2026-10-05. This batch follows `a723afe` and contains 35 initial implementation,
test and documentation commits plus this release checklist. A subsequent fix
also applies parent market/side/IOC scope to delegated cancellation.

## Delivered

- Fully bound v2 TWAP signatures shared by HTTP and MCP.
- Parent replay protection, checked nonce ranges, bounded schedule inputs,
  market tick/lot alignment and per-child minimum quantities.
- Monotonic scheduling, prompt cancellation and agent authority checks before
  each child dispatch.
- Accurate Completed/Incomplete/Failed outcomes and exact remaining quantity.
- Wallet-session protection for parent progress and account parent lists.
- Versioned algorithm routes and updated OpenAPI/client schemas.
- TypeScript signing, submission, progress and cancellation helpers, private
  history reads, structured errors and bounded requests without write retries.
- Read-only Python order/fill/algo history, structured HTTP failures and
  connection-option validation.
- Separate required full dependency-audit job so it does not suppress builds
  and SDK tests. CI database test concurrency is bounded to two threads.

## Validation

575 Rust tests passed with all features enabled, including database-backed tests.
Workspace Clippy and formatting passed. Frontend type checking, production build,
29 frontend tests and the actual Chrome agent registration/trade/revocation test
passed. Seven TypeScript client tests and eight Python client tests passed.
Frontend lint has zero errors and 22 existing warnings.

Production dependency audit is clean. The full audit still reports five high
advisories in the development-only Next ESLint/fast-glob/braces chain; current
upstream versions remain unchanged. The audit gate stays required.

## Limits and compatibility

TWAP now requires `signature_version: 2`, a positive price limit and a fully bound
signature. Old signatures are rejected. At this batch boundary, TWAP parents
remained in memory and were unavailable with an authoritative ledger. The
subsequent ledger session added durable execution and restart recovery; see
[the current algorithm contract](api-reference/execution-algorithms.mdx).
Cancellation and revocation stop future slices; a submitted child can still fill.
Indeterminate child outcomes require checking account fills before another parent.
Read-only Python access does not enable trading or cancellation.

## Commit history

| Commit | Change |
|---|---|
| `65f001e` | ci: bound database test concurrency on shared runners |
| `7fa8321` | feat(algos): define canonical fully bound TWAP signing payload |
| `bc7e1dc` | fix(algos): require HTTP signatures binding side price and slices |
| `99aca5a` | fix(mcp): share canonical v2 TWAP authorization with HTTP |
| `13cfba2` | fix(algos): reject unbounded prices invalid slices and excessive durations |
| `ab1835f` | fix(algos): validate child nonce ranges before dispatch |
| `9bd9488` | fix(algos): fence ephemeral parent execution in ledger mode |
| `2b2ae73` | fix(algos): reject replayed signed parents atomically |
| `a722f65` | fix(algos): align child quantities to market lots and minimums |
| `74c0c82` | fix(api): protect algo progress with owner wallet sessions |
| `ea24729` | feat(api): list execution parents for an authenticated account |
| `bcaaad6` | fix(algos): distinguish exhausted schedules from fully filled parents |
| `606046e` | fix(algos): stop execution on rejected or indeterminate child orders |
| `fcc048e` | fix(algos): anchor slice scheduling to monotonic deadlines |
| `7949a2c` | fix(algos): wake scheduled parents immediately on cancellation |
| `a8bdbfd` | fix(algos): propagate typed child execution results |
| `3aed00a` | fix(agents): stop TWAP children after revocation expiry or scope removal |
| `ad7244f` | feat(api): expose signed execution algorithms through v1 routes |
| `83e45e5` | feat(sdk): add canonical TWAP signing helper with integer guards |
| `9bf0112` | feat(sdk): submit externally signed TWAP parents |
| `eb10c92` | feat(sdk): add authenticated algo progress list and cancellation |
| `343a1a0` | test(algos): verify signed parent lifecycle and adversarial inputs |
| `3cbfefa` | feat(sdk): expose structured API errors and retry timing |
| `6dd6e34` | feat(sdk): bound HTTP requests without retrying signed mutations |
| `09c5538` | feat(sdk): add private open-order and fill-history reads |
| `23ff64e` | feat(python-sdk): add read-only order fill and algo history |
| `7ed33ce` | feat(python-sdk): preserve HTTP errors and retry-after metadata |
| `fcd30f1` | fix(python-sdk): validate connection options before sending credentials |
| `ca952bd` | fix(sdk): validate transport URLs before attaching bearer sessions |
| `f4af5a6` | test(sdk): cover authorization encoding timeout and malformed responses |
| `7f795b9` | test(algos): cover partial execution and changing delegated authority |
| `681e060` | feat(api): publish versioned execution contracts and regenerated clients |
| `8dc5a02` | docs(sdk): explain execution lifecycle errors and private history |
| `7455d46` | docs(algos): publish TWAP signing recovery and outcome contracts |
| `fb5b2bf` | ci: run build and SDK checks independently of the full audit gate |
