import { afterEach, describe, expect, it, vi } from "vitest";
import { createDevelopmentGateway } from "./developmentGateway";
import { createCrowClawGateway } from "./gateway";

afterEach(() => vi.unstubAllEnvs());

describe("Membership gateway availability", () => {
  it("reports native-only operations as unsupported in development without fake account success", async () => {
    const gateway = createDevelopmentGateway({ delayMs: 0 });
    await expect(gateway.membershipSnapshot()).resolves.toEqual({ accounts: [], welcomeAcknowledged: false });
    const operations = [
      gateway.signInMembership({ requestId: "request", accountId: null, label: "Personal" }),
      gateway.cancelMembershipSignIn("request"), gateway.signOutMembership("account"),
      gateway.refreshMembershipModels("account"),
      gateway.useMembershipModel({ accountId: "account", model: "returned", reasoningEffort: null }),
      gateway.acknowledgeMembershipWelcome(), gateway.manageMembershipUsage(),
    ];
    await Promise.all(operations.map((operation) => expect(operation).rejects.toThrow(/require the installed native CrowClaw application/)));
    await expect(gateway.membershipSnapshot()).resolves.toEqual({ accounts: [], welcomeAcknowledged: false });
  });

  it("covers all membership operations when the production native gateway is unavailable", async () => {
    vi.stubEnv("DEV", false);
    const gateway = createCrowClawGateway();
    const operations = [
      gateway.membershipSnapshot(), gateway.signInMembership({ requestId: "request", label: "Personal", accountId: null }),
      gateway.cancelMembershipSignIn("request"), gateway.signOutMembership("account"),
      gateway.refreshMembershipModels("account"), gateway.useMembershipModel({ accountId: "account", model: "returned", reasoningEffort: null }),
      gateway.acknowledgeMembershipWelcome(), gateway.manageMembershipUsage(),
    ];
    await Promise.all(operations.map((operation) => expect(operation).rejects.toThrow("native runtime is unavailable")));
  });
});
