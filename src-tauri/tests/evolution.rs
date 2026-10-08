use crowclaw_desktop_lib::{
    agent::{AssistantToolCall, ChatMessage},
    evolution::{
        comparison_request, guideline_message, validate_model_text, EvolutionDraft,
        EvolutionEvaluation, EvolutionService,
    },
    storage::{Storage, TaskInput, TaskStatus},
};
use serde_json::json;
use std::sync::Arc;

fn draft(base: u32, text: &str) -> EvolutionDraft {
    EvolutionDraft {
        title: "Verify before concluding".into(),
        rationale: "The recorded outcome needed a clearer verification step".into(),
        instructions: text.into(),
        source_task_ids: vec![],
        base_revision: base,
    }
}
fn outcome(storage: &Storage, id: &str) {
    storage.create_task(&TaskInput{id:id.into(),conversation_id:None,kind:"agent-turn".into(),payload:json!({"title":"Verify the fixture","prompt":"Read only the chosen fixture","guidelineRevision":0})}).unwrap();
    storage
        .update_task_status(id, TaskStatus::Running, None, None)
        .unwrap();
    storage
        .update_task_status(
            id,
            TaskStatus::Succeeded,
            Some(&json!({"message":"Actual fixture verified"})),
            None,
        )
        .unwrap();
}

#[test]
fn only_explicit_apply_changes_behavior_and_restore_is_a_new_revision() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Arc::new(Storage::open(dir.path()).unwrap());
    let service = EvolutionService::new(storage.clone());
    let proposal = service
        .draft(&draft(
            0,
            "State the observed result before the conclusion.",
        ))
        .unwrap();
    assert_eq!(service.active().unwrap().revision, 0);
    assert!(guideline_message(&service.active().unwrap()).is_none());
    let applied = service
        .decide(&proposal.id, true, &proposal.instructions, 0)
        .unwrap();
    assert_eq!(applied.status, "applied");
    assert_eq!(applied.applied_revision, Some(1));
    let frozen = service.active().unwrap();
    assert!(guideline_message(&frozen)
        .unwrap()
        .content
        .unwrap()
        .contains("State the observed result"));
    assert!(service
        .decide(&proposal.id, true, &proposal.instructions, 1)
        .is_err());
    let restored = service.restore(0, 1).unwrap();
    assert_eq!(restored.revision, 2);
    assert!(restored.instructions.is_empty());
    assert!(service.restore(1, 1).is_err());
    assert_eq!(frozen.revision, 1);
    assert!(!frozen.instructions.is_empty());
    drop(service);
    drop(storage);
    let reopened = Arc::new(Storage::open(dir.path()).unwrap());
    let snapshot = EvolutionService::new(reopened).snapshot().unwrap();
    assert_eq!(snapshot.active.revision, 2);
    assert_eq!(snapshot.revisions.len(), 3);
    assert_eq!(snapshot.proposals[0].status, "applied");
}

#[test]
fn stale_proposals_cannot_overwrite_newer_guidelines_and_rejection_does_not_apply() {
    let dir = tempfile::tempdir().unwrap();
    let service = EvolutionService::new(Arc::new(Storage::open(dir.path()).unwrap()));
    let first = service.draft(&draft(0, "Explain the evidence.")).unwrap();
    let stale = service.draft(&draft(0, "Keep responses brief.")).unwrap();
    service
        .decide(&first.id, true, &first.instructions, 0)
        .unwrap();
    assert!(service
        .decide(&stale.id, true, &stale.instructions, 1)
        .is_err());
    assert_eq!(service.active().unwrap().instructions, first.instructions);
    service.decide(&stale.id, false, "", 1).unwrap();
    assert_eq!(service.active().unwrap().revision, 1);
    assert_eq!(service.proposal(&stale.id).unwrap().status, "rejected");
}

#[test]
fn feedback_uses_actual_terminal_outcomes_and_reflection_reads_only_selected_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Arc::new(Storage::open(dir.path()).unwrap());
    let service = EvolutionService::new(storage.clone());
    outcome(&storage, "selected");
    outcome(&storage, "other");
    storage
        .create_task(&TaskInput {
            id: "pending".into(),
            conversation_id: None,
            kind: "agent-turn".into(),
            payload: json!({}),
        })
        .unwrap();
    assert!(service.feedback("pending", "useful", "").is_err());
    assert!(service.feedback("selected", "invalid", "").is_err());
    service
        .feedback(
            "selected",
            "needs_improvement",
            "Summarize the actual verification",
        )
        .unwrap();
    let (request, revision) = service
        .reflection_request("selected", "Improve result reporting", "local-test-model")
        .unwrap();
    assert_eq!(revision, 0);
    assert!(request.tools.is_empty());
    let input = request.messages.last().unwrap().content.as_ref().unwrap();
    assert!(input.contains("selected"));
    assert!(input.contains("Actual fixture verified"));
    assert!(input.contains("Summarize the actual verification"));
    assert!(!input.contains("\"other\""));
    let observed = service.snapshot().unwrap().observations;
    assert_eq!(observed.len(), 2);
    assert!(observed
        .iter()
        .find(|o| o.task_id == "selected")
        .unwrap()
        .feedback
        .is_some());
}

