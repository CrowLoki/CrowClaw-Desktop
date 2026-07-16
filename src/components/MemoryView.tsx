import { Brain, Clock3, FileCheck2, MessageSquareText, Search, StickyNote } from "lucide-react";
import { useMemo, useState } from "react";
import type { MemoryRecord } from "../gateway/contracts";

type MemoryViewProps = {
  memories: MemoryRecord[];
};

const sourceLabel: Record<MemoryRecord["source"], string> = {
  conversation: "Conversation",
  "approved-action": "Approved action",
  "user-note": "Your note",
};

function sourceIcon(source: MemoryRecord["source"]) {
  if (source === "conversation") return <MessageSquareText size={18} />;
  if (source === "approved-action") return <FileCheck2 size={18} />;
  return <StickyNote size={18} />;
}

function formatMemoryDate(value: string): string {
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    year: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(new Date(value));
}

export function MemoryView({ memories }: MemoryViewProps) {
  const [query, setQuery] = useState("");
  const [source, setSource] = useState<MemoryRecord["source"] | "all">("all");
  const filtered = useMemo(() => {
    const normalized = query.trim().toLocaleLowerCase();
    return memories.filter((memory) => {
      if (source !== "all" && memory.source !== source) return false;
      return !normalized || `${memory.title} ${memory.preview} ${memory.tags.join(" ")}`.toLocaleLowerCase().includes(normalized);
    });
  }, [memories, query, source]);

  return (
    <main className="section-view">
      <header className="section-heading">
        <div>
          <span className="eyebrow">Continuity</span>
          <h1>Memory</h1>
          <p>Review information CrowClaw has retained from your conversations and approved actions.</p>
        </div>
        <span className="section-stat"><Brain size={18} /> {memories.length} records</span>
      </header>

      <div className="view-toolbar">
        <label className="search-field search-field--large">
          <Search size={17} aria-hidden="true" />
          <span className="sr-only">Search memory</span>
          <input value={query} onChange={(event) => setQuery(event.currentTarget.value)} placeholder="Search retained memory" />
        </label>
        <label className="select-field">
          <span className="sr-only">Filter memory source</span>
          <select value={source} onChange={(event) => setSource(event.currentTarget.value as typeof source)}>
            <option value="all">All sources</option>
            <option value="conversation">Conversations</option>
            <option value="approved-action">Approved actions</option>
            <option value="user-note">Your notes</option>
          </select>
        </label>
      </div>

      {filtered.length === 0 ? (
        <section className="full-empty-state">
          <span><Brain size={26} /></span>
          <h2>{memories.length ? "No matching memory" : "Nothing retained yet"}</h2>
          <p>{memories.length ? "Try a different search or source filter." : "Saved context and approved actions will appear here."}</p>
        </section>
      ) : (
        <div className="memory-grid">
          {filtered.map((memory) => (
            <article className="memory-card" key={memory.id}>
              <div className="memory-card__source">{sourceIcon(memory.source)}<span>{sourceLabel[memory.source]}</span></div>
              <h2>{memory.title}</h2>
              <p>{memory.preview}</p>
              <div className="tag-row">{memory.tags.map((tag) => <span key={tag}>{tag}</span>)}</div>
              <time dateTime={memory.createdAt}><Clock3 size={13} /> {formatMemoryDate(memory.createdAt)}</time>
            </article>
          ))}
        </div>
      )}
    </main>
  );
}

