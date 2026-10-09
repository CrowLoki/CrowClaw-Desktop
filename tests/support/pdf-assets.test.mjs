import assert from 'node:assert/strict';
import { test } from 'node:test';
import { loadPdfAssets, pdfAssetMiddleware, pdfAssetsPlugin } from '../../scripts/pdf-assets.mjs';

test('PDF support resources and license are emitted into the standalone build', () => {
  const assets = loadPdfAssets();
  for (const folder of ['cmaps', 'standard_fonts', 'wasm', 'iccs']) {
    assert.ok([...assets.keys()].some(key => key.startsWith(`pdfjs/${folder}/`)));
  }
  assert.match(assets.get('pdfjs/LICENSE').toString(), /Apache License/);
  const emitted = new Map();
  pdfAssetsPlugin().generateBundle.call({ emitFile(asset) { emitted.set(asset.fileName, asset.source); } });
  assert.deepEqual([...emitted.keys()].sort(), [...assets.keys()].sort());
  for (const [name, bytes] of emitted) { assert.ok(name.startsWith('pdfjs/')); assert.ok(bytes.length > 0); }
});

test('PDF development serving uses exact packaged names with no path traversal', () => {
  const bytes = Buffer.from([1,2,3]);
  const middleware = pdfAssetMiddleware(new Map([['pdfjs/wasm/test.wasm',bytes],['pdfjs/wasm/fallback.js',bytes]]));
  function request(url, method = 'GET') {
    const result = { statusCode:200,headers:{},body:null,next:false };
    middleware({url,method},{ get statusCode(){return result.statusCode;},set statusCode(value){result.statusCode=value;},setHeader(k,v){result.headers[k]=v;},end(body){result.body=body;} },()=>{result.next=true;});
    return result;
  }
  const hit=request('/pdfjs/wasm/test.wasm');
  assert.equal(hit.statusCode,200);assert.deepEqual(hit.body,bytes);
  assert.equal(hit.headers['Content-Type'],'application/wasm');
  assert.equal(hit.headers['X-Content-Type-Options'],'nosniff');
  assert.equal(request('/pdfjs/wasm/fallback.js').headers['Content-Type'],'text/javascript; charset=utf-8');
  assert.equal(request('/pdfjs/wasm/test.wasm','HEAD').body,undefined);
  assert.equal(request('/pdfjs/wasm/test.wasm','POST').statusCode,405);
  assert.equal(request('/pdfjs/%2e%2e%2fpackage.json').statusCode,404);
  assert.equal(request('/pdfjs/missing').statusCode,404);
  assert.equal(request('/src/main.tsx').next,true);
});
