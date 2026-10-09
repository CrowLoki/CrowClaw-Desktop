import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { createRequire } from 'node:module';

// Build-time only. Installed CrowClaw serves these local assets from its bundle;
// no Node runtime, CDN, donor checkout or external font service is required.
export function loadPdfAssets() {
  const root = dirname(createRequire(import.meta.url).resolve('pdfjs-dist/package.json'));
  const assets = new Map([['pdfjs/LICENSE', readFileSync(join(root, 'LICENSE'))]]);
  for (const directory of ['cmaps', 'standard_fonts', 'wasm', 'iccs']) {
    for (const entry of readdirSync(join(root, directory), { withFileTypes: true })) {
      if (!entry.isFile()) throw new Error(`Unexpected non-file PDF asset: ${directory}/${entry.name}`);
      assets.set(`pdfjs/${directory}/${entry.name}`, readFileSync(join(root, directory, entry.name)));
    }
  }
  return assets;
}

export function pdfAssetMiddleware(assets) {
  return (request, response, next) => {
    const pathname = new URL(request.url ?? '/', 'http://localhost').pathname;
    if (!pathname.startsWith('/pdfjs/')) return next();
    if (request.method !== 'GET' && request.method !== 'HEAD') {
      response.statusCode = 405; response.end(); return;
    }
    // Exact preloaded map lookup, never an untrusted filesystem path.
    const bytes = assets.get(pathname.slice(1));
    if (!bytes) { response.statusCode = 404; response.end(); return; }
    const type = pathname.endsWith('.wasm') ? 'application/wasm'
      : /\.m?js$/.test(pathname) ? 'text/javascript; charset=utf-8'
      : pathname.endsWith('.ttf') ? 'font/ttf'
      : pathname.endsWith('.otf') ? 'font/otf'
      : pathname.endsWith('LICENSE') ? 'text/plain; charset=utf-8' : 'application/octet-stream';
    response.setHeader('Content-Type', type);
    response.setHeader('X-Content-Type-Options', 'nosniff');
    response.setHeader('Content-Length', bytes.length);
    response.end(request.method === 'HEAD' ? undefined : bytes);
  };
}

export function pdfAssetsPlugin() {
  const assets = loadPdfAssets();
  return {
    name: 'crowclaw-local-pdf-assets',
    configureServer(server) { server.middlewares.use(pdfAssetMiddleware(assets)); },
    generateBundle() {
      for (const [fileName, source] of assets) this.emitFile({ type: 'asset', fileName, source });
    },
  };
}
