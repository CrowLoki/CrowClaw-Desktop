use crate::agent::CancellationToken;

use super::{ActionId, MemorySearchMatch, MemorySearchResponse, RememberedMemory, ToolError};

/// The local memory boundary used by approval-gated agent tools.
///
/// Implementations are called only after the executor atomically consumes an
/// approved token. Proposal parsing and denial never receive this backend.
#[async_trait::async_trait]
pub trait MemoryBackend: Send + Sync {
    fn remember(
        &self,
        action_id: &ActionId,
        text: &str,
        cancellation: &CancellationToken,
    ) -> Result<RememberedMemory, ToolError>;

    fn search(
        &self,
        query: &str,
        limit: usize,
        cancellation: &CancellationToken,
    ) -> Result<Vec<MemorySearchMatch>, ToolError>;

    async fn search_async(
        &self,
        query: &str,
        limit: usize,
        cancellation: &CancellationToken,
    ) -> Result<MemorySearchResponse, ToolError> {
        Ok(MemorySearchResponse {
            results: self.search(query, limit, cancellation)?,
            warnings: Vec::new(),
        })
    }
}
