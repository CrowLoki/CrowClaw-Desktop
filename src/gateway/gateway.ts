import type { CrowClawGateway } from "./contracts";
import { createDevelopmentGateway } from "./developmentGateway";
import { createTauriGateway, isTauriRuntime } from "./tauriGateway";

class UnavailableGateway implements CrowClawGateway {
  private unavailable(): never {
    throw new Error("CrowClaw native runtime is unavailable. Launch the installed desktop application.");
  }

  bootstrap = async () => this.unavailable();
  discoverEndpoints = async () => this.unavailable();
  testConnection = async () => this.unavailable();
  connectModel = async () => this.unavailable();
  createConversation = async () => this.unavailable();
  getConversation = async () => this.unavailable();
  selectFolder = async () => this.unavailable();
  sendMessage = async () => this.unavailable();
  cancelTask = async () => this.unavailable();
  decideAction = async () => this.unavailable();
  saveSettings = async () => this.unavailable();
  listCrowQuantMemories = async () => this.unavailable();
  rememberCrowQuant = async () => this.unavailable();
  recallCrowQuant = async () => this.unavailable();
  memoryStatus = async () => this.unavailable();
  configureMemory = async () => this.unavailable();
  searchMemory = async () => this.unavailable();
  withdrawMemory = async () => this.unavailable();
  syncMemory = async () => this.unavailable();
  rebuildMemory = async () => this.unavailable();
  exportMemory = async () => this.unavailable();
  admitFileMemory = async () => this.unavailable();
  syncSemanticMemory = async () => this.unavailable();
  evolutionSnapshot = async () => this.unavailable();
  saveEvolutionFeedback = async () => this.unavailable();
  draftEvolution = async () => this.unavailable();
  reflectEvolution = async () => this.unavailable();
  evaluateEvolution = async () => this.unavailable();
  rateEvolutionEvaluation = async () => this.unavailable();
  decideEvolution = async () => this.unavailable();
  restoreEvolution = async () => this.unavailable();
  cancelEvolution = async () => this.unavailable();
}

export function createCrowClawGateway(): CrowClawGateway {
  if (isTauriRuntime()) return createTauriGateway();
  if (import.meta.env.DEV) return createDevelopmentGateway();
  return new UnavailableGateway();
}
