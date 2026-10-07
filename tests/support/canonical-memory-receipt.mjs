import { createHash } from "node:crypto";
import { DatabaseSync } from "node:sqlite";
import { pathToFileURL } from "node:url";

// Verification tooling only, never an installed application dependency.
// Cast integers in SQLite so even 64-bit seeds are compared without JS rounding.
export function canonicalMemoryReceipt(db) {
  const conversations = db.prepare("SELECT id,title,provider_profile_id,CAST(created_at_ms AS TEXT) AS createdAt,CAST(updated_at_ms AS TEXT) AS updatedAt,CAST(archived_at_ms AS TEXT) AS archivedAt FROM conversations ORDER BY id").all();
  const messages = db.prepare("SELECT id,conversation_id,CAST(ordinal AS TEXT) AS ordinal,role,content,metadata_json,CAST(created_at_ms AS TEXT) AS createdAt FROM messages ORDER BY id").all();
  const notes = db.prepare("SELECT id,text,hex(block) AS compressedBlock,CAST(format_version AS TEXT) AS formatVersion,algorithm,CAST(dimension AS TEXT) AS dimension,CAST(seed AS TEXT) AS seed,CAST(bits AS TEXT) AS bits,CAST(original_bytes AS TEXT) AS originalBytes,CAST(created_at_ms AS TEXT) AS createdAt FROM crowquant_memories ORDER BY id").all();
  return {
    schemaVersion: db.prepare("PRAGMA user_version").get().user_version,
    conversations: conversations.length,
    messages: messages.length,
    notes: notes.length,
    canonicalSha256: createHash("sha256").update(JSON.stringify({ conversations, messages, notes })).digest("hex"),
  };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  if (!process.argv[2]) throw new Error("Provide the owned acceptance database path");
  const db = new DatabaseSync(process.argv[2], { readOnly: true });
  try { process.stdout.write(JSON.stringify(canonicalMemoryReceipt(db)) + "\n"); }
  finally { db.close(); }
}
