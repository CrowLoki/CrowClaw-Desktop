import { act, fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { AttachmentPreview } from './AttachmentPreview';
import type { AttachmentPreview as Preview, AttachmentSummary } from '../gateway/attachmentContracts';

const pdfView = vi.hoisted(() => ({ render: vi.fn() }));
vi.mock('./PdfAttachmentPreview', () => ({
  default: (props: { dataUrl: string; name: string }) => {
    pdfView.render(props);
    return <section aria-label={`Local PDF: ${props.name}`} />;
  },
}));

const attachment: AttachmentSummary = { id: 'a', conversationId: 'chat-a', messageId: null, name: 'notes.txt', mediaType: 'text/plain', kind: 'text', byteLength: 1024, sha256: 'hash', createdAtMs: 1 };
function show(value: Partial<Preview> = {}, summary: Partial<AttachmentSummary> = {}) {
  const selected = { ...attachment, ...summary };
  const previewAttachment = vi.fn(async (): Promise<Preview> => ({ attachment: selected, text: null, dataUrl: null, ...value }));
  const onRemove = vi.fn(async () => {});
  const rendered = render(<AttachmentPreview attachment={selected} previewAttachment={previewAttachment} onRemove={onRemove} />);
  fireEvent.click(screen.getByRole('button', { name: `Preview ${selected.name}` }));
  return { ...rendered, previewAttachment, onRemove };
}

describe('AttachmentPreview', () => {
  it('shows filename, size, remove, and text literally without interpreting markup', async () => {
    const text = '<script>alert(1)</script><svg onload="alert(1)">literal code</svg>';
    const { container, previewAttachment, onRemove } = show({ text });
    expect(await screen.findByText(text)).toBeVisible();
    expect(container.querySelector('script')).toBeNull();
    expect(container.querySelector('.file-attachment__preview svg')).toBeNull();
    expect(screen.getByText('1.0 KiB')).toBeVisible();
    expect(previewAttachment).toHaveBeenCalledWith('chat-a', 'a');
    fireEvent.click(screen.getByRole('button', { name: 'Remove notes.txt' }));
    expect(onRemove).toHaveBeenCalledWith('a');
  });

  it.each(['png', 'jpeg', 'webp', 'gif'])('previews a native %s image', async format => {
    const url = `data:image/${format};base64,YWJj`;
    show({ dataUrl: url }, { kind: 'image', mediaType: `image/${format}`, name: `image.${format}` });
    expect(await screen.findByRole('img')).toHaveAttribute('src', url);
  });

  it('lazily mounts the local PDF renderer with verified bytes and closes it without an iframe', async () => {
    const dataUrl = 'data:application/pdf;base64,JVBERi0=';
    const { container } = show({ dataUrl }, { kind: 'file', mediaType: 'application/pdf', name: 'report.pdf' });
    expect(await screen.findByRole('region', { name: 'Local PDF: report.pdf' })).toBeVisible();
    expect(pdfView.render).toHaveBeenLastCalledWith({ dataUrl, name: 'report.pdf' });
    expect(container.querySelector('iframe, object, embed')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Preview report.pdf' }));
    expect(screen.queryByRole('region', { name: 'Local PDF: report.pdf' })).not.toBeInTheDocument();
  });

  it('does not pass mismatched native PDF bytes to the renderer', async () => {
    pdfView.render.mockClear();
    const selected = { ...attachment, kind: 'file' as const, mediaType: 'application/pdf', name: 'report.pdf' };
    show({ attachment: { ...selected, sha256: 'wrong' }, dataUrl: 'data:application/pdf;base64,JVBERi0=' }, selected);
    expect(await screen.findByRole('alert')).toHaveTextContent('does not match');
    expect(pdfView.render).not.toHaveBeenCalled();
  });

  it.each([
    ['image/svg+xml', 'image', 'data:image/svg+xml;base64,PHN2Zz4='],
    ['text/html', 'file', 'data:text/html;base64,PGgxPg=='],
    ['image/png', 'image', 'data:text/html;base64,PGgxPg=='],
    ['image/png', 'image', 'https://example.test/image.png'],
    ['application/pdf', 'file', 'file:///private/report.pdf'],
    ['application/vnd.openxmlformats-officedocument.wordprocessingml.document', 'file', null],
  ] as const)('does not render unsupported or mismatched content (%s, %s, %s)', async (mediaType, kind, dataUrl) => {
    const { container } = show({ dataUrl }, { mediaType, kind });
    expect(await screen.findByText(/Visual preview unavailable/)).toBeVisible();
    expect(container.querySelector('iframe, img, object, embed')).toBeNull();
  });

  it('rejects a preview returned for another chat', async () => {
    show({ attachment: { ...attachment, conversationId: 'other' }, text: 'wrong chat secret' });
    expect(await screen.findByRole('alert')).toHaveTextContent('does not match');
    expect(screen.queryByText('wrong chat secret')).not.toBeInTheDocument();
  });

  it('allows retry after a preview error and discards completion after closing', async () => {
    let finish!: (value: Preview) => void;
    const previewAttachment = vi.fn<() => Promise<Preview>>()
      .mockRejectedValueOnce(new Error('Snapshot unavailable'))
      .mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }))
      .mockResolvedValue({ attachment, text: 'current', dataUrl: null });
    render(<AttachmentPreview attachment={attachment} previewAttachment={previewAttachment} />);
    fireEvent.click(screen.getByRole('button', { name: 'Preview notes.txt' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Snapshot unavailable');
    fireEvent.click(screen.getByRole('button', { name: 'Retry preview' }));
    fireEvent.click(screen.getByRole('button', { name: 'Preview notes.txt' }));
    await act(async () => finish({ attachment, text: 'late', dataUrl: null }));
    expect(screen.queryByText('late')).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Preview notes.txt' }));
    expect(await screen.findByText('current')).toBeVisible();
  });
});