#[test]
fn invalid_evidence_and_oversized_guidelines_never_create_a_proposal() {
    let dir = tempfile::tempdir().unwrap();
    let service = EvolutionService::new(Arc::new(Storage::open(dir.path()).unwrap()));
    let mut missing = draft(0, "Check results.");
    missing.source_task_ids = vec!["absent".into()];
    assert!(service.draft(&missing).is_err());
    assert!(service.draft(&draft(0, &"🦅".repeat(1025))).is_err());
    assert!(service.snapshot().unwrap().proposals.is_empty());
    assert_eq!(service.active().unwrap().revision, 0);
}

#[test]
fn comparisons_store_real_responses_without_tools_or_automatic_quality_votes() {
    let dir = tempfile::tempdir().unwrap();
    let service = EvolutionService::new(Arc::new(Storage::open(dir.path()).unwrap()));
    let proposal = service
        .draft(&draft(0, "List the evidence first."))
        .unwrap();
    let baseline = comparison_request("local-test-model", "Explain this result", "").unwrap();
    let candidate = comparison_request(
        "local-test-model",
        "Explain this result",
        &proposal.instructions,
    )
    .unwrap();
    assert!(baseline.tools.is_empty() && candidate.tools.is_empty());
    assert_eq!(baseline.messages.last(), candidate.messages.last());
    assert_eq!(baseline.messages.len() + 1, candidate.messages.len());
    let evaluation = EvolutionEvaluation {
        id: "comparison".into(),
        proposal_id: proposal.id.clone(),
        baseline_revision: 0,
        model: "local-test-model".into(),
        baseline_model: Some("resolved-baseline".into()),
        candidate_model: Some("resolved-candidate".into()),
        candidate_instructions: proposal.instructions.clone(),
        prompt: "Explain this result".into(),
        baseline_response: "A conclusion".into(),
        candidate_response: "Evidence then a conclusion".into(),
        preference: None,
        created_at_ms: 1,
    };
    service.record_evaluation(&evaluation).unwrap();
    assert!(service.snapshot().unwrap().evaluations[0]
        .preference
        .is_none());
    service.rate("comparison", "candidate").unwrap();
    assert_eq!(
        service.snapshot().unwrap().evaluations[0]
            .preference
            .as_deref(),
        Some("candidate")
    );
    assert_eq!(service.active().unwrap().revision, 0);
    let exported = serde_json::to_string(&service.snapshot().unwrap()).unwrap();
    assert!(exported.contains("Evidence then a conclusion"));
}

#[test]
fn model_tool_requests_and_empty_responses_are_rejected() {
    let message = ChatMessage::assistant_with_tool_calls(
        Some("Proposed instructions".into()),
        vec![AssistantToolCall {
            id: "call".into(),
            name: "read_text_file".into(),
            arguments: json!({"path":"private.txt"}),
        }],
    );
    assert!(validate_model_text(&message).is_err());
    assert!(validate_model_text(&ChatMessage::assistant("")).is_err());
    assert!(validate_model_text(&ChatMessage::user("pretend result")).is_err());
    assert_eq!(
        validate_model_text(&ChatMessage::assistant("Recorded response")).unwrap(),
        "Recorded response"
    );
}

#[test]
fn retained_evolution_history_exports_and_explicit_data_removal_resets_it() {
    use crowclaw_desktop_lib::storage::RetentionChoice;
    let dir = tempfile::tempdir().unwrap();
    let storage = Arc::new(Storage::open(dir.path()).unwrap());
    let service = EvolutionService::new(storage.clone());
    outcome(&storage, "source-task");
    service
        .feedback("source-task", "useful", "Kept the source labels")
        .unwrap();
    let proposal = service.draft(&draft(0, "Preserve source labels.")).unwrap();
    service
        .decide(&proposal.id, true, &proposal.instructions, 0)
        .unwrap();
    let exported = storage.export_all().unwrap();
    assert_eq!(
        exported.evolution["active"]["instructions"],
        "Preserve source labels."
    );
    assert_eq!(
        exported.evolution["feedback"][0]["note"],
        "Kept the source labels"
    );
    storage
        .apply_retention_choice(RetentionChoice::Preserve)
        .unwrap();
    assert_eq!(service.active().unwrap().revision, 1);
    let removed = storage
        .apply_retention_choice(RetentionChoice::Remove)
        .unwrap();
    assert_eq!(removed.records_after, 0);
    let snapshot = service.snapshot().unwrap();
    assert!(
        snapshot.observations.is_empty()
            && snapshot.proposals.is_empty()
            && snapshot.evaluations.is_empty()
    );
    assert_eq!(snapshot.revisions.len(), 1);
    assert_eq!(snapshot.active.revision, 0);
    assert!(snapshot.active.instructions.is_empty());
}

