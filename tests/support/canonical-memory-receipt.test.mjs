import assert from "node:assert/strict";
import { test } from "node:test";
import { DatabaseSync } from "node:sqlite";
import { canonicalMemoryReceipt } from "./canonical-memory-receipt.mjs";

function fixture() {
  const db = new DatabaseSync(":memory:");
  db.exec(`
    PRAGMA user_version=2;
    CREATE TABLE conversations(id TEXT,title TEXT,provider_profile_id TEXT,created_at_ms INTEGER,updated_at_ms INTEGER,archived_at_ms INTEGER);
    CREATE TABLE messages(id TEXT,conversation_id TEXT,ordinal INTEGER,role TEXT,content TEXT,metadata_json TEXT,created_at_ms INTEGER);
    CREATE TABLE crowquant_memories(id TEXT,text TEXT,block BLOB,format_version INTEGER,algorithm TEXT,dimension INTEGER,seed INTEGER,bits INTEGER,original_bytes INTEGER,created_at_ms INTEGER);
    INSERT INTO conversations VALUES('conversation-1','Telescope',NULL,123,124,NULL);
    INSERT INTO messages VALUES('message-1','conversation-1',0,'user','Cobalt telescope','{"toolCalls":[]}',124);
    INSERT INTO crowquant_memories VALUES('note-1','Retained note',X'010203',1,'fixture',256,9223372036854775807,4,2048,125);
  `);
  return db;
}

test("upgrade evidence preserves exact canonical data independently of derived schema", () => {
  const db = fixture();
  try {
    const before = canonicalMemoryReceipt(db);
    assert.equal(before.schemaVersion, 2);
    assert.equal(before.conversations, 1);
    assert.equal(before.messages, 1);
    assert.equal(before.notes, 1);
    assert.match(before.canonicalSha256, /^[a-f0-9]{64}$/);
    db.exec("PRAGMA user_version=5; CREATE TABLE derived_index(dummy TEXT);");
    const after = canonicalMemoryReceipt(db);
    assert.equal(after.schemaVersion, 5);
    assert.equal(after.canonicalSha256, before.canonicalSha256);
    assert(!JSON.stringify(after).includes("Retained note"));
  } finally { db.close(); }
});

test("upgrade evidence detects lost messages, changed IDs, compressed bytes and metadata", () => {
  for (const mutation of [
    "DELETE FROM messages",
    "UPDATE messages SET ordinal=1",
    "UPDATE messages SET metadata_json='{}'",
    "UPDATE conversations SET id='different'",
    "UPDATE conversations SET archived_at_ms=999",
    "UPDATE conversations SET provider_profile_id='different-profile'",
    "UPDATE crowquant_memories SET block=X'010204'",
    "UPDATE crowquant_memories SET seed=9223372036854775806",
    "UPDATE crowquant_memories SET text='changed'",
  ]) {
    const db = fixture();
    try {
      const before = canonicalMemoryReceipt(db);
      db.exec(mutation);
      assert.notEqual(canonicalMemoryReceipt(db).canonicalSha256, before.canonicalSha256, mutation);
    } finally { db.close(); }
  }
});
