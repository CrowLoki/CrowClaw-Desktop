use super::evolution_types::{
    bounded, EvolutionDraft, EvolutionEvaluation, EvolutionFeedback, EvolutionObservation,
    EvolutionProposal, EvolutionRevision, EvolutionSnapshot, INSTRUCTION_BYTES,
    REFLECTION_CONTEXT_BYTES, RESPONSE_BYTES,
};
use super::{now_ms, Storage, StorageError, StorageResult, StoredTask};
use rusqlite::{params, Connection, OptionalExtension, Row, TransactionBehavior};
use std::collections::HashSet;
use uuid::Uuid;

const PROPOSAL_COLUMNS:&str="id,base_revision,title,rationale,instructions,source_task_ids_json,model,reported_model,reflection_context_json,status,created_at_ms,decided_at_ms,applied_revision";
const EVALUATION_COLUMNS:&str="id,proposal_id,baseline_revision,model,baseline_model,candidate_model,candidate_instructions,prompt,baseline_response,candidate_response,preference,created_at_ms";

impl Storage {
    pub fn evolution_active(&self) -> StorageResult<EvolutionRevision> {
        let connection = self.connection()?;
        active_from(&connection)
    }
    pub fn evolution_revision(&self, revision: u32) -> StorageResult<EvolutionRevision> {
        let connection = self.connection()?;
        revision_from(&connection, revision)
    }
    pub fn evolution_proposal(&self, id: &str) -> StorageResult<EvolutionProposal> {
        let connection = self.connection()?;
        proposal_from(&connection, id)
    }

