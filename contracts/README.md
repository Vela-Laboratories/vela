# Vela contracts

The initial consumer-exchange scope is `VelaSettlement`: allowlisted ETH/USDC
custody, unique deposit identities, policy-bound EIP-712 withdrawals, emergency
exit, distinct owner/operator/guardian controls, and optional L2 sequencer
health gating. The repository remains testnet-only and unaudited for real funds.

## Foundry

**Foundry is a blazing fast, portable and modular toolkit for Ethereum application development written in Rust.**

Foundry consists of:

- **Forge**: Ethereum testing framework (like Truffle, Hardhat and DappTools).
- **Cast**: Swiss army knife for interacting with EVM smart contracts, sending transactions and getting chain data.
- **Anvil**: Local Ethereum node, akin to Ganache, Hardhat Network.
- **Chisel**: Fast, utilitarian, and verbose solidity REPL.

## Documentation

https://book.getfoundry.sh/

## Usage

### Build

```shell
$ forge build
```

### Test

```shell
$ forge test
```

Set `MAINNET_RPC_URL` to run the canonical-USDC fork path. Without it, the fork
harness returns before creating a fork and does not constitute fork evidence.

### Reproducible deployment metadata

```shell
$ ./scripts/reproducible_build.sh
```

The JSON output binds the pinned compiler/EVM/optimizer configuration, source
commit, selected chain, creation-bytecode hash, and runtime-template hash.
Because the source commit is an immutable constructor value, the deployment
script separately prints the final on-chain code hash. `script/Deploy.s.sol`
requires explicit chain, operator, owner, guardian, USDC, and source-commit
inputs and refuses role collisions or an undeployed USDC address.

### Format

```shell
$ forge fmt
```

### Gas Snapshots

```shell
$ forge snapshot
```

### Anvil

```shell
$ anvil
```

### Deploy

```shell
$ forge script script/Counter.s.sol:CounterScript --rpc-url <your_rpc_url> --private-key <your_private_key>
```

### Cast

```shell
$ cast <subcommand>
```

### Help

```shell
$ forge --help
$ anvil --help
$ cast --help
```
