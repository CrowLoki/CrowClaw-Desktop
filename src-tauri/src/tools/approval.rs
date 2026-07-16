use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard},
};

use super::{
    ActionId, ApprovalDecision, ApprovalStatus, ApprovalToken, ProposedAction, ToolError,
    ToolRequest,
};

#[derive(Debug, Default)]
pub(crate) struct ApprovalRegistry {
    records: Mutex<HashMap<ApprovalToken, ApprovalRecord>>,
}

#[derive(Clone, Debug)]
struct ApprovalRecord {
    proposal: ProposedAction,
    status: ApprovalStatus,
}

pub(crate) enum ClaimedAction {
    Execute(ProposedAction),
    Denied(ProposedAction, String),
}

impl ApprovalRegistry {
    pub(crate) fn propose(&self, request: ToolRequest) -> Result<ProposedAction, ToolError> {
        let proposal = ProposedAction {
            action_id: ActionId::new(),
            approval_token: ApprovalToken::new(),
            tool_name: request.tool_name().into(),
            summary: request.summary(),
            request,
        };

        self.lock()?.insert(
            proposal.approval_token.clone(),
            ApprovalRecord {
                proposal: proposal.clone(),
                status: ApprovalStatus::Pending,
            },
        );
        Ok(proposal)
    }

    pub(crate) fn resolve(
        &self,
        token: &ApprovalToken,
        decision: ApprovalDecision,
    ) -> Result<ApprovalStatus, ToolError> {
        let mut records = self.lock()?;
        let record = records
            .get_mut(token)
            .ok_or_else(|| ToolError::ApprovalNotFound {
                token: token.to_string(),
            })?;

        if record.status != ApprovalStatus::Pending {
            return Err(ToolError::ApprovalAlreadyResolved {
                token: token.to_string(),
                state: status_name(&record.status).into(),
            });
        }

        record.status = match decision {
            ApprovalDecision::Approve => ApprovalStatus::Approved,
            ApprovalDecision::Deny { reason } => ApprovalStatus::Denied {
                reason: reason.unwrap_or_else(|| "Denied by the user".into()),
            },
        };
        Ok(record.status.clone())
    }

    pub(crate) fn status(&self, token: &ApprovalToken) -> Result<ApprovalStatus, ToolError> {
        self.lock()?
            .get(token)
            .map(|record| record.status.clone())
            .ok_or_else(|| ToolError::ApprovalNotFound {
                token: token.to_string(),
            })
    }

    pub(crate) fn claim(&self, token: &ApprovalToken) -> Result<ClaimedAction, ToolError> {
        let mut records = self.lock()?;
        let record = records
            .get_mut(token)
            .ok_or_else(|| ToolError::ApprovalNotFound {
                token: token.to_string(),
            })?;

        let claimed = match &record.status {
            ApprovalStatus::Pending => {
                return Err(ToolError::ApprovalPending {
                    token: token.to_string(),
                })
            }
            ApprovalStatus::Approved => ClaimedAction::Execute(record.proposal.clone()),
            ApprovalStatus::Denied { reason } => {
                ClaimedAction::Denied(record.proposal.clone(), reason.clone())
            }
            ApprovalStatus::Consumed => {
                return Err(ToolError::ApprovalAlreadyConsumed {
                    token: token.to_string(),
                })
            }
        };

        record.status = ApprovalStatus::Consumed;
        Ok(claimed)
    }

    fn lock(&self) -> Result<MutexGuard<'_, HashMap<ApprovalToken, ApprovalRecord>>, ToolError> {
        self.records
            .lock()
            .map_err(|_| ToolError::ApprovalStateUnavailable)
    }
}

fn status_name(status: &ApprovalStatus) -> &'static str {
    match status {
        ApprovalStatus::Pending => "pending",
        ApprovalStatus::Approved => "approved",
        ApprovalStatus::Denied { .. } => "denied",
        ApprovalStatus::Consumed => "consumed",
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{ApprovalRegistry, ClaimedAction};
    use crate::tools::{ApprovalDecision, ApprovalStatus, ToolRequest};

    #[test]
    fn approval_is_single_use_and_cannot_be_changed() {
        let registry = ApprovalRegistry::default();
        let proposal = registry
            .propose(ToolRequest::ReadTextFile {
                path: PathBuf::from("fixture.txt"),
            })
            .unwrap();

        assert_eq!(
            registry.status(&proposal.approval_token).unwrap(),
            ApprovalStatus::Pending
        );
        registry
            .resolve(&proposal.approval_token, ApprovalDecision::Approve)
            .unwrap();
        assert!(registry
            .resolve(
                &proposal.approval_token,
                ApprovalDecision::Deny { reason: None }
            )
            .is_err());
        assert!(matches!(
            registry.claim(&proposal.approval_token).unwrap(),
            ClaimedAction::Execute(_)
        ));
        assert!(registry.claim(&proposal.approval_token).is_err());
    }
}
