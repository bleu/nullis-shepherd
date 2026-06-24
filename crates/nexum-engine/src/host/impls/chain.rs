//! `nexum:host/chain`: raw JSON-RPC dispatch over alloy.

use std::time::Instant;

use crate::bindings::HostError;
use crate::bindings::nexum;
use crate::bindings::nexum::host::types::HostErrorKind;
use crate::host::error::internal_error;
use crate::host::provider_pool::ProviderError;
use crate::host::state::HostState;

impl nexum::host::chain::Host for HostState {
    async fn request(
        &mut self,
        chain_id: u64,
        method: String,
        params: String,
    ) -> Result<String, HostError> {
        let start = Instant::now();
        tracing::debug!(chain_id, %method, "chain::request");
        let method_label = method.clone();
        let result = match self.chain.request(chain_id, method, params).await {
            Ok(body) => Ok(body),
            Err(ProviderError::UnknownChain(id)) => Err(HostError {
                domain: "chain".into(),
                kind: HostErrorKind::Unsupported,
                code: 0,
                message: format!("chain {id} has no engine.toml RPC entry"),
                data: None,
            }),
            Err(err @ ProviderError::InvalidParams { .. }) => Err(HostError {
                domain: "chain".into(),
                kind: HostErrorKind::InvalidInput,
                code: -32602,
                message: err.to_string(),
                data: None,
            }),
            Err(err @ ProviderError::Rpc { .. }) => Err(HostError {
                domain: "chain".into(),
                kind: HostErrorKind::Internal,
                code: -32603,
                message: err.to_string(),
                data: None,
            }),
            Err(err) => Err(internal_error("chain", err.to_string())),
        };
        tracing::trace!(elapsed_ms = ?start.elapsed(), "chain::request done");
        let outcome = if result.is_ok() { "ok" } else { "err" };
        metrics::counter!(
            "shepherd_chain_request_total",
            "chain_id" => chain_id.to_string(),
            "method" => method_label,
            "outcome" => outcome,
        )
        .increment(1);
        result
    }

    async fn request_batch(
        &mut self,
        chain_id: u64,
        requests: Vec<nexum::host::chain::RpcRequest>,
    ) -> Result<Vec<nexum::host::chain::RpcResult>, HostError> {
        let start = Instant::now();
        tracing::debug!(chain_id, count = requests.len(), "chain::request-batch");
        let mut out = Vec::with_capacity(requests.len());
        for req in requests {
            match nexum::host::chain::Host::request(self, chain_id, req.method, req.params).await {
                Ok(s) => out.push(nexum::host::chain::RpcResult::Ok(s)),
                Err(e) => out.push(nexum::host::chain::RpcResult::Err(e)),
            }
        }
        tracing::trace!(elapsed_ms = ?start.elapsed(), "chain::request-batch done");
        Ok(out)
    }
}
