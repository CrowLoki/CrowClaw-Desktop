import { useEffect, useRef, useState } from 'react';
import { AnnotationMode, getDocument, GlobalWorkerOptions } from 'pdfjs-dist';
import type { PDFDocumentLoadingTask, PDFDocumentProxy, PDFPageProxy, RenderTask } from 'pdfjs-dist';
import workerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url';
import './PdfAttachmentPreview.css';

GlobalWorkerOptions.workerSrc = workerUrl;

const MAX_BYTES = 20 * 1024 * 1024;
const MAX_PIXELS = 4_000_000;
const MAX_DIMENSION = 4096;
const TIMEOUT_MS = 30_000;
type Props = { dataUrl: string; name: string };
type Session = { pdf: PDFDocumentProxy; fail: (cause: unknown) => void };

function pdfBytes(dataUrl: string): Uint8Array {
  const prefix = 'data:application/pdf;base64,';
  if (!dataUrl.startsWith(prefix)) throw new Error('Only a verified PDF data URL can be previewed.');
  const encodedLength = dataUrl.length - prefix.length;
  if (encodedLength > Math.ceil(MAX_BYTES / 3) * 4) throw new Error('PDF preview is limited to 20 MiB.');
  const encoded = dataUrl.slice(prefix.length);
  if (!encoded || encoded.length % 4 !== 0 || !/^[A-Za-z0-9+/]+={0,2}$/.test(encoded)) {
    throw new Error('The PDF data is invalid or corrupt.');
  }
  const padding = encoded.endsWith('==') ? 2 : encoded.endsWith('=') ? 1 : 0;
  if (encodedLength / 4 * 3 - padding > MAX_BYTES) throw new Error('PDF preview is limited to 20 MiB.');
  const binary = atob(encoded);
  return Uint8Array.from(binary, character => character.charCodeAt(0));
}

function failureMessage(cause: unknown): string {
  const error = cause as { name?: string; message?: string } | null;
  if (error?.name === 'PasswordException') return 'This PDF is password-protected and cannot be previewed here.';
  if (error?.name === 'InvalidPDFException') return 'This PDF is invalid or corrupt and cannot be previewed.';
  return error?.message || 'The PDF preview could not be loaded.';
}

// A new keyed session removes old content immediately when the document changes.
export default function PdfAttachmentPreview(props: Props) {
  const [attempt, setAttempt] = useState(0);
  return <PdfSession key={`${attempt}:${props.dataUrl}`} {...props} onRetry={() => setAttempt(value => value + 1)} />;
}

function PdfSession({ dataUrl, name, onRetry }: Props & { onRetry: () => void }) {
  const [session, setSession] = useState<Session | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pageNumber, setPageNumber] = useState(1);

  useEffect(() => {
    let active = true;
    let task: PDFDocumentLoadingTask | undefined;
    let destroyed = false;
    const destroy = () => {
      if (task && !destroyed) {
        destroyed = true;
        // PDF.js owns the worker and all document resources through this task.
        void task.destroy().catch(() => undefined);
      }
    };
    const timer = window.setTimeout(() => fail(new Error('Loading the PDF timed out. Retry the preview.')), TIMEOUT_MS);
    function fail(cause: unknown) {
      if (!active) return;
      active = false;
      window.clearTimeout(timer);
      setError(failureMessage(cause));
      setSession(null);
      destroy();
    }
    try {
      const data = pdfBytes(dataUrl);
      task = getDocument({
        data,
        enableXfa: false,
        useSystemFonts: false,
        stopAtErrors: true,
        cMapUrl: new URL('pdfjs/cmaps/', document.baseURI).href,
        cMapPacked: true,
        standardFontDataUrl: new URL('pdfjs/standard_fonts/', document.baseURI).href,
        wasmUrl: new URL('pdfjs/wasm/', document.baseURI).href,
        iccUrl: new URL('pdfjs/iccs/', document.baseURI).href,
        canvasMaxAreaInBytes: MAX_PIXELS * 4,
      });
      // No scripting manager, annotation layer, links, attachments or actions.
      // Leaving onPassword unset makes PDF.js reject instead of waiting for input.
      void task.promise.then(pdf => {
        if (!active) return;
        if (!Number.isSafeInteger(pdf.numPages) || pdf.numPages < 1) throw new Error('This PDF has no readable pages.');
        if (pdf.isPureXfa) throw new Error('This PDF uses XFA forms, which this preview does not display.');
        window.clearTimeout(timer);
        setSession({ pdf, fail });
      }).catch(fail);
    } catch (cause) {
      fail(cause);
    }
    return () => {
      active = false;
      window.clearTimeout(timer);
      destroy();
    };
  }, [dataUrl]);

  return <section className="pdf-attachment-preview" aria-label={`PDF preview: ${name}`}>
    <p>Static page preview. Annotations, forms, actions and embedded files are not shown. Page text may differ from the visual reading order; scanned pages may have no selectable text.</p>
    {error ? <div role="alert">{error} <button type="button" onClick={onRetry}>Retry PDF preview</button></div>
      : !session ? <p role="status">Loading PDF…</p>
      : <>
        <nav className="pdf-attachment-preview__navigation" aria-label={`PDF pages: ${name}`}>
          <button type="button" disabled={pageNumber === 1} onClick={() => setPageNumber(value => value - 1)}>Previous</button>
          <span aria-live="polite">Page {pageNumber} of {session.pdf.numPages}</span>
          <button type="button" disabled={pageNumber === session.pdf.numPages} onClick={() => setPageNumber(value => value + 1)}>Next</button>
        </nav>
        <PdfPage key={pageNumber} session={session} pageNumber={pageNumber} />
      </>}
  </section>;
}

