# JSONRPSEE Migration Explained (Current `getBalance` PoC)

## Why this exists

This document explains:

1. What we are trying to achieve with the `jsonrpsee` migration.
2. How the current code works end-to-end.
3. What the current tests validate, and what they do not validate yet.

---

## Goal

The goal is to migrate Agave RPC serving from legacy `jsonrpc-core` / `jsonrpc-http-server` to `jsonrpsee`, while keeping existing RPC business logic (`JsonRpcRequestProcessor`) intact.

Current scope is intentionally small: **only `getBalance` is migrated as a proof-of-concept**.

---

## High-level design

The migration keeps the same processor/business layer and replaces only the transport/dispatch layer.

- Old stack: `jsonrpc-http-server` + trait delegates.
- New stack: `jsonrpsee::Server` + `RpcModule` async method registration.
- Selection is compile-time via feature flag:
  - `--features use-jsonrpsee` => new server path.
  - no feature => existing legacy path.

---

## Code walkthrough

### 1) Feature and dependencies

File: `rpc/Cargo.toml`

- `use-jsonrpsee = []` feature enables the new branch.
- `jsonrpsee` dependency is added with server/client features used by implementation and tests.

### 2) New jsonrpsee service module

File: `rpc/src/rpc_service_jsonrpsee.rs`

Main functions:

- `build_rpc_module(processor)`:
  - builds `RpcModule<JsonRpcRequestProcessor>`
  - registers async method `"getBalance"`
- `start_jsonrpsee_server(rpc_addr, processor, ...)`:
  - starts `jsonrpsee::server::Server`
  - mounts module
  - returns `ServerHandle`

`getBalance` handler path:

1. Parse params as `(String, Option<RpcContextConfig>)`.
2. Validate pubkey via `verify_pubkey`.
3. Call existing processor logic: `context.get_balance(...)`.
4. Return standard `RpcResponse<u64>`.

No balance business logic was rewritten; this is an adapter layer.

### 3) RPC service branch wiring

File: `rpc/src/rpc_service.rs`

`JsonRpcService::new()` has two `#[cfg]` branches:

- `#[cfg(not(feature = "use-jsonrpsee"))]`: legacy server thread + middleware.
- `#[cfg(feature = "use-jsonrpsee")]`: runtime task that starts `start_jsonrpsee_server(...)`.

Shutdown/join fields and logic are also feature-gated:

- old: `CloseHandle` + OS thread join
- new: `jsonrpsee::ServerHandle` + tokio task join

### 4) Validator integration path

File: `core/src/validator.rs`

Validator startup always constructs:

- `JsonRpcService::new_with_config(rpc_svc_config)`

So when `solana-rpc/use-jsonrpsee` is enabled, validator startup uses the jsonrpsee branch from `rpc_service.rs`.

---

## End-to-end runtime flow (jsonrpsee mode)

`Validator::new` -> `JsonRpcService::new_with_config` -> `JsonRpcService::new` (`use-jsonrpsee` branch) -> `start_jsonrpsee_server` -> HTTP JSON-RPC request -> `RpcModule` route `"getBalance"` -> `JsonRpcRequestProcessor::get_balance`.

---

## Tests: what they cover

File: `rpc/tests/test_jsonrpsee_migration.rs`

### `test_jsonrpsee_get_balance`

Validates:

- successful `getBalance` for funded account
- explicit commitment config handling
- non-existent account returns `0`
- invalid pubkey returns RPC error

### `test_jsonrpsee_concurrent_requests`

Validates:

- concurrent `getBalance` calls return correct values

### `test_jsonrpsee_with_runtime`

Validates:

- server can run inside Agave-style custom runtime (`service_runtime(...)`)

### Important testing note

These tests start `start_jsonrpsee_server(...)` directly.  
They validate handler correctness and runtime compatibility, but they do **not** by themselves prove the full validator startup path.

---

## Validator-backed verification (what we observed)

With `solana-rpc/use-jsonrpsee` enabled, running a real validator and calling JSON-RPC `getBalance` succeeds.

Example response observed:

```json
{"jsonrpc":"2.0","id":1,"result":{"context":{"slot":0,"apiVersion":"4.0.0"},"value":1}}
```

This confirms:

- validator wiring reaches jsonrpsee path
- `getBalance` works over the real validator RPC endpoint

---

## Current limitations and caveats

1. Only `getBalance` is implemented in jsonrpsee module.
2. Most RPC methods are still missing in jsonrpsee mode, so clients/tests calling them will get `-32601 Method not found`.
3. During some test teardown paths, runtime-drop related panics were observed (`Cannot drop a runtime in a context where blocking is not allowed`), indicating cleanup/lifecycle still needs hardening.
4. Legacy HTTP middleware endpoints and full PubSub migration are not completed in jsonrpsee path.

---

## What we are trying to achieve next

1. Keep the architecture (processor logic unchanged, transport layer migrated).
2. Incrementally port RPC methods into `build_rpc_module(...)`.
3. Prioritize methods required by validator startup/test-validator flow so broader tests stop failing in `use-jsonrpsee` mode.
4. Fix shutdown/cleanup lifecycle issues in jsonrpsee mode.
5. Add at least one validator-backed integration test specifically for `getBalance` in `use-jsonrpsee` mode.

---

## Useful commands

Build:

```bash
cargo check -p solana-rpc
cargo check -p solana-rpc --features use-jsonrpsee
```

jsonrpsee integration tests:

```bash
cargo test -p solana-rpc --features use-jsonrpsee,dev-context-only-utils \
  --test test_jsonrpsee_migration -- --nocapture
```

Validator-backed `getBalance` smoke test (jsonrpsee feature enabled):

```bash
cargo run -p agave-validator --bin solana-test-validator --features solana-rpc/use-jsonrpsee -- \
  --ledger /tmp/test-ledger-jsonrpsee --reset \
  --rpc-port 18999 --gossip-port 19000 --dynamic-port-range 19000-19050 --quiet

curl -X POST http://127.0.0.1:18999 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"getBalance","params":["11111111111111111111111111111111",null]}'
```