    pub fn evolution_snapshot(&self) -> StorageResult<EvolutionSnapshot> {
        let mut handle = self.connection()?;
        let transaction = handle.transaction_with_behavior(TransactionBehavior::Deferred)?;
        let connection: &Connection = &transaction;
        let active = active_from(&connection)?;
        let mut observations=connection.prepare(
            "SELECT t.id,json_extract(t.payload_json,'$.title'),t.status,json_extract(t.payload_json,'$.guidelineRevision'),t.updated_at_ms,f.rating,f.note,f.updated_at_ms FROM tasks t LEFT JOIN evolution_feedback f ON f.task_id=t.id WHERE t.kind='agent-turn' AND t.status IN ('succeeded','failed','cancelled') ORDER BY t.updated_at_ms DESC,t.id LIMIT 50")?;
        let observations = observations
            .query_map([], |row| {
                let rating: Option<String> = row.get(5)?;
                Ok(EvolutionObservation {
                    task_id: row.get(0)?,
                    title: row
                        .get::<_, Option<String>>(1)?
                        .unwrap_or_else(|| "Agent task".into()),
                    outcome: row.get(2)?,
                    guideline_revision: row.get(3)?,
                    updated_at_ms: row.get(4)?,
                    feedback: match rating {
                        Some(rating) => Some(EvolutionFeedback {
                            rating,
                            note: row.get(6)?,
                            updated_at_ms: row.get(7)?,
                        }),
                        None => None,
                    },
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut proposals=connection.prepare(&format!("SELECT {PROPOSAL_COLUMNS} FROM evolution_proposals ORDER BY created_at_ms DESC,id LIMIT 100"))?;
        let proposals = proposals
            .query_map([], proposal_row)?
            .collect::<Result<Vec<_>, _>>()?;
        let mut revisions=connection.prepare("SELECT revision,title,instructions,reason,created_at_ms FROM evolution_revisions ORDER BY revision DESC LIMIT 100")?;
        let revisions = revisions
            .query_map([], revision_row)?
            .collect::<Result<Vec<_>, _>>()?;
        let mut evaluations=connection.prepare(&format!("SELECT {EVALUATION_COLUMNS} FROM evolution_evaluations ORDER BY created_at_ms DESC,id LIMIT 100"))?;
        let evaluations = evaluations
            .query_map([], evaluation_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(EvolutionSnapshot {
            active,
            observations,
            proposals,
            revisions,
            evaluations,
        })
    }

    pub fn evolution_feedback(&self, task_id: &str, rating: &str, note: &str) -> StorageResult<()> {
        if !matches!(rating, "useful" | "needs_improvement" | "uncertain") {
            return Err(StorageError::InvalidData(
                "Choose a valid outcome rating".into(),
            ));
        }
        bounded("Feedback note", note, 2048, true)?;
        let now = now_ms()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_outcome(&transaction, task_id)?;
        transaction.execute("INSERT INTO evolution_feedback VALUES(?1,?2,?3,?4) ON CONFLICT(task_id) DO UPDATE SET rating=excluded.rating,note=excluded.note,updated_at_ms=excluded.updated_at_ms",params![task_id,rating,note,now])?;
        transaction.commit()?;
        Ok(())
    }

    pub fn evolution_reflection_source(
        &self,
        task_id: &str,
    ) -> StorageResult<(StoredTask, Option<EvolutionFeedback>)> {
        {
            let connection = self.connection()?;
            require_outcome(&connection, task_id)?;
        }
        let task = self
            .get_task(task_id)?
            .ok_or_else(|| StorageError::not_found("task", task_id))?;
        let connection = self.connection()?;
        let feedback = connection
            .query_row(
                "SELECT rating,note,updated_at_ms FROM evolution_feedback WHERE task_id=?1",
                [task_id],
                |r| {
                    Ok(EvolutionFeedback {
                        rating: r.get(0)?,
                        note: r.get(1)?,
                        updated_at_ms: r.get(2)?,
                    })
                },
            )
            .optional()?;
        Ok((task, feedback))
    }

    pub fn evolution_draft(
        &self,
        draft: &EvolutionDraft,
        model: Option<&str>,
        request_id: Option<&str>,
        reported_model: Option<&str>,
        reflection_context: Option<&serde_json::Value>,
    ) -> StorageResult<EvolutionProposal> {
        bounded("Proposal title", &draft.title, 160, false)?;
        bounded("Proposal rationale", &draft.rationale, 2048, false)?;
        bounded(
            "Working guidelines",
            &draft.instructions,
            INSTRUCTION_BYTES,
            false,
        )?;
        if let Some(model) = model {
            bounded("Model identifier", model, 256, false)?;
        }
        if let Some(model) = reported_model {
            bounded("Reported model identifier", model, 256, false)?;
        }
        let reflection_json = reflection_context.map(serde_json::to_string).transpose()?;
        if let Some(context) = &reflection_json {
            bounded(
                "Reflection context",
                context,
                REFLECTION_CONTEXT_BYTES,
                false,
            )?;
        }
        if draft.source_task_ids.len() > 8
            || draft.source_task_ids.iter().collect::<HashSet<_>>().len()
                != draft.source_task_ids.len()
        {
            return Err(StorageError::InvalidData(
                "Select at most eight distinct source tasks".into(),
            ));
        }
        let id = Uuid::new_v4().to_string();
        let now = now_ms()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        revision_from(&transaction, draft.base_revision)?;
        for task in &draft.source_task_ids {
            require_outcome(&transaction, task)?;
        }
        transaction.execute("INSERT INTO evolution_proposals(id,base_revision,title,rationale,instructions,source_task_ids_json,model,reported_model,reflection_context_json,status,created_at_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'draft',?10)",params![id,draft.base_revision,draft.title,draft.rationale,draft.instructions,serde_json::to_string(&draft.source_task_ids)?,model,reported_model,reflection_json,now])?;
        let proposal = proposal_from(&transaction, &id)?;
        if let Some(request_id) = request_id {
            finish_request_from(
                &transaction,
                request_id,
                "evolution-reflection",
                &serde_json::json!({"proposalId":id,"detail":"Working guideline proposal is ready for review"}),
                now,
            )?;
        }
        transaction.commit()?;
        Ok(proposal)
    }

    pub fn evolution_decide(
        &self,
        id: &str,
        apply: bool,
        instructions: &str,
        expected: u32,
    ) -> StorageResult<EvolutionProposal> {
        if apply {
            bounded("Working guidelines", instructions, INSTRUCTION_BYTES, false)?;
        }
        let now = now_ms()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let proposal = proposal_from(&transaction, id)?;
        if proposal.status != "draft" {
            return Err(StorageError::Conflict(
                "This proposal has already been decided".into(),
            ));
        }
        let revision = if apply {
            let current = active_from(&transaction)?;
            if current.revision != expected || proposal.base_revision != expected {
                return Err(StorageError::Conflict("Working guidelines changed since this proposal was based on them. Review a new proposal against the current revision.".into()));
            }
            let next = insert_revision(
                &transaction,
                &current,
                &proposal.title,
                instructions,
                &proposal.rationale,
                now,
            )?;
            transaction.execute(
                "UPDATE evolution_head SET revision=?1 WHERE id=1 AND revision=?2",
                params![next.revision, expected],
            )?;
            Some(next.revision)
        } else {
            None
        };
        transaction.execute("UPDATE evolution_proposals SET status=?2,decided_at_ms=?3,applied_revision=?4,instructions=?5 WHERE id=?1",params![id,if apply {"applied"} else {"rejected"},now,revision,if apply {instructions} else {&proposal.instructions}])?;
        let proposal = proposal_from(&transaction, id)?;
        transaction.commit()?;
        Ok(proposal)
    }

    pub fn evolution_restore(
        &self,
        revision: u32,
        expected: u32,
    ) -> StorageResult<EvolutionRevision> {
        let now = now_ms()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = active_from(&transaction)?;
        if current.revision != expected {
            return Err(StorageError::Conflict(
                "Working guidelines changed; refresh before restoring a revision".into(),
            ));
        }
        let target = revision_from(&transaction, revision)?;
        let restored = insert_revision(
            &transaction,
            &current,
            &format!("Restore revision {revision}"),
            &target.instructions,
            &format!("Restored the user-approved guidelines from revision {revision}"),
            now,
        )?;
        transaction.execute(
            "UPDATE evolution_head SET revision=?1 WHERE id=1 AND revision=?2",
            params![restored.revision, expected],
        )?;
        transaction.commit()?;
        Ok(restored)
    }

    pub fn evolution_record_evaluation(
        &self,
        evaluation: &EvolutionEvaluation,
        request_id: Option<&str>,
    ) -> StorageResult<EvolutionEvaluation> {
        bounded("Comparison prompt", &evaluation.prompt, 4096, false)?;
        bounded(
            "Candidate guidelines",
            &evaluation.candidate_instructions,
            INSTRUCTION_BYTES,
            false,
        )?;
        bounded(
            "Baseline response",
            &evaluation.baseline_response,
            RESPONSE_BYTES,
            false,
        )?;
        bounded(
            "Candidate response",
            &evaluation.candidate_response,
            RESPONSE_BYTES,
            false,
        )?;
        bounded("Model identifier", &evaluation.model, 256, false)?;
        for model in [&evaluation.baseline_model, &evaluation.candidate_model]
            .into_iter()
            .flatten()
        {
            bounded("Reported model identifier", model, 256, false)?;
        }
        if evaluation.preference.is_some() {
            return Err(StorageError::InvalidData(
                "A generated comparison cannot assign the user's preference".into(),
            ));
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let proposal = proposal_from(&transaction, &evaluation.proposal_id)?;
        if proposal.status != "draft" || proposal.base_revision != evaluation.baseline_revision {
            return Err(StorageError::Conflict(
                "Proposal changed while the comparison ran".into(),
            ));
        }
        revision_from(&transaction, evaluation.baseline_revision)?;
        transaction.execute(
            "INSERT INTO evolution_evaluations VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,NULL,?11)",
            params![
                evaluation.id,
                evaluation.proposal_id,
                evaluation.baseline_revision,
                evaluation.model,
                evaluation.baseline_model,
                evaluation.candidate_model,
                evaluation.candidate_instructions,
                evaluation.prompt,
                evaluation.baseline_response,
                evaluation.candidate_response,
                evaluation.created_at_ms
            ],
        )?;
        if let Some(request_id) = request_id {
            finish_request_from(
                &transaction,
                request_id,
                "evolution-comparison",
                &serde_json::json!({"evaluationId":evaluation.id,"detail":"Baseline and candidate responses are ready for review"}),
                evaluation.created_at_ms,
            )?;
        }
        transaction.commit()?;
        Ok(evaluation.clone())
    }

    pub fn evolution_rate(&self, id: &str, preference: &str) -> StorageResult<()> {
        if !matches!(preference, "baseline" | "candidate" | "tie" | "neither") {
            return Err(StorageError::InvalidData(
                "Choose a valid comparison preference".into(),
            ));
        }
        let connection = self.connection()?;
        if connection.execute(
            "UPDATE evolution_evaluations SET preference=?2 WHERE id=?1",
            params![id, preference],
        )? != 1
        {
            return Err(StorageError::not_found("evaluation", id));
        }
        Ok(())
    }

    pub fn evolution_export(&self) -> StorageResult<serde_json::Value> {
        let connection = self.connection()?;
        export_from(&connection)
    }
}

pub(crate) fn export_from(connection: &Connection) -> StorageResult<serde_json::Value> {
    let mut proposals = connection.prepare(&format!(
        "SELECT {PROPOSAL_COLUMNS} FROM evolution_proposals ORDER BY created_at_ms,id"
    ))?;
    let proposals = proposals
        .query_map([], proposal_row)?
        .collect::<Result<Vec<_>, _>>()?;
    let mut revisions=connection.prepare("SELECT revision,title,instructions,reason,created_at_ms FROM evolution_revisions ORDER BY revision")?;
    let revisions = revisions
        .query_map([], revision_row)?
        .collect::<Result<Vec<_>, _>>()?;
    let mut evaluations = connection.prepare(&format!(
        "SELECT {EVALUATION_COLUMNS} FROM evolution_evaluations ORDER BY created_at_ms,id"
    ))?;
    let evaluations = evaluations
        .query_map([], evaluation_row)?
        .collect::<Result<Vec<_>, _>>()?;
    let mut feedback = connection.prepare(
        "SELECT task_id,rating,note,updated_at_ms FROM evolution_feedback ORDER BY task_id",
    )?;
    let feedback=feedback.query_map([],|r|Ok(serde_json::json!({"taskId":r.get::<_,String>(0)?,"rating":r.get::<_,String>(1)?,"note":r.get::<_,String>(2)?,"updatedAtMs":r.get::<_,i64>(3)?})))?.collect::<Result<Vec<_>,_>>()?;
    Ok(
        serde_json::json!({"active":active_from(&connection)?,"proposals":proposals,"revisions":revisions,"evaluations":evaluations,"feedback":feedback}),
    )
}

fn require_outcome(connection: &Connection, id: &str) -> StorageResult<()> {
    let record: Option<(String, String)> = connection
        .query_row("SELECT kind,status FROM tasks WHERE id=?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()?;
    match record {
        Some((kind, status))
            if kind == "agent-turn"
                && matches!(status.as_str(), "succeeded" | "failed" | "cancelled") =>
        {
            Ok(())
        }
        Some(_) => Err(StorageError::InvalidData(
            "Feedback and reflection require a finished agent task".into(),
        )),
        None => Err(StorageError::not_found("task", id)),
    }
}

fn finish_request_from(
    connection: &Connection,
    id: &str,
    kind: &str,
    result: &serde_json::Value,
    now: i64,
) -> StorageResult<()> {
    let changed=connection.execute("UPDATE tasks SET status='succeeded',result_json=?3,error=NULL,updated_at_ms=?4,completed_at_ms=?4 WHERE id=?1 AND kind=?2 AND status='running' AND cancellation_requested=0",params![id,kind,serde_json::to_string(result)?,now])?;
    if changed != 1 {
        return Err(StorageError::Conflict(
            "Evolution request ended before its result could be published".into(),
        ));
    }
    Ok(())
}

fn active_from(connection: &Connection) -> StorageResult<EvolutionRevision> {
    Ok(connection.query_row("SELECT r.revision,r.title,r.instructions,r.reason,r.created_at_ms FROM evolution_revisions r JOIN evolution_head h ON h.revision=r.revision WHERE h.id=1",[],revision_row)?)
}
fn revision_from(connection: &Connection, revision: u32) -> StorageResult<EvolutionRevision> {
    Ok(connection.query_row("SELECT revision,title,instructions,reason,created_at_ms FROM evolution_revisions WHERE revision=?1",[revision],revision_row)?)
}
fn proposal_from(connection: &Connection, id: &str) -> StorageResult<EvolutionProposal> {
    connection
        .query_row(
            &format!("SELECT {PROPOSAL_COLUMNS} FROM evolution_proposals WHERE id=?1"),
            [id],
            proposal_row,
        )
        .optional()?
        .ok_or_else(|| StorageError::not_found("proposal", id))
}
fn insert_revision(
    connection: &Connection,
    current: &EvolutionRevision,
    title: &str,
    instructions: &str,
    reason: &str,
    now: i64,
) -> StorageResult<EvolutionRevision> {
    let revision = current
        .revision
        .checked_add(1)
        .filter(|n| *n <= 2147483647)
        .ok_or_else(|| StorageError::Conflict("Guideline revision counter exhausted".into()))?;
    connection.execute(
        "INSERT INTO evolution_revisions VALUES(?1,?2,?3,?4,?5)",
        params![revision, title, instructions, reason, now],
    )?;
    Ok(EvolutionRevision {
        revision,
        title: title.into(),
        instructions: instructions.into(),
        reason: reason.into(),
        created_at_ms: now,
    })
}
fn revision_row(row: &Row<'_>) -> rusqlite::Result<EvolutionRevision> {
    Ok(EvolutionRevision {
        revision: row.get(0)?,
        title: row.get(1)?,
        instructions: row.get(2)?,
        reason: row.get(3)?,
        created_at_ms: row.get(4)?,
    })
}
fn proposal_row(row: &Row<'_>) -> rusqlite::Result<EvolutionProposal> {
    let ids: String = row.get(5)?;
    let ids = serde_json::from_str(&ids).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let context: Option<String> = row.get(8)?;
    let context = context
        .map(|value| serde_json::from_str(&value))
        .transpose()
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(e))
        })?;
    Ok(EvolutionProposal {
        id: row.get(0)?,
        base_revision: row.get(1)?,
        title: row.get(2)?,
        rationale: row.get(3)?,
        instructions: row.get(4)?,
        source_task_ids: ids,
        model: row.get(6)?,
        reported_model: row.get(7)?,
        reflection_context: context,
        status: row.get(9)?,
        created_at_ms: row.get(10)?,
        decided_at_ms: row.get(11)?,
        applied_revision: row.get(12)?,
    })
}
fn evaluation_row(row: &Row<'_>) -> rusqlite::Result<EvolutionEvaluation> {
    Ok(EvolutionEvaluation {
        id: row.get(0)?,
        proposal_id: row.get(1)?,
        baseline_revision: row.get(2)?,
        model: row.get(3)?,
        baseline_model: row.get(4)?,
        candidate_model: row.get(5)?,
        candidate_instructions: row.get(6)?,
        prompt: row.get(7)?,
        baseline_response: row.get(8)?,
        candidate_response: row.get(9)?,
        preference: row.get(10)?,
        created_at_ms: row.get(11)?,
    })
}