function PdfPage({ session, pageNumber }: { session: Session; pageNumber: number }) {
  const host = useRef<HTMLDivElement>(null);
  const [text, setText] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    let page: PDFPageProxy | undefined;
    let renderTask: RenderTask | undefined;
    // Never reuse a canvas across renders, including StrictMode effect replay.
    const canvas = document.createElement('canvas');
    const release = () => {
      renderTask?.cancel();
      canvas.remove();
      const settled = renderTask?.promise.catch(() => undefined) ?? Promise.resolve();
      void settled.then(() => {
        page?.cleanup();
        canvas.width = canvas.height = 0;
      });
    };
    const timer = window.setTimeout(() => {
      if (!active) return;
      active = false;
      release();
      session.fail(new Error(`Page ${pageNumber} timed out while loading or rendering. Retry the preview.`));
    }, TIMEOUT_MS);

    void (async () => {
      const loadedPage = await session.pdf.getPage(pageNumber);
      if (!active) { loadedPage.cleanup(); return; }
      page = loadedPage;
      const original = page.getViewport({ scale: 1 });
      const { width, height } = original;
      if (![width, height].every(value => Number.isFinite(value) && value > 0)) {
        throw new Error(`Page ${pageNumber} has invalid dimensions.`);
      }
      const scale = Math.min(1.5, MAX_DIMENSION / width, MAX_DIMENSION / height, Math.sqrt(MAX_PIXELS / width / height));
      const viewport = page.getViewport({ scale });
      canvas.width = Math.floor(viewport.width);
      canvas.height = Math.floor(viewport.height);
      if (canvas.width < 1 || canvas.height < 1 || canvas.width > MAX_DIMENSION || canvas.height > MAX_DIMENSION
        || canvas.width * canvas.height > MAX_PIXELS) throw new Error(`Page ${pageNumber} cannot fit within the preview canvas limits.`);
      const context = canvas.getContext('2d');
      if (!context) throw new Error('Canvas rendering is unavailable. This PDF cannot be previewed here.');
      canvas.setAttribute('role', 'img');
      canvas.setAttribute('aria-label', `PDF page ${pageNumber}; selectable text follows`);
      renderTask = page.render({ canvas, canvasContext: context, viewport, annotationMode: AnnotationMode.DISABLE });
      const [, content] = await Promise.all([renderTask.promise, page.getTextContent()]);
      if (!active) return;
      const plainText = content.items.map(item => 'str' in item ? item.str + (item.hasEOL ? '\n' : ' ') : '').join('');
      window.clearTimeout(timer);
      host.current?.append(canvas);
      setText(plainText);
    })().catch(cause => {
      if (!active) return;
      active = false;
      window.clearTimeout(timer);
      release();
      session.fail(cause);
    });
    return () => {
      active = false;
      window.clearTimeout(timer);
      release();
    };
  }, [session, pageNumber]);

  return <div aria-busy={text === null}>
    {text === null && <p role="status">Loading page {pageNumber}…</p>}
    <div className="pdf-attachment-preview__canvas" ref={host} />
    {text !== null && <details>
      <summary>Page {pageNumber} selectable text</summary>
      <section aria-label={`Selectable text for page ${pageNumber}`}>
      {text.trim() ? <pre tabIndex={0}>{text}</pre> : <p>No selectable text was found on this page.</p>}
      </section>
    </details>}
  </div>;
}
