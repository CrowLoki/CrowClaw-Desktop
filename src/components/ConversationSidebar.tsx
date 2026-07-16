import { MessageSquarePlus, Search } from "lucide-react";
import { useMemo, useState } from "react";
import type { ConversationSummary } from "../gateway/contracts";

type ConversationSidebarProps = {
  conversations: ConversationSummary[];
  selectedId: string | null;
  creating: boolean;
  onCreate: () => void;
  onSelect: (conversationId: string) => void;
};

function formatConversationTime(value: string): string {
  const date = new Date(value);
  const today = new Date();
  if (date.toDateString() === today.toDateString()) {
    return new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" }).format(date);
  }
  return new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" }).format(date);
}

export function ConversationSidebar({
  conversations,
  selectedId,
  creating,
  onCreate,
  onSelect,
}: ConversationSidebarProps) {
  const [query, setQuery] = useState("");
  const filtered = useMemo(() => {
    const normalized = query.trim().toLocaleLowerCase();
    if (!normalized) return conversations;
    return conversations.filter(({ title, preview }) =>
      `${title} ${preview}`.toLocaleLowerCase().includes(normalized),
    );
  }, [conversations, query]);

  return (
    <aside className="conversation-sidebar" aria-label="Conversations">
      <div className="conversation-sidebar__heading">
        <div>
          <span className="eyebrow">Workspace</span>
          <h2>Conversations</h2>
        </div>
        <button className="icon-button icon-button--accent" type="button" onClick={onCreate} disabled={creating} aria-label="New conversation">
          <MessageSquarePlus size={19} />
        </button>
      </div>
      <label className="search-field">
        <Search size={16} aria-hidden="true" />
        <span className="sr-only">Search conversations</span>
        <input value={query} onChange={(event) => setQuery(event.currentTarget.value)} placeholder="Search" />
        <kbd>Ctrl K</kbd>
      </label>
      <div className="conversation-list">
        {filtered.length === 0 && (
          <div className="sidebar-empty">
            <p>{query ? "No conversations match that search." : "No conversations yet."}</p>
            {!query && <button type="button" onClick={onCreate}>Start one</button>}
          </div>
        )}
        {filtered.map((conversation) => (
          <button
            className={conversation.id === selectedId ? "conversation-row conversation-row--active" : "conversation-row"}
            type="button"
            key={conversation.id}
            onClick={() => onSelect(conversation.id)}
          >
            <span className="conversation-row__topline">
              <strong>{conversation.title}</strong>
              <time dateTime={conversation.updatedAt}>{formatConversationTime(conversation.updatedAt)}</time>
            </span>
            <span className="conversation-row__preview">{conversation.preview}</span>
          </button>
        ))}
      </div>
      <div className="conversation-sidebar__footer">
        <span className="local-indicator"><span /> Local workspace</span>
        <small>{conversations.length} saved</small>
      </div>
    </aside>
  );
}

