import {
  Brain,
  Clock3,
  Database,
  FileCheck2,
  Gauge,
  MessageSquareText,
  Plus,
  Search,
  StickyNote,
} from "lucide-react";
import { useEffect, useMemo, useState, type FormEvent } from "react";
import type {
  CrowClawGateway,
  CrowQuantMemory,
  CrowQuantSearchHit,
  MemoryRecord,
} from "../gateway/contracts";

type MemoryViewProps = {
  memories: MemoryRecord[];
  listCrowQuantMemories: CrowClawGateway["listCrowQuantMemories"];
  rememberCrowQuant: CrowClawGateway["rememberCrowQuant"];
  recallCrowQuant: CrowClawGateway["recallCrowQuant"];
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

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  return `${(bytes / 1024).toFixed(1)} KB`;
}

function memoryFromHit(hit: CrowQuantSearchHit): CrowQuantMemory {
  return hit.memory;
}

export function MemoryView({
  memories,
  listCrowQuantMemories,
  rememberCrowQuant,
  recallCrowQuant,
}: MemoryViewProps) {
  const [query, setQuery] = useState("");
  const [source, setSource] = useState<MemoryRecord["source"] | "all">("all");
  const [crowQuantMemories, setCrowQuantMemories] = useState<CrowQuantMemory[]>([]);
  const [rememberText, setRememberText] = useState("");
  const [recallQuery, setRecallQuery] = useState("");
  const [recallResults, setRecallResults] = useState<CrowQuantSearchHit[] | null>(null);
  const [crowQuantLoading, setCrowQuantLoading] = useState(true);
  const [remembering, setRemembering] = useState(false);
  const [recalling, setRecalling] = useState(false);
  const [crowQuantError, setCrowQuantError] = useState<string | null>(null);
  const [crowQuantNotice, setCrowQuantNotice] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    setCrowQuantLoading(true);
    setCrowQuantError(null);
    void listCrowQuantMemories()
      .then((records) => {
        if (active) setCrowQuantMemories(records);
      })
      .catch((cause: unknown) => {
        if (active) {
          setCrowQuantError(cause instanceof Error ? cause.message : "CrowQuant memory could not be loaded.");
        }
      })
      .finally(() => {
        if (active) setCrowQuantLoading(false);
      });
    return () => {
      active = false;
    };
  }, [listCrowQuantMemories]);

  const filtered = useMemo(() => {
    const normalized = query.trim().toLocaleLowerCase();
    return memories.filter((memory) => {
      if (source !== "all" && memory.source !== source) return false;
      return !normalized || `${memory.title} ${memory.preview} ${memory.tags.join(" ")}`.toLocaleLowerCase().includes(normalized);
    });
  }, [memories, query, source]);

  const visibleCrowQuantMemories = recallResults?.map(memoryFromHit) ?? crowQuantMemories;

  async function handleRemember(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const text = rememberText.trim();
    if (!text || remembering) return;
    setRemembering(true);
    setCrowQuantError(null);
    setCrowQuantNotice(null);
    try {
      const memory = await rememberCrowQuant(text);
      setCrowQuantMemories((current) => [memory, ...current.filter(({ id }) => id !== memory.id)]);
      setRecallResults(null);
      setRememberText("");
      setCrowQuantNotice("Stored locally with CrowQuant.");
    } catch (cause) {
      setCrowQuantError(cause instanceof Error ? cause.message : "CrowQuant could not store that memory.");
    } finally {
      setRemembering(false);
    }
  }

  async function handleRecall(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const nextQuery = recallQuery.trim();
    if (!nextQuery || recalling) return;
    setRecalling(true);
    setCrowQuantError(null);
    setCrowQuantNotice(null);
    try {
      setRecallResults(await recallCrowQuant(nextQuery, 8));
    } catch (cause) {
      setCrowQuantError(cause instanceof Error ? cause.message : "CrowQuant recall failed.");
    } finally {
      setRecalling(false);
    }
  }

  function showAllCrowQuantMemories() {
    setRecallResults(null);
    setRecallQuery("");
    setCrowQuantNotice(null);
  }

  return (
    <main className="section-view">
      <header className="section-heading">
        <div>
          <span className="eyebrow">Continuity</span>
          <h1>Memory</h1>
          <p>Store and retrieve local compressed lexical memory with CrowQuant while keeping approved activity records separate.</p>
        </div>
        <span className="section-stat"><Brain size={18} /> {crowQuantMemories.length + memories.length} records</span>
      </header>

      <section className="crowquant-panel" aria-labelledby="crowquant-heading">
        <header className="crowquant-panel__heading">
          <div className="crowquant-panel__identity">
            <span className="crowquant-panel__icon"><Database size={20} /></span>
            <div>
              <span className="eyebrow">Local compressed retrieval</span>
              <h2 id="crowquant-heading">CrowQuant compressed memory</h2>
              <p>Remember text as a compressed local lexical vector, then recall the highest-ranked stored context.</p>
            </div>
          </div>
          <span className="crowquant-record-count">{crowQuantMemories.length} stored</span>
        </header>

        <div className="crowquant-controls">
          <form className="crowquant-form" onSubmit={handleRemember}>
            <label htmlFor="crowquant-remember">Remember something</label>
            <textarea
              id="crowquant-remember"
              value={rememberText}
              onChange={(event) => setRememberText(event.currentTarget.value)}
              placeholder="Add a fact, preference, observation, or working note"
              rows={4}
            />
            <button className="button button--primary" type="submit" disabled={!rememberText.trim() || remembering}>
              <Plus size={16} /> {remembering ? "Compressing…" : "Remember with CrowQuant"}
            </button>
          </form>

          <form className="crowquant-form" onSubmit={handleRecall}>
            <label htmlFor="crowquant-recall">Recall related memory</label>
            <input
              id="crowquant-recall"
              value={recallQuery}
              onChange={(event) => setRecallQuery(event.currentTarget.value)}
              placeholder="What do you want CrowClaw to remember?"
            />
            <div className="crowquant-form__actions">
              <button className="button button--secondary" type="submit" disabled={!recallQuery.trim() || recalling}>
                <Search size={16} /> {recalling ? "Searching…" : "Recall memory"}
              </button>
              {recallResults !== null && (
                <button className="button button--secondary" type="button" onClick={showAllCrowQuantMemories}>
                  Show all
                </button>
              )}
            </div>
          </form>
        </div>

        {crowQuantError && <p className="inline-error" role="alert">{crowQuantError}</p>}
        {crowQuantNotice && <p className="crowquant-notice" role="status">{crowQuantNotice}</p>}

        <div className="crowquant-results-heading">
          <h3>{recallResults === null ? "Stored CrowQuant memories" : "Recall results"}</h3>
          {recallResults !== null && <span>{recallResults.length} nearest</span>}
        </div>

        {crowQuantLoading ? (
          <p className="crowquant-empty" role="status">Loading CrowQuant memory…</p>
        ) : visibleCrowQuantMemories.length === 0 ? (
          <div className="crowquant-empty">
            <Database size={22} />
            <p>{recallResults === null ? "Nothing has been stored with CrowQuant yet." : "No CrowQuant memory matched that query."}</p>
          </div>
        ) : (
          <div className="crowquant-grid">
            {visibleCrowQuantMemories.map((memory, index) => {
              const score = recallResults?.[index]?.score;
              return (
                <article className="crowquant-card" key={memory.id}>
                  <div className="crowquant-card__topline">
                    <span><Database size={14} /> {memory.algorithm}</span>
                    {score !== undefined && <strong>{Math.round(score * 100)}% match</strong>}
                  </div>
                  <p>{memory.text}</p>
                  <dl>
                    <div><dt>Original</dt><dd>{formatBytes(memory.originalBytes)}</dd></div>
                    <div><dt>Compressed</dt><dd>{formatBytes(memory.compressedBytes)}</dd></div>
                    <div><dt>Ratio</dt><dd><Gauge size={12} /> {memory.compressionRatio.toFixed(2)}×</dd></div>
                  </dl>
                  <time dateTime={memory.createdAt}><Clock3 size={12} /> {formatMemoryDate(memory.createdAt)}</time>
                </article>
              );
            })}
          </div>
        )}
      </section>

      <section className="activity-memory" aria-labelledby="activity-memory-heading">
        <header className="activity-memory__heading">
          <div>
            <span className="eyebrow">Audit continuity</span>
            <h2 id="activity-memory-heading">Approved activity memory</h2>
          </div>
          <span>{memories.length} records</span>
        </header>

        <div className="view-toolbar">
          <label className="search-field search-field--large">
            <Search size={17} aria-hidden="true" />
            <span className="sr-only">Search approved activity memory</span>
            <input value={query} onChange={(event) => setQuery(event.currentTarget.value)} placeholder="Search approved activity memory" />
          </label>
          <label className="select-field">
            <span className="sr-only">Filter activity memory source</span>
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
            <h2>{memories.length ? "No matching activity memory" : "No approved activity retained yet"}</h2>
            <p>{memories.length ? "Try a different search or source filter." : "Approved actions and retained conversation context will appear here."}</p>
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
      </section>
    </main>
  );
}
