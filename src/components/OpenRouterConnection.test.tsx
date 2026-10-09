import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { OpenRouterConnection } from './OpenRouterConnection';
import { App } from '../App';
import { createDevelopmentGateway } from '../gateway/developmentGateway';
import * as native from '../gateway/tauriGateway';
import type { ModelConnection } from '../gateway/contracts';
import type { FreeCatalog } from '../gateway/openRouterContracts';

vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => undefined) }));

const catalog: FreeCatalog = { fetchedAtMs: 1700000000000, models: [
  { id: 'vendor/free-one', name: 'Free One', contextLength: 32768, inputModalities: ['text', 'image'], supportedParameters: ['tools'], reasoningEfforts: [] },
  { id: 'vendor/free-two', name: 'Free Two', contextLength: 8192, inputModalities: ['text'], supportedParameters: [], reasoningEfforts: ['low', 'high'] },
] };
const connection: ModelConnection = { id: 'openrouter:profile', provider: 'openrouter', label: 'OpenRouter FREE', baseUrl: 'https://openrouter.ai/api/v1', model: 'vendor/free-one', status: 'connected', connectedAt: null, latencyMs: null };
function setup(current: ModelConnection | null = null) {
  const gateway = { openRouterCatalog: vi.fn(async () => catalog), connectOpenRouter: vi.fn(async () => connection), disconnectOpenRouter: vi.fn(async () => undefined) };
  const onConnected = vi.fn(async () => undefined);
  const onDisconnected = vi.fn();
  return { gateway, onConnected, onDisconnected, props: { gateway, connection: current, hasConversation: true, onConnected, onDisconnected } };
}
async function fill() {
  await waitFor(() => expect(screen.getByLabelText('Free model')).toBeEnabled());
  await userEvent.selectOptions(screen.getByLabelText('Free model'), 'vendor/free-one');
  fireEvent.change(screen.getByLabelText('OpenRouter API key'), { target: { value: 'test-secret-value' } });
}