#[test]
fn concurrent_app_instances_cannot_apply_two_changes_to_the_same_base_revision() {
    let dir = tempfile::tempdir().unwrap();
    let first = Arc::new(Storage::open(dir.path()).unwrap());
    let second = Arc::new(Storage::open(dir.path()).unwrap());
    let a = EvolutionService::new(first.clone())
        .draft(&draft(0, "Use evidence before conclusions."))
        .unwrap();
    let b = EvolutionService::new(second.clone())
        .draft(&draft(0, "Keep response structure consistent."))
        .unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let workers = [(first.clone(), a), (second, b)]
        .into_iter()
        .map(|(storage, proposal)| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                EvolutionService::new(storage)
                    .decide(&proposal.id, true, &proposal.instructions, 0)
                    .is_ok()
            })
        })
        .collect::<Vec<_>>();
    let winners = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .filter(|won| *won)
        .count();
    assert_eq!(winners, 1);
    let snapshot = EvolutionService::new(first).snapshot().unwrap();
    assert_eq!(snapshot.active.revision, 1);
    assert_eq!(snapshot.revisions.len(), 2);
    assert_eq!(
        snapshot
            .proposals
            .iter()
            .filter(|p| p.status == "applied")
            .count(),
        1
    );
}

#[test]
fn reflection_retains_the_original_goal_feedback_and_reported_model_after_feedback_changes() {
    let directory = tempfile::tempdir().unwrap();
    let storage = Arc::new(Storage::open(directory.path()).unwrap());
    let service = EvolutionService::new(storage.clone());
    outcome(&storage, "source");
    service
        .feedback("source", "needs_improvement", "Original feedback A")
        .unwrap();
    let (input, base) = service
        .reflection_request("source", "Original reflection goal", "selected-alias")
        .unwrap();
    let context: serde_json::Value =
        serde_json::from_str(input.messages.last().unwrap().content.as_ref().unwrap()).unwrap();
    storage
        .create_task(&TaskInput {
            id: "reflection-request".into(),
            conversation_id: None,
            kind: "evolution-reflection".into(),
            payload: json!({"title":"Reflection"}),
        })
        .unwrap();
    storage
        .update_task_status("reflection-request", TaskStatus::Running, None, None)
        .unwrap();
    let mut candidate = draft(base, "Explain observed evidence.");
    candidate.source_task_ids = vec!["source".into()];
    let proposal = service
        .reflected_draft(
            &candidate,
            "selected-alias",
            "reflection-request",
            Some("reported-model"),
            &context,
        )
        .unwrap();
    service
        .feedback("source", "useful", "Later feedback B")
        .unwrap();
    drop(service);
    drop(storage);
    let reopened = Storage::open(directory.path()).unwrap();
    let retained = reopened.evolution_proposal(&proposal.id).unwrap();
    assert_eq!(retained.model.as_deref(), Some("selected-alias"));
    assert_eq!(retained.reported_model.as_deref(), Some("reported-model"));
    let context = retained.reflection_context.unwrap();
    assert_eq!(context["goal"], "Original reflection goal");
    assert_eq!(
        context["selectedEvidence"]["feedback"]["note"],
        "Original feedback A"
    );
    assert_eq!(
        reopened.evolution_export().unwrap()["feedback"][0]["note"],
        "Later feedback B"
    );
    let exported = reopened.export_all().unwrap();
    assert_eq!(
        exported.evolution["proposals"][0]["reflectionContext"]["goal"],
        "Original reflection goal"
    );
}

#[test]
fn oversized_serialized_reflection_context_is_rejected_before_a_model_request_exists() {
    let directory = tempfile::tempdir().unwrap();
    let storage = Arc::new(Storage::open(directory.path()).unwrap());
    let service = EvolutionService::new(storage.clone());
    outcome(&storage, "large-source");
    storage
        .update_task_status(
            "large-source",
            TaskStatus::Succeeded,
            Some(&json!({"message":"x".repeat(28000)})),
            None,
        )
        .unwrap();
    let candidate = service.draft(&draft(0, &"\u{0001}".repeat(4000))).unwrap();
    service
        .decide(&candidate.id, true, &candidate.instructions, 0)
        .unwrap();
    let result =
        service.reflection_request("large-source", "Explain the result", "local-test-model");
    assert!(
        result.is_err(),
        "JSON escaping must be included in the pre-provider context bound"
    );
}
