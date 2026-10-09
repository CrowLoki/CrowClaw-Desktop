/** Native catalog contains only models verified to have zero input/output price. */
export type FreeModel = {
  id: string;
  name: string;
  contextLength: number;
  inputModalities: string[];
  supportedParameters: string[];
  reasoningEfforts: string[];
};

export type FreeCatalog = { models: FreeModel[]; fetchedAtMs: number };
export type OpenRouterConnectRequest = { label: string; apiKey: string; model: string };