describe('OpenRouter free catalog connection', () => {
  it('browses before key entry, searches only backend free results, and reports actual metadata', async () => {
    const { props, gateway } = setup();
    render(<OpenRouterConnection {...props} />);
    expect(screen.getByText('Loading free catalog…')).toBeVisible();
    await screen.findByRole('option', { name: /Free One/ });
    expect(gateway.openRouterCatalog).toHaveBeenCalledWith(undefined);
    expect(screen.getByText(/2 free models/)).toBeVisible();
    expect(screen.getByText(/no paid fallback/)).toBeVisible();
    expect(screen.queryByRole('option', { name: /paid model/i })).not.toBeInTheDocument();
    expect(gateway.connectOpenRouter).not.toHaveBeenCalled();
    await userEvent.selectOptions(screen.getByLabelText('Free model'), 'vendor/free-one');
    expect(screen.getByText('32,768 tokens')).toBeVisible();
    expect(screen.getByText('text, image')).toBeVisible();
    expect(screen.getByText('Listed as supported')).toBeVisible();
    expect(screen.getByText('Provider default; no effort levels reported')).toBeVisible();
    fireEvent.change(screen.getByLabelText('Search free models'), { target: { value: 'two' } });
    await userEvent.selectOptions(screen.getByLabelText('Free model'), 'vendor/free-two');
    expect(screen.getByText('low, high')).toBeVisible();
    expect(screen.getByText('Not listed by the catalog')).toBeVisible();
    expect(screen.queryByRole('option', { name: /Free One/ })).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText('Search free models'), { target: { value: 'missing' } });
    expect(screen.getByText('No free models match your search.')).toBeVisible();
  });

  it('shows load failure, retry and empty catalog without inventing models', async () => {
    const { props, gateway } = setup();
    gateway.openRouterCatalog.mockRejectedValueOnce(new Error('offline')).mockResolvedValueOnce({ ...catalog, models: [] });
    render(<OpenRouterConnection {...props} />);
    expect(await screen.findByRole('alert')).toHaveTextContent('Could not refresh');
    await userEvent.click(screen.getByRole('button', { name: 'Retry catalog' }));
    expect(await screen.findByText(/No free models are currently available/)).toBeVisible();
    expect(screen.getAllByRole('option')).toHaveLength(1);
    expect(screen.getByRole('button', { name: 'Save for new chats' })).toBeDisabled();
  });

  it('blocks stale selections after a refresh removes a model and after refresh failure', async () => {
    const { props, gateway } = setup();
    render(<OpenRouterConnection {...props} />);
    await fill();
    gateway.openRouterCatalog.mockRejectedValueOnce(new Error('offline'));
    await userEvent.click(screen.getByRole('button', { name: 'Refresh free catalog' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('catalog is stale');
    expect(screen.getByRole('button', { name: 'Save for new chats' })).toBeDisabled();
    gateway.openRouterCatalog.mockResolvedValueOnce({ ...catalog, models: [catalog.models[1]] });
    await userEvent.click(screen.getByRole('button', { name: 'Retry catalog' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('no longer in the free catalog');
    expect(screen.getByRole('button', { name: 'Save and use in this chat' })).toBeDisabled();
    expect(gateway.connectOpenRouter).not.toHaveBeenCalled();
  });

  it.each([false, true])('clears the password after save and explicitly passes use-current-chat=%s', async useCurrent => {
    const { props, gateway, onConnected } = setup();
    const storage = vi.spyOn(Storage.prototype, 'setItem');
    const logs = [vi.spyOn(console, 'log'), vi.spyOn(console, 'warn'), vi.spyOn(console, 'error')];
    render(<OpenRouterConnection {...props} />);
    await fill();
    expect(screen.getByLabelText('OpenRouter API key')).toHaveAttribute('type', 'password');
    await userEvent.click(screen.getByRole('button', { name: useCurrent ? 'Save and use in this chat' : 'Save for new chats' }));
    await waitFor(() => expect(onConnected).toHaveBeenCalledWith(connection, useCurrent));
    expect(gateway.connectOpenRouter).toHaveBeenCalledWith({ label: 'OpenRouter FREE', apiKey: 'test-secret-value', model: 'vendor/free-one' });
    expect(screen.getByLabelText('OpenRouter API key')).toHaveValue('');
    expect(storage).not.toHaveBeenCalled();
    for (const log of logs) expect(JSON.stringify(log.mock.calls)).not.toContain('test-secret-value');
  });

  it('distinguishes save failure from saved-but-choice-failed without echoing sensitive errors', async () => {
    const { props, gateway, onConnected } = setup();
    gateway.connectOpenRouter.mockRejectedValueOnce(new Error('response included test-secret-value'));
    render(<OpenRouterConnection {...props} />);
    await fill();
    await userEvent.click(screen.getByRole('button', { name: 'Save and use in this chat' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('could not be saved');
    expect(screen.getByRole('alert')).not.toHaveTextContent('test-secret-value');
    expect(onConnected).not.toHaveBeenCalled();
    onConnected.mockRejectedValueOnce(new Error('choice failed'));
    await userEvent.click(screen.getByRole('button', { name: 'Save and use in this chat' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Connection saved as the default, but selecting it for this chat failed');
    expect(screen.getByLabelText('OpenRouter API key')).toHaveValue('');
  });

  it('uses only the current OpenRouter profile for authenticated refresh and disconnect', async () => {
    const { props, gateway, onDisconnected } = setup(connection);
    gateway.disconnectOpenRouter.mockRejectedValueOnce(new Error('offline'));
    render(<OpenRouterConnection {...props} />);
    await screen.findByRole('option', { name: /Free One/ });
    expect(gateway.openRouterCatalog).toHaveBeenCalledWith(connection.id);
    await userEvent.click(screen.getByRole('button', { name: 'Disconnect this OpenRouter connection' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Could not disconnect');
    expect(onDisconnected).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole('button', { name: 'Disconnect this OpenRouter connection' }));
    expect(gateway.disconnectOpenRouter).toHaveBeenLastCalledWith(connection.id);
    expect(onDisconnected).toHaveBeenCalledWith(connection.id);
    expect(screen.getByText(/Chats are retained/)).toBeVisible();
  });

  it('honestly rejects browser operations and does not accept a key in browser preview', async () => {
    const gateway = createDevelopmentGateway();
    await expect(gateway.openRouterCatalog()).rejects.toThrow('Browser preview does not access OpenRouter');
    await expect(gateway.connectOpenRouter({ label: 'test', apiKey: 'test', model: 'test' })).rejects.toThrow('No key was saved');
    render(<OpenRouterConnection {...setup().props} gateway={gateway} browserPreview />);
    expect(await screen.findByRole('alert')).toHaveTextContent('unavailable in browser preview');
    expect(screen.getByLabelText('OpenRouter API key')).toBeDisabled();
    expect(screen.queryByRole('button', { name: /Disconnect/ })).not.toBeInTheDocument();
  });

  it.each([false, true])('App applies explicit choice=%s while preserving independent chats, draft, files and active task', async useCurrent => {
    vi.spyOn(native, 'isTauriRuntime').mockReturnValue(true);
    const gateway = createDevelopmentGateway({ firstRun: false, delayMs: 0, includeRunningTask: true });
    const initial = await gateway.bootstrap();
    const id = initial.selectedConversationId!;
    const other = await gateway.createConversation();
    const bootstrap = gateway.bootstrap.bind(gateway);
    vi.spyOn(gateway, 'bootstrap').mockImplementation(async () => ({ ...await bootstrap(), selectedConversationId: id }));
    const otherBefore = await gateway.getComposer(other.conversation.id);
    const saved = await gateway.getComposer(id);
    await gateway.selectAttachments(id, saved.composer.revision);
    const before = await gateway.getComposer(id);
    vi.spyOn(gateway, 'openRouterCatalog').mockResolvedValue(catalog);
    vi.spyOn(gateway, 'connectOpenRouter').mockResolvedValue(connection);
    const choose = vi.spyOn(gateway, 'chooseComposerModel').mockImplementation(async (chatId, revision, selection) => {
      const state = await gateway.getComposer(chatId);
      expect(state.composer.revision).toBe(revision);
      return { ...state, composer: { ...state.composer, revision: revision + 1, selection }, connection };
    });
    const send = vi.spyOn(gateway, 'sendMessage');
    render(<App gateway={gateway} />);
    const input = await screen.findByLabelText('Message CrowClaw');
    await waitFor(() => expect(input).toBeEnabled());
    fireEvent.change(input, { target: { value: 'Keep my draft' } });
    await userEvent.click(screen.getByRole('button', { name: 'Connections' }));
    await fill();
    await userEvent.click(screen.getByRole('button', { name: useCurrent ? 'Save and use in this chat' : 'Save for new chats' }));
    await screen.findByText(useCurrent ? /Connection saved and selected/ : /Connection saved as the default for new chats/);
    if (useCurrent) expect(choose).toHaveBeenCalledWith(id, expect.any(Number), { providerProfileId: connection.id, model: connection.model, reasoningEffort: null });
    else expect(choose).not.toHaveBeenCalled();
    expect(screen.getAllByRole('heading', { name: 'OpenRouter FREE' })).toHaveLength(2);
    await userEvent.click(screen.getByRole('button', { name: 'Chat' }));
    await waitFor(() => expect(screen.getByLabelText('Message CrowClaw')).toHaveValue('Keep my draft'));
    expect((await gateway.getComposer(id)).composer.attachments).toEqual(before.composer.attachments);
    expect(await gateway.getComposer(other.conversation.id)).toEqual(otherBefore);
    expect((await gateway.bootstrap()).tasks).toEqual(initial.tasks);
    expect(send).not.toHaveBeenCalled();
  });

  it('App retains the saved default when choosing it for the current chat fails', async () => {
    vi.spyOn(native, 'isTauriRuntime').mockReturnValue(true);
    const gateway = createDevelopmentGateway({ firstRun: false, delayMs: 0 });
    vi.spyOn(gateway, 'openRouterCatalog').mockResolvedValue(catalog);
    vi.spyOn(gateway, 'connectOpenRouter').mockResolvedValue(connection);
    vi.spyOn(gateway, 'chooseComposerModel').mockRejectedValue(new Error('revision conflict'));
    render(<App gateway={gateway} />);
    await screen.findByLabelText('Message CrowClaw');
    await userEvent.click(screen.getByRole('button', { name: 'Connections' }));
    await fill();
    await userEvent.click(screen.getByRole('button', { name: 'Save and use in this chat' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Connection saved as the default, but selecting it for this chat failed');
    expect(screen.getAllByRole('heading', { name: 'OpenRouter FREE' })).toHaveLength(2);
    expect(screen.getByLabelText('OpenRouter API key')).toHaveValue('');
    expect(screen.getByRole('button', { name: 'Disconnect this OpenRouter connection' })).toBeEnabled();
  });

  it('allows first-run catalog browsing before any login or local connection', async () => {
    vi.spyOn(native, 'isTauriRuntime').mockReturnValue(true);
    const gateway = createDevelopmentGateway({ firstRun: true, delayMs: 0 });
    const browse = vi.spyOn(gateway, 'openRouterCatalog').mockResolvedValue(catalog);
    const connect = vi.spyOn(gateway, 'connectOpenRouter');
    render(<App gateway={gateway} />);
    await screen.findByRole('option', { name: /Free One/ });
    expect(browse).toHaveBeenCalledWith(undefined);
    expect(connect).not.toHaveBeenCalled();
    expect(screen.queryByRole('button', { name: 'Save and use in this chat' })).not.toBeInTheDocument();
    expect(screen.getByLabelText('OpenRouter API key')).toHaveValue('');
  });

  it('opens chat after first-run OpenRouter save without connecting a local server', async () => {
    vi.spyOn(native, 'isTauriRuntime').mockReturnValue(true);
    const gateway = createDevelopmentGateway({ firstRun: true, delayMs: 0 });
    vi.spyOn(gateway, 'openRouterCatalog').mockResolvedValue(catalog);
    vi.spyOn(gateway, 'connectOpenRouter').mockResolvedValue(connection);
    const getComposer = gateway.getComposer.bind(gateway);
    vi.spyOn(gateway, 'getComposer').mockImplementation(async id => {
      const snapshot = await getComposer(id);
      return { ...snapshot, composer: { ...snapshot.composer, selection: { providerProfileId: connection.id, model: connection.model, reasoningEffort: null } }, connection };
    });
    const connectLocal = vi.spyOn(gateway, 'connectModel');
    render(<App gateway={gateway} />);
    await screen.findByRole('option', { name: /Free One/ });
    await fill();
    await userEvent.click(screen.getByRole('button', { name: 'Save for new chats' }));
    const input = await screen.findByLabelText('Message CrowClaw');
    await waitFor(() => expect(input).toBeEnabled());
    expect(input).toHaveAttribute('placeholder', `Message CrowClaw · ${connection.model}`);
    expect(connectLocal).not.toHaveBeenCalled();
    expect(screen.queryByLabelText('OpenRouter API key')).not.toBeInTheDocument();
  });
});
