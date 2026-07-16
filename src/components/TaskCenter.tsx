import {
  Ban,
  CheckCircle2,
  CircleDashed,
  Clock3,
  ListTodo,
  LoaderCircle,
  ShieldQuestion,
  TriangleAlert,
} from "lucide-react";
import type { AgentTask, AgentTaskStatus } from "../gateway/contracts";

type TaskCenterProps = {
  tasks: AgentTask[];
  cancellingTaskId: string | null;
  onCancel: (taskId: string) => void;
};

const statusCopy: Record<AgentTaskStatus, string> = {
  queued: "Queued",
  running: "Running",
  "waiting-approval": "Waiting for approval",
  completed: "Completed",
  cancelled: "Cancelled",
  failed: "Failed",
};

function TaskStatusIcon({ status }: { status: AgentTaskStatus }) {
  if (status === "running") return <LoaderCircle className="spin" size={17} />;
  if (status === "waiting-approval") return <ShieldQuestion size={17} />;
  if (status === "completed") return <CheckCircle2 size={17} />;
  if (status === "cancelled") return <Ban size={17} />;
  if (status === "failed") return <TriangleAlert size={17} />;
  return <CircleDashed size={17} />;
}

function formatTaskTime(value: string): string {
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(new Date(value));
}

export function TaskCenter({ tasks, cancellingTaskId, onCancel }: TaskCenterProps) {
  const active = tasks.filter(({ status }) => ["queued", "running", "waiting-approval"].includes(status));
  const previous = tasks.filter(({ status }) => !["queued", "running", "waiting-approval"].includes(status));

  return (
    <main className="section-view">
      <header className="section-heading">
        <div>
          <span className="eyebrow">Agent work</span>
          <h1>Tasks</h1>
          <p>See what CrowClaw is doing and stop any task that is still in progress.</p>
        </div>
        <span className="section-stat"><ListTodo size={18} /> {active.length} active</span>
      </header>

      {tasks.length === 0 && (
        <section className="full-empty-state">
          <span><ListTodo size={26} /></span>
          <h2>No tasks yet</h2>
          <p>Tasks appear here when you ask CrowClaw to do more than answer a question.</p>
        </section>
      )}

      {active.length > 0 && (
        <section className="content-section" aria-labelledby="active-tasks-title">
          <div className="content-section__title"><h2 id="active-tasks-title">Active</h2><span>{active.length}</span></div>
          <div className="task-list">
            {active.map((task) => (
              <article className="task-card task-card--active" key={task.id}>
                <div className={`task-card__status task-card__status--${task.status}`}><TaskStatusIcon status={task.status} /></div>
                <div className="task-card__body">
                  <div className="task-card__topline">
                    <h3>{task.title}</h3>
                    <span className={`status-label status-label--${task.status}`}>{statusCopy[task.status]}</span>
                  </div>
                  <p>{task.detail}</p>
                  {task.progress !== null && (
                    <div className="progress-row">
                      <span className="progress-track"><span style={{ width: `${task.progress}%` }} /></span>
                      <small>{task.progress}%</small>
                    </div>
                  )}
                  <time dateTime={task.updatedAt}><Clock3 size={13} /> Updated {formatTaskTime(task.updatedAt)}</time>
                </div>
                {task.cancellable && (
                  <button className="button button--danger-quiet" type="button" onClick={() => onCancel(task.id)} disabled={cancellingTaskId === task.id}>
                    {cancellingTaskId === task.id ? <LoaderCircle className="spin" size={16} /> : <Ban size={16} />}
                    Cancel
                  </button>
                )}
              </article>
            ))}
          </div>
        </section>
      )}

      {previous.length > 0 && (
        <section className="content-section" aria-labelledby="previous-tasks-title">
          <div className="content-section__title"><h2 id="previous-tasks-title">Previous</h2><span>{previous.length}</span></div>
          <div className="task-list task-list--compact">
            {previous.map((task) => (
              <article className="task-card" key={task.id}>
                <div className={`task-card__status task-card__status--${task.status}`}><TaskStatusIcon status={task.status} /></div>
                <div className="task-card__body">
                  <div className="task-card__topline"><h3>{task.title}</h3><span className={`status-label status-label--${task.status}`}>{statusCopy[task.status]}</span></div>
                  <p>{task.detail}</p>
                </div>
              </article>
            ))}
          </div>
        </section>
      )}
    </main>
  );
}

