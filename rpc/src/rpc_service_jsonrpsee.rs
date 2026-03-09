use {
    crate::rpc::{verify_pubkey, JsonRpcRequestProcessor},
    jsonrpsee::{
        server::{ServerBuilder, ServerHandle},
        types::{ErrorCode, ErrorObjectOwned},
        RpcModule,
    },
    log::{debug, info},
    solana_rpc_client_api::config::RpcContextConfig,
    std::net::SocketAddr,
};

/// Build RPC module with methods registered
///
/// This function creates an RpcModule and registers all RPC methods.
/// Currently implements getBalance as a proof-of-concept.
fn build_rpc_module(
    processor: JsonRpcRequestProcessor,
) -> Result<RpcModule<JsonRpcRequestProcessor>, Box<dyn std::error::Error>> {
    let mut module = RpcModule::new(processor);

    // Register getBalance method
    // Note: register_async_method takes 3 parameters: params, context, and extensions
    module.register_async_method("getBalance", |params, context, _extensions| async move {
        // Parse parameters as tuple - jsonrpsee API
        let (pubkey_str, config): (String, Option<RpcContextConfig>) =
            params.parse().map_err(|e| {
                ErrorObjectOwned::owned(
                    ErrorCode::InvalidParams.code(),
                    format!("Invalid params: {}", e),
                    None::<()>,
                )
            })?;

        debug!("get_balance rpc request received: {:?}", pubkey_str);

        // Validate pubkey (same validation as current implementation)
        let pubkey = verify_pubkey(&pubkey_str).map_err(|e| {
            ErrorObjectOwned::owned(
                ErrorCode::InvalidParams.code(),
                format!("Invalid pubkey: {}", e),
                None::<()>,
            )
        })?;

        // Call processor method (same business logic as before)
        context
            .get_balance(&pubkey, config.unwrap_or_default())
            .map_err(|e| {
                ErrorObjectOwned::owned(ErrorCode::InternalError.code(), e.to_string(), None::<()>)
            })
    })?;

    // Future methods will be registered here:

    Ok(module)
}

/// Start jsonrpsee server within existing Tokio runtime
///
/// This function builds and starts a jsonrpsee server that:
/// - Uses the current Tokio runtime context automatically
/// - Integrates Tower services for middleware
/// - Supports the same RPC methods as the legacy jsonrpc-core implementation
///
/// # Arguments
/// * `rpc_addr` - Socket address to bind the server to
/// * `processor` - JsonRpcRequestProcessor with all the RPC business logic
/// * `max_request_body_size` - Maximum size of request body in bytes
/// * `max_response_body_size` - Maximum size of response body in bytes
///
/// # Returns
/// ServerHandle that can be used to stop the server
pub async fn start_jsonrpsee_server(
    rpc_addr: SocketAddr,
    processor: JsonRpcRequestProcessor,
    _max_request_body_size: u32,
    _max_response_body_size: u32,
) -> Result<ServerHandle, Box<dyn std::error::Error>> {
    let server = ServerBuilder::new().build(rpc_addr).await?;

    let module = build_rpc_module(processor)?;

    let handle = server.start(module);

    info!("jsonrpsee RPC server started on {}", rpc_addr);

    Ok(handle)
}
