import assert from "node:assert/strict";
import { DatabaseSync } from "node:sqlite";
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

// Verification-only: compare the model's observation with committed native data.
export function installedMemoryAudit(db, modelEvidence, sourceKind) {
  const note = "CI native telescope cobalt record";
  assert.equal(modelEvidence.violations, 0, "Model observed a memory boundary violation");
  assert.equal(modelEvidence.deniedWithoutDisclosure, 1);
  assert.equal(modelEvidence.approvedSources.length, 1);
  const actions = db.prepare("SELECT id,status,result_json FROM proposed_actions WHERE tool_name='search_memory' AND json_extract(request_json,'$.query')='cobalt' ORDER BY created_at_ms,id").all();
  assert.equal(actions.length, 2, "Expected exactly the two packaged memory-search actions");
  const denied = actions.filter(action => action.status === "denied");
  const approved = actions.filter(action => action.status === "succeeded");
  assert.equal(denied.length, 1);
  assert.equal(approved.length, 1);
  const deniedAudit = db.prepare("SELECT event_kind,detail_json FROM action_audit WHERE action_id=? ORDER BY sequence").all(denied[0].id);
  assert.ok(deniedAudit.length, "Denied action has no audit trail");
  assert.ok(!JSON.stringify({ action: denied[0], audit: deniedAudit }).includes(note), "Denied action/audit contains retained text");
  const result = JSON.parse(approved[0].result_json);
  assert.equal(result.state, "executed");
  assert.equal(result.output.type, "memory_search");
  const hit = result.output.results.find(item => item.text === note);
  assert.ok(hit, "Successful action did not retain its actual memory result");
  const source = db.prepare("SELECT id,origin_id,source_kind,authorship FROM memory_sources WHERE id=?").get(hit.provenance.sourceId);
  assert.ok(source, "Approved result refers to a missing source");
  assert.equal(source.source_kind, sourceKind);
  assert.equal(source.origin_id, hit.id);
  assert.equal(source.origin_id, hit.provenance.originId);
  assert.equal(source.authorship, hit.provenance.authorship);
  assert.equal(db.prepare("SELECT text FROM crowquant_memories WHERE id=?").get(source.origin_id)?.text, note);
  assert.deepEqual(modelEvidence.approvedSources[0], { id: hit.id, ...hit.provenance });
  const approvedAudit = db.prepare("SELECT detail_json FROM action_audit WHERE action_id=? ORDER BY sequence").all(approved[0].id);
  assert.ok(approvedAudit.some(row => row.detail_json.includes(note)), "Successful read lacks its source-bound audit result");
  return { deniedAction: denied[0].id, approvedAction: approved[0].id, sourceId: source.id, originId: source.origin_id, sourceKind, modelAndAuditAgree: true };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [database, evidence, sourceKind] = process.argv.slice(2);
  assert.ok(database && evidence && ["user_note", "legacy_crowquant"].includes(sourceKind), "Provide the owned database, observer receipt and source kind");
  const db = new DatabaseSync(database, { readOnly: true });
  try { console.log(JSON.stringify(installedMemoryAudit(db, JSON.parse(readFileSync(evidence, "utf8")), sourceKind))); }
  finally { db.close(); }
}
