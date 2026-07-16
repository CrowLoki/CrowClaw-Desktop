import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ApprovalDialog } from "./ApprovalDialog";

describe("ApprovalDialog", () => {
  it("shows the exact CrowQuant search boundary as a medium-risk memory action", () => {
    render(
      <ApprovalDialog
        action={{
          id: "action-1",
          taskId: "task-1",
          conversationId: "conversation-1",
          kind: "memory",
          title: "Search CrowQuant memory",
          summary: "Search local CrowQuant memory for \"qubit calibration\" and return up to 7 matches",
          target: "CrowClaw local CrowQuant memory",
          details: [
            "Search for exactly: \"qubit calibration\"",
            "Return up to 7 compressed-lexical matches",
            "Read and return matching stored text to the connected model and approved-action audit",
          ],
          risk: "medium",
          requestedAt: "2026-07-17T00:00:00.000Z",
        }}
        deciding={null}
        onDecision={vi.fn()}
      />,
    );

    expect(screen.getByRole("heading", { name: "Search CrowQuant memory" })).toBeVisible();
    expect(screen.getByText("Requested memory action")).toBeVisible();
    expect(screen.getByText("CrowClaw local CrowQuant memory")).toBeVisible();
    expect(screen.getAllByText(/qubit calibration/)).toHaveLength(2);
    expect(screen.getByText("medium risk")).toBeVisible();
    expect(screen.getByText(/connected model and approved-action audit/)).toBeVisible();
  });
});
