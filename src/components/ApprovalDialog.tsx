import { Check, FileText, LoaderCircle, ShieldAlert, X } from "lucide-react";
import type { ActionDecision, PendingAction } from "../gateway/contracts";

type ApprovalDialogProps = {
  action: PendingAction;
  deciding: ActionDecision | null;
  onDecision: (decision: ActionDecision) => void;
};

export function ApprovalDialog({ action, deciding, onDecision }: ApprovalDialogProps) {
  return (
    <div className="modal-backdrop">
      <section className="approval-dialog" role="alertdialog" aria-modal="true" aria-labelledby="approval-title" aria-describedby="approval-summary">
        <div className="approval-dialog__heading">
          <span className="approval-dialog__icon"><ShieldAlert size={23} /></span>
          <div>
            <span className="eyebrow">Your approval is required</span>
            <h2 id="approval-title">{action.title}</h2>
          </div>
        </div>
        <p id="approval-summary" className="approval-summary">{action.summary}</p>
        <div className="action-target">
          <FileText size={20} />
          <div><span>Requested access</span><strong>{action.target}</strong></div>
          <span className={`risk-badge risk-badge--${action.risk}`}>{action.risk} risk</span>
        </div>
        <div className="action-details">
          <h3>Exactly what CrowClaw will do</h3>
          <ul>{action.details.map((detail) => <li key={detail}><Check size={15} /> {detail}</li>)}</ul>
        </div>
        <p className="approval-boundary">Nothing in this request runs unless you approve it. Denying it stops this action.</p>
        <div className="approval-dialog__actions">
          <button className="button button--secondary button--wide" type="button" onClick={() => onDecision("denied")} disabled={deciding !== null} autoFocus>
            {deciding === "denied" ? <LoaderCircle className="spin" size={17} /> : <X size={17} />}
            Deny
          </button>
          <button className="button button--primary button--wide" type="button" onClick={() => onDecision("approved")} disabled={deciding !== null}>
            {deciding === "approved" ? <LoaderCircle className="spin" size={17} /> : <Check size={17} />}
            Approve once
          </button>
        </div>
      </section>
    </div>
  );
}

