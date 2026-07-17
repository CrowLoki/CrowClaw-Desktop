use crate::agent::CancellationToken;

use super::{ActionId, MemorySearchMatch, RememberedMemory, ToolError};

/// The local memory boundary used by approval-gated agent tools.
///
/// Implementations are called only after the executor atomically consumes an
/// approved token. Proposal parsing and denial never receive this backend.
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
}
