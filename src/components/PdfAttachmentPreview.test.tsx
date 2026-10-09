import { StrictMode } from 'react';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import PdfAttachmentPreview from './PdfAttachmentPreview';

const renderer = vi.hoisted(() => ({ getDocument: vi.fn(), workerOptions: { workerSrc: '' } }));
vi.mock('pdfjs-dist', () => ({
  getDocument: renderer.getDocument,
  GlobalWorkerOptions: renderer.workerOptions,
  AnnotationMode: { DISABLE: 0 },
}));
vi.mock('pdfjs-dist/build/pdf.worker.min.mjs?url', () => ({ default: '/assets/local-pdf-worker.mjs' }));

const dataUrl = 'data:application/pdf;base64,JVBERi0=';
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (cause: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function page(text = 'PAPER-9157', width = 612, height = 792) {
  const painting = { promise: Promise.resolve(), cancel: vi.fn() };
  return {
    getViewport: vi.fn(({ scale }: { scale: number }) => ({ width: width * scale, height: height * scale })),
    render: vi.fn((_parameters: { canvas: HTMLCanvasElement; annotationMode: number }) => painting),
    getTextContent: vi.fn(async () => ({ items: [{ str: text, hasEOL: true }] })),
    cleanup: vi.fn(),
    painting,
  };
}
function documentTask(pages = [page()]) {
  const pdf = { numPages: pages.length, isPureXfa: false, getPage: vi.fn(async (number: number) => pages[number - 1]) };
  const task = { promise: Promise.resolve(pdf), destroy: vi.fn(async () => {}) };
  renderer.getDocument.mockReturnValue(task);
  return { pdf, task, pages };
}
async function flush() { await act(async () => { await Promise.resolve(); }); }
async function ready(number = 1) {
  return screen.findByRole('img', { name: `PDF page ${number}; selectable text follows` });
}

beforeEach(() => {
  renderer.getDocument.mockReset();
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue({} as CanvasRenderingContext2D);
});
afterEach(() => { vi.useRealTimers(); });

describe('PdfAttachmentPreview', () => {
  it('uses bytes, a packaged worker and base-relative local assets; renders literal selectable text and no active PDF content', async () => {
    const text = '<script>alert(1)</script> https://external.test';
    const source = documentTask([page(text)]);
    const { container, unmount } = render(<PdfAttachmentPreview dataUrl={dataUrl} name="report.pdf" />);
    await ready();
    const options = renderer.getDocument.mock.calls[0][0];
    expect(Array.from(options.data)).toEqual([37, 80, 68, 70, 45]);
    expect(options).toMatchObject({ enableXfa: false, useSystemFonts: false, stopAtErrors: true, cMapPacked: true });
    expect(options.url).toBeUndefined();
    for (const [key, folder] of Object.entries({ cMapUrl: 'cmaps', standardFontDataUrl: 'standard_fonts', wasmUrl: 'wasm', iccUrl: 'iccs' })) {
      expect(options[key]).toBe(new URL(`pdfjs/${folder}/`, document.baseURI).href);
    }
    expect(renderer.workerOptions.workerSrc).toBe('/assets/local-pdf-worker.mjs');
    expect(source.pages[0].render.mock.calls[0][0]).toMatchObject({ annotationMode: 0 });
    expect(container.querySelector('script, iframe, object, embed, a')).toBeNull();
    const textToggle = await screen.findByText('Page 1 selectable text');
    expect(textToggle.closest('details')).not.toHaveAttribute('open');
    fireEvent.click(textToggle);
    expect(screen.getByText(text)).toBeVisible();
    expect(screen.getByText(text).tagName).toBe('PRE');
    expect(screen.getByText(/Annotations, forms, actions and embedded files are not shown/)).toBeVisible();
    expect(screen.getByRole('button', { name: 'Previous' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Next' })).toBeDisabled();
    unmount();
    await flush();
    expect(source.task.destroy).toHaveBeenCalledTimes(1);
    expect(source.pages[0].cleanup).toHaveBeenCalled();
  });

  it('navigates a single current page with a fresh canvas and cancels the previous render', async () => {
    const source = documentTask([page('first'), page('second')]);
    const { container } = render(<PdfAttachmentPreview dataUrl={dataUrl} name="report.pdf" />);
    const firstCanvas = await ready();
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));
    const secondCanvas = await ready(2);
    expect(secondCanvas).not.toBe(firstCanvas);
    expect(container.querySelectorAll('canvas')).toHaveLength(1);
    expect(screen.getByText('Page 2 of 2')).toBeVisible();
    expect(screen.queryByText('first')).not.toBeInTheDocument();
    expect(source.pages[0].painting.cancel).toHaveBeenCalled();
    expect(source.pages[0].cleanup).toHaveBeenCalled();
    expect(source.task.destroy).not.toHaveBeenCalled();
    expect(renderer.getDocument).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole('button', { name: 'Previous' }));
    expect(await ready()).not.toBe(firstCanvas);
  });

  it('ignores a late render and text outcome after navigating away', async () => {
    const pendingRender = deferred<void>();
    const pendingText = deferred<{ items: { str: string; hasEOL: boolean }[] }>();
    const first = page();
    first.painting.promise = pendingRender.promise;
    first.getTextContent.mockReturnValue(pendingText.promise);
    documentTask([first, page('current')]);
    render(<PdfAttachmentPreview dataUrl={dataUrl} name="report.pdf" />);
    await waitFor(() => expect(first.render).toHaveBeenCalled());
    const abandonedCanvas = (first.render.mock.calls[0] as unknown as [{ canvas: HTMLCanvasElement }])[0].canvas;
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));
    const currentCanvas = await ready(2);
    await act(async () => {
      pendingRender.resolve();
      pendingText.resolve({ items: [{ str: 'stale', hasEOL: true }] });
    });
    expect(screen.queryByText('stale')).not.toBeInTheDocument();
    expect(currentCanvas).toBeInTheDocument();
    expect(abandonedCanvas).not.toBeInTheDocument();
    expect(abandonedCanvas.width).toBe(0);
  });

  it('destroys a changed document and ignores its late loading success', async () => {
    const old = documentTask();
    const pending = deferred<typeof old.pdf>();
    old.task.promise = pending.promise;
    const { rerender } = render(<PdfAttachmentPreview dataUrl={dataUrl} name="old.pdf" />);
    const current = documentTask([page('current')]);
    rerender(<PdfAttachmentPreview dataUrl="data:application/pdf;base64,JVBERi0x" name="new.pdf" />);
    await ready();
    await act(async () => pending.resolve(old.pdf));
    expect(old.task.destroy).toHaveBeenCalledTimes(1);
    expect(old.pdf.getPage).not.toHaveBeenCalled();
    expect(current.pdf.getPage).toHaveBeenCalledWith(1);
  });

  it('cleans up a page that arrives after unmount without rendering it', async () => {
    const source = documentTask();
    const pending = deferred<ReturnType<typeof page>>();
    source.pdf.getPage.mockReturnValue(pending.promise);
    const { unmount } = render(<PdfAttachmentPreview dataUrl={dataUrl} name="report.pdf" />);
    await waitFor(() => expect(source.pdf.getPage).toHaveBeenCalled());
    unmount();
    await act(async () => pending.resolve(source.pages[0]));
    expect(source.pages[0].render).not.toHaveBeenCalled();
    expect(source.pages[0].cleanup).toHaveBeenCalled();
    expect(source.task.destroy).toHaveBeenCalledTimes(1);
  });

  it('ignores late document rejection after unmount', async () => {
    const source = documentTask();
    const pending = deferred<typeof source.pdf>();
    source.task.promise = pending.promise;
    const { unmount } = render(<PdfAttachmentPreview dataUrl={dataUrl} name="report.pdf" />);
    unmount();
    await act(async () => pending.reject(new Error('late failure')));
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    expect(source.task.destroy).toHaveBeenCalledTimes(1);
  });

  it.each(['PasswordException', 'InvalidPDFException'])('reports %s and retries with a new document task', async name => {
    const source = documentTask();
    source.task.promise = Promise.reject(Object.assign(new Error('internal details'), { name }));
    render(<PdfAttachmentPreview dataUrl={dataUrl} name="report.pdf" />);
    expect(await screen.findByRole('alert')).toHaveTextContent(name === 'PasswordException' ? 'password-protected' : 'invalid or corrupt');
    expect(source.task.destroy).toHaveBeenCalledTimes(1);
    documentTask();
    fireEvent.click(screen.getByRole('button', { name: 'Retry PDF preview' }));
    await ready();
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it.each(['document', 'page', 'render', 'text'])('times out stalled %s work, destroys the document, and ignores late success', async stage => {
    vi.useFakeTimers();
    const source = documentTask();
    const pendingDocument = deferred<typeof source.pdf>();
    const pendingPage = deferred<ReturnType<typeof page>>();
    const pendingRender = deferred<void>();
    const pendingText = deferred<{ items: { str: string; hasEOL: boolean }[] }>();
    if (stage === 'document') source.task.promise = pendingDocument.promise;
    if (stage === 'page') source.pdf.getPage.mockReturnValue(pendingPage.promise);
    if (stage === 'render') source.pages[0].painting.promise = pendingRender.promise;
    if (stage === 'text') source.pages[0].getTextContent.mockReturnValue(pendingText.promise);
    const { container } = render(<PdfAttachmentPreview dataUrl={dataUrl} name="report.pdf" />);
    await flush();
    await act(async () => { await vi.advanceTimersByTimeAsync(30_000); });
    expect(screen.getByRole('alert')).toHaveTextContent('timed out');
    expect(source.task.destroy).toHaveBeenCalledTimes(1);
    if (stage === 'render' || stage === 'text') expect(source.pages[0].painting.cancel).toHaveBeenCalled();
    await act(async () => {
      pendingDocument.resolve(source.pdf);
      pendingPage.resolve(source.pages[0]);
      pendingRender.resolve();
      pendingText.resolve({ items: [{ str: 'late', hasEOL: true }] });
    });
    expect(container.querySelector('canvas')).toBeNull();
    expect(screen.queryByText('late')).not.toBeInTheDocument();
  });

  it.each(['page', 'render', 'text'])('reports %s failures and destroys resources instead of presenting partial success', async stage => {
    const source = documentTask();
    if (stage === 'page') source.pdf.getPage.mockRejectedValue(new Error('Page failed'));
    if (stage === 'render') source.pages[0].render.mockImplementation(() => ({ promise: Promise.reject(new Error('Render failed')), cancel: vi.fn() }));
    if (stage === 'text') source.pages[0].getTextContent.mockRejectedValue(new Error('Text failed'));
    const { container } = render(<PdfAttachmentPreview dataUrl={dataUrl} name="report.pdf" />);
    expect(await screen.findByRole('alert')).toHaveTextContent(/failed/);
    expect(source.task.destroy).toHaveBeenCalledTimes(1);
    expect(container.querySelector('canvas')).toBeNull();
  });

  it.each(['https://example.test/report.pdf', 'data:text/html;base64,JVBERi0=', 'data:application/pdf;base64,invalid!', 'data:application/pdf;base64,'])('rejects unsupported or malformed input before invoking PDF.js (%s)', async dataUrl => {
    render(<PdfAttachmentPreview dataUrl={dataUrl} name="report.pdf" />);
    expect(await screen.findByRole('alert')).toBeVisible();
    expect(renderer.getDocument).not.toHaveBeenCalled();
  });

  it('rejects input larger than 20 MiB before decoding', async () => {
    const decode = vi.spyOn(window, 'atob');
    const oversized = 'data:application/pdf;base64,' + 'A'.repeat(Math.ceil((20 * 1024 * 1024 + 1) / 3) * 4);
    render(<PdfAttachmentPreview dataUrl={oversized} name="large.pdf" />);
    expect(await screen.findByRole('alert')).toHaveTextContent('20 MiB');
    expect(decode).not.toHaveBeenCalled();
    expect(renderer.getDocument).not.toHaveBeenCalled();
  });

  it.each([[100_000, 100_000], [100_000, 1000]])('fits the entire %sx%s page inside 4MP and 4096px bounds', async (width, height) => {
    documentTask([page('large page', width, height)]);
    render(<PdfAttachmentPreview dataUrl={dataUrl} name="large.pdf" />);
    const canvas = await ready() as HTMLCanvasElement;
    expect(canvas.width * canvas.height).toBeLessThanOrEqual(4_000_000);
    expect(Math.max(canvas.width, canvas.height)).toBeLessThanOrEqual(4096);
    expect(canvas.width).toBeGreaterThan(0);
    expect(canvas.height).toBeGreaterThan(0);
  });

  it('rejects invalid page dimensions', async () => {
    documentTask([page('invalid', Infinity, 792)]);
    render(<PdfAttachmentPreview dataUrl={dataUrl} name="report.pdf" />);
    expect(await screen.findByRole('alert')).toHaveTextContent('invalid dimensions');
  });

  it('reports missing canvas support', async () => {
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null);
    const source = documentTask();
    render(<PdfAttachmentPreview dataUrl={dataUrl} name="report.pdf" />);
    expect(await screen.findByRole('alert')).toHaveTextContent('Canvas rendering is unavailable');
    expect(source.task.destroy).toHaveBeenCalledTimes(1);
  });

  it('does not label an image-only page as having extracted text', async () => {
    documentTask([page('')]);
    render(<PdfAttachmentPreview dataUrl={dataUrl} name="scan.pdf" />);
    await ready();
    fireEvent.click(screen.getByText('Page 1 selectable text'));
    expect(screen.getByText('No selectable text was found on this page.')).toBeVisible();
  });

  it('survives StrictMode effect replay using separate loading tasks', async () => {
    const old = documentTask();
    const current = documentTask();
    renderer.getDocument.mockReset().mockReturnValueOnce(old.task).mockReturnValueOnce(current.task);
    const { unmount } = render(<StrictMode><PdfAttachmentPreview dataUrl={dataUrl} name="report.pdf" /></StrictMode>);
    await ready();
    expect(old.task.destroy).toHaveBeenCalledTimes(1);
    expect(old.pdf.getPage).not.toHaveBeenCalled();
    unmount();
    expect(current.task.destroy).toHaveBeenCalledTimes(1);
  });
});
