import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { ProviderForm } from "./ProviderForm";

describe("Standalone CrowBot AI connection", () => {
  it("chooses direct internet instead of an automatically detected local donor app", async () => {
    const test = vi.fn().mockResolvedValue({ ok: true, detail: "Connected", latencyMs: 1 });
    const user = userEvent.setup();
    render(<ProviderForm initialDraft={{ provider: "custom", label: "Custom", baseUrl: "http://127.0.0.1:8000/v1", model: "test", apiKey: "synthetic-old-key" }}
      discovered={[{ id: "local", provider: "crowbot-ai", label: "CrowBot AI", baseUrl: "http://127.0.0.1:12345/api/crowbot-ai/v1", model: "crowbot-auto", detected: true, availableModels: ["crowbot-auto"] }]}
      submitLabel="Connect" onSubmit={vi.fn()} onTest={test} />);
    await user.click(screen.getByRole("radio", { name: /CrowBot AI/ }));
    expect(screen.getByLabelText("Endpoint URL")).toHaveValue("https://miaoxue.api.open.ocrmath.com");
    expect(screen.queryByText("Gateway key")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Test connection" }));
    await waitFor(() => expect(test).toHaveBeenCalledWith(expect.objectContaining({
      provider: "crowbot-ai", baseUrl: "https://miaoxue.api.open.ocrmath.com", model: "crowbot-auto", apiKey: undefined,
    })));
  });
});
