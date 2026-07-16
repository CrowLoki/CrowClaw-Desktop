import {
  Brain,
  Cable,
  CircleUserRound,
  ListTodo,
  MessageSquareText,
  Settings,
} from "lucide-react";
import type { ReactNode } from "react";
import type { AgentTask, ModelConnection } from "../gateway/contracts";
import { BrandMark } from "./BrandMark";

export type AppView = "chat" | "tasks" | "memory" | "connections" | "settings";

type AppShellProps = {
  view: AppView;
  connection: ModelConnection;
  tasks: AgentTask[];
  developmentPreview: boolean;
  sidebar?: ReactNode;
  children: ReactNode;
  onViewChange: (view: AppView) => void;
};

const navigation = [
  { id: "chat", label: "Chat", icon: MessageSquareText },
  { id: "tasks", label: "Tasks", icon: ListTodo },
  { id: "memory", label: "Memory", icon: Brain },
  { id: "connections", label: "Connections", icon: Cable },
  { id: "settings", label: "Settings", icon: Settings },
] satisfies Array<{ id: AppView; label: string; icon: typeof MessageSquareText }>;

export function AppShell({
  view,
  connection,
  tasks,
  developmentPreview,
  sidebar,
  children,
  onViewChange,
}: AppShellProps) {
  const activeTasks = tasks.filter(({ status }) =>
    ["queued", "running", "waiting-approval"].includes(status),
  ).length;

  return (
    <div className="desktop-app">
      {developmentPreview && (
        <div className="development-ribbon" role="status">
          Development preview · simulated local data · no files or model are accessed
        </div>
      )}
      <nav className="navigation-rail" aria-label="CrowClaw sections">
        <div className="navigation-rail__brand"><BrandMark compact /></div>
        <div className="navigation-rail__items">
          {navigation.map(({ id, label, icon: Icon }) => (
            <button
              className={view === id ? "rail-button rail-button--active" : "rail-button"}
              type="button"
              key={id}
              aria-current={view === id ? "page" : undefined}
              onClick={() => onViewChange(id)}
            >
              <span className="rail-button__icon">
                <Icon size={20} strokeWidth={1.8} />
                {id === "tasks" && activeTasks > 0 && <span className="rail-button__badge">{activeTasks}</span>}
              </span>
              <span>{label}</span>
            </button>
          ))}
        </div>
        <button className="account-button" type="button" aria-label="Local profile">
          <CircleUserRound size={23} strokeWidth={1.6} />
        </button>
      </nav>
      {sidebar}
      <div className="application-stage">
        <header className="application-titlebar">
          <span className="application-titlebar__title">CrowClaw</span>
          <span className="model-chip" title={`${connection.baseUrl} · ${connection.model}`}>
            <span className="model-chip__status" />
            {connection.label}
            <small>{connection.model}</small>
          </span>
        </header>
        {children}
      </div>
    </div>
  );
}

