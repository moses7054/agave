//! Integration test for jsonrpsee migration
//!
//! This test verifies that the jsonrpsee-based RPC server works correctly
//! and can handle the getBalance method.

#![cfg(test)]

// Note: This test requires the dev-context-only-utils feature which is enabled in dev-dependencies
use {
    jsonrpsee::{core::client::ClientT, http_client::HttpClientBuilder, rpc_params},
    serde_json::json,
    solana_ledger::genesis_utils::create_genesis_config,
    solana_net_utils::SocketAddrSpace,
    solana_rpc::{
        rpc::JsonRpcRequestProcessor, rpc_service::service_runtime,
        rpc_service_jsonrpsee::start_jsonrpsee_server,
    },
    solana_rpc_client_api::response::Response as RpcResponse,
    solana_runtime::bank::Bank,
    solana_send_transaction_service::transaction_client::TpuClientNextClient,
    solana_signer::Signer,
    std::{
        net::{IpAddr, Ipv4Addr, SocketAddr},
        sync::Arc,
    },
};

#[tokio::test]
async fn test_jsonrpsee_get_balance() {
    // Create test genesis config with initial balance
    let genesis = create_genesis_config(20_000_000); // 20 SOL in lamports
    let mint_pubkey = genesis.mint_keypair.pubkey();

    // Create test bank
    let bank = Bank::new_for_tests(&genesis.genesis_config);

    // Create JsonRpcRequestProcessor (same as existing tests)
    let processor = JsonRpcRequestProcessor::new_from_bank::<TpuClientNextClient>(
        bank,
        SocketAddrSpace::Unspecified,
    );

    // Find available port for test server
    let port = solana_net_utils::find_available_port_in_range(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        (10000, 11000),
    )
    .expect("Failed to find port");
    let rpc_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();

    // Start jsonrpsee server
    let server_handle = start_jsonrpsee_server(
        rpc_addr,
        processor,
        50 * 1024,         // 50KB request size
        200 * 1024 * 1024, // 200MB response size
    )
    .await
    .expect("Failed to start server");

    // Give server a moment to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Create HTTP client
    let client = HttpClientBuilder::default()
        .build(format!("http://{}", rpc_addr))
        .expect("Failed to create client");

    // Test 1: Get balance for mint account (must provide both pubkey and config)
    let response: RpcResponse<u64> = client
        .request(
            "getBalance",
            rpc_params![mint_pubkey.to_string(), json!(null)],
        )
        .await
        .expect("Failed to call getBalance");

    assert_eq!(
        response.value, 20_000_000,
        "Balance should match initial amount"
    );
    assert_eq!(response.context.slot, 0, "Slot should be 0");

    // Test 2: Get balance with explicit config
    let response_with_config: RpcResponse<u64> = client
        .request(
            "getBalance",
            rpc_params![
                mint_pubkey.to_string(),
                json!({
                    "commitment": "processed"
                })
            ],
        )
        .await
        .expect("Failed to call getBalance with config");

    assert_eq!(response_with_config.value, 20_000_000);

    // Test 3: Try to get balance for non-existent account (should return 0)
    let random_pubkey = solana_pubkey::new_rand();
    let response_nonexistent: RpcResponse<u64> = client
        .request(
            "getBalance",
            rpc_params![random_pubkey.to_string(), json!(null)],
        )
        .await
        .expect("Failed to call getBalance for non-existent account");

    assert_eq!(
        response_nonexistent.value, 0,
        "Non-existent account should have 0 balance"
    );

    // Test 4: Invalid pubkey should return error
    let result: Result<RpcResponse<u64>, _> = client
        .request("getBalance", rpc_params!["invalid-pubkey", json!(null)])
        .await;

    assert!(result.is_err(), "Invalid pubkey should return an error");

    // Clean up
    drop(server_handle);
}

#[tokio::test]
async fn test_jsonrpsee_concurrent_requests() {
    // Test that the server can handle multiple concurrent requests
    let genesis = create_genesis_config(100_000_000);
    let mint_pubkey = genesis.mint_keypair.pubkey();
    let bank = Bank::new_for_tests(&genesis.genesis_config);
    let processor = JsonRpcRequestProcessor::new_from_bank::<TpuClientNextClient>(
        bank,
        SocketAddrSpace::Unspecified,
    );

    let port = solana_net_utils::find_available_port_in_range(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        (11000, 12000),
    )
    .expect("Failed to find port");
    let rpc_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();

    let _server_handle = start_jsonrpsee_server(rpc_addr, processor, 50 * 1024, 200 * 1024 * 1024)
        .await
        .expect("Failed to start server");

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = Arc::new(
        HttpClientBuilder::default()
            .build(format!("http://{}", rpc_addr))
            .expect("Failed to create client"),
    );

    // Spawn 10 concurrent requests
    let mut handles = vec![];
    for _ in 0..10 {
        let pubkey = mint_pubkey.to_string();
        let client_ref = client.clone();
        let handle: tokio::task::JoinHandle<u64> = tokio::spawn(async move {
            let response: RpcResponse<u64> = client_ref
                .request("getBalance", rpc_params![pubkey, json!(null)])
                .await
                .expect("Request failed");
            response.value
        });
        handles.push(handle);
    }

    // Wait for all requests to complete
    for handle in handles {
        let balance = handle.await.expect("Task failed");
        assert_eq!(
            balance, 100_000_000,
            "All requests should get correct balance"
        );
    }
}

#[test]
fn test_jsonrpsee_with_runtime() {
    // Test that jsonrpsee works with a custom Tokio runtime (like Agave uses)
    let genesis = create_genesis_config(50_000_000);
    let mint_pubkey = genesis.mint_keypair.pubkey();
    let bank = Bank::new_for_tests(&genesis.genesis_config);
    let processor = JsonRpcRequestProcessor::new_from_bank::<TpuClientNextClient>(
        bank,
        SocketAddrSpace::Unspecified,
    );

    // Create a custom runtime like Agave does
    let runtime = service_runtime(
        4, // worker threads
        2, // blocking threads
        0, // niceness adjustment
    );

    let port = solana_net_utils::find_available_port_in_range(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        (12000, 13000),
    )
    .expect("Failed to find port");
    let rpc_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();

    // Start server in the custom runtime
    let _server_handle = runtime.block_on(async {
        start_jsonrpsee_server(rpc_addr, processor, 50 * 1024, 200 * 1024 * 1024)
            .await
            .expect("Failed to start server")
    });

    std::thread::sleep(std::time::Duration::from_millis(100));

    // Make request in the runtime
    runtime.block_on(async {
        let client = HttpClientBuilder::default()
            .build(format!("http://{}", rpc_addr))
            .expect("Failed to create client");

        let response: RpcResponse<u64> = client
            .request(
                "getBalance",
                rpc_params![mint_pubkey.to_string(), json!(null)],
            )
            .await
            .expect("Failed to call getBalance");

        assert_eq!(response.value, 50_000_000);
        println!("✅ jsonrpsee works correctly with custom Tokio runtime");
    });
}
