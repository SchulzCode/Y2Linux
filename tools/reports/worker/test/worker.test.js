// SPDX-License-Identifier: GPL-2.0-only
// The receiver against a fake R2 bucket and KV namespace: `node --test` in
// tools/reports/worker (Node 20 or newer: fetch Request/Response and WebCrypto).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createHash, randomUUID } from 'node:crypto';
import { gzipSync } from 'node:zlib';
import worker, { handle, MAX_BYTES, PER_INSTALL_PER_DAY, PER_ADDRESS_PER_DAY } from '../src/index.js';

const KEY = 'test-upload-key-0123456789abcdef0123456789';
const NOW = new Date('2026-10-19T12:00:00Z');

function environment() {
  const kv = new Map();
  const r2 = new Map();
  return {
    kv,
    r2,
    env: {
      UPLOAD_KEY: KEY,
      RATE_SALT: 'salt',
      RATES: {
        async get(key) { return kv.has(key) ? kv.get(key).value : null; },
        async put(key, value, options) { kv.set(key, { value, options }); },
      },
      REPORTS: {
        async put(key, body, options) { r2.set(key, { body: new Uint8Array(body), options }); },
      },
    },
  };
}

function report(overrides = {}) {
  const body = overrides.body ?? gzipSync(Buffer.from('report.json {}'));
  const headers = {
    'x-y2-upload-key': KEY,
    'x-y2-schema': '1',
    'x-y2-install-id': randomUUID(),
    'x-y2-report-id': randomUUID(),
    'x-y2-sha256': createHash('sha256').update(body).digest('hex'),
    'x-y2-trigger': 'manual',
    'x-y2-release': '1.0.0-beta.8.9',
    'content-length': String(body.byteLength),
    'cf-connecting-ip': '203.0.113.7',
    ...overrides.headers,
  };
  for (const [name, value] of Object.entries(headers)) if (value === undefined) delete headers[name];
  return new Request(overrides.url ?? 'https://reports.schulzcode.dev/v1/report', {
    method: overrides.method ?? 'POST',
    headers,
    body: (overrides.method ?? 'POST') === 'POST' ? body : undefined,
  });
}

async function send(env, overrides) {
  const response = await handle(report(overrides), env, NOW);
  return { status: response.status, json: await response.json(), response };
}

test('a valid report is stored under its day, install and report ID with metadata', async () => {
  const { env, r2, kv } = environment();
  const request = report();
  const install = request.headers.get('x-y2-install-id');
  const id = request.headers.get('x-y2-report-id');
  const response = await handle(request, env, NOW);
  assert.equal(response.status, 201);
  assert.deepEqual(await response.json(), { report_id: id, status: 'stored' });
  assert.equal(response.headers.get('cache-control'), 'no-store');
  const key = `reports/2026-10-19/${install}/${id}.tar.gz`;
  assert.deepEqual([...r2.keys()], [key]);
  const stored = r2.get(key);
  assert.equal(stored.options.httpMetadata.contentType, 'application/gzip');
  assert.equal(stored.options.customMetadata.trigger, 'manual');
  assert.equal(stored.options.customMetadata.release, '1.0.0-beta.8.9');
  assert.equal(stored.options.customMetadata.bytes, String(stored.body.byteLength));
  // Nothing in KV or R2 holds the sender's address.
  for (const [name, entry] of [...kv, ...r2]) {
    assert.ok(!name.includes('203.0.113.7'));
    assert.ok(!JSON.stringify(entry.options ?? {}).includes('203.0.113.7'));
  }
});

test('a retried upload of a stored report is a success without a second copy', async () => {
  const { env, r2 } = environment();
  const first = report();
  const headers = Object.fromEntries(first.headers);
  assert.equal((await handle(first, env, NOW)).status, 201);
  const again = await send(env, { headers });
  assert.equal(again.status, 200);
  assert.equal(again.json.status, 'duplicate');
  assert.equal(r2.size, 1);
});

test('only POST /v1/report exists, and there is no way to read reports back', async () => {
  const { env } = environment();
  assert.equal((await send(env, { url: 'https://reports.schulzcode.dev/' })).status, 404);
  assert.equal((await send(env, { url: 'https://reports.schulzcode.dev/v1/report/x' })).status, 404);
  assert.equal((await send(env, { method: 'GET' })).status, 405);
  assert.equal((await send(env, { method: 'PUT' })).status, 405);
});

test('the upload key is required and compared exactly', async () => {
  const { env, r2 } = environment();
  assert.equal((await send(env, { headers: { 'x-y2-upload-key': undefined } })).status, 403);
  assert.equal((await send(env, { headers: { 'x-y2-upload-key': KEY.slice(0, -1) } })).status, 403);
  assert.equal((await send(env, { headers: { 'x-y2-upload-key': KEY + 'x' } })).status, 403);
  const { env: unset } = environment();
  delete unset.UPLOAD_KEY;
  assert.equal((await send(unset, {})).status, 403);
  assert.equal(r2.size, 0);
});

test('malformed headers are refused before anything is stored', async () => {
  const { env, r2 } = environment();
  const cases = [
    [{ 'x-y2-schema': '2' }, 'schema'],
    [{ 'x-y2-install-id': 'not-a-uuid' }, 'id'],
    [{ 'x-y2-report-id': randomUUID().toUpperCase() }, 'id'],
    [{ 'x-y2-sha256': 'abc' }, 'sha256'],
    [{ 'x-y2-trigger': 'weather' }, 'trigger'],
    [{ 'x-y2-release': '1.0 beta' }, 'release'],
  ];
  for (const [headers, error] of cases) {
    const result = await send(env, { headers });
    assert.equal(result.status, 400, error);
    assert.equal(result.json.error, error);
  }
  assert.equal(r2.size, 0);
});

// The same list as report.TRIGGERS on the player (tests/test_report_upload.py
// checks that the two stay equal).
test('every trigger the player knows is accepted', async () => {
  for (const trigger of ['manual', 'kernel_crash', 'kernel_warning', 'unclean_restart', 'reborn_crash',
    'reborn_failure', 'update_failure', 'sleep_failure', 'radio_failure', 'display_failure', 'storage_error']) {
    const { env } = environment();
    assert.equal((await send(env, { headers: { 'x-y2-trigger': trigger } })).status, 201, trigger);
  }
});

test('the body must be gzip, of the declared length and hash, and at most 8 MiB', async () => {
  const { env, r2 } = environment();
  const plain = Buffer.from('not gzip at all');
  assert.equal((await send(env, { body: plain })).json.error, 'not_gzip');
  const body = gzipSync(Buffer.from('x'));
  assert.equal((await send(env, { body, headers: { 'x-y2-sha256': '0'.repeat(64) } })).json.error,
    'sha256_mismatch');
  assert.equal((await send(env, { body, headers: { 'content-length': String(body.byteLength + 1) } })).json.error,
    'length_mismatch');
  assert.equal((await send(env, { body, headers: { 'content-length': String(MAX_BYTES + 1) } })).status, 413);
  assert.equal((await send(env, { body, headers: { 'content-length': '0' } })).status, 411);
  assert.equal(r2.size, 0);
});

test('a body larger than declared is cut off at the limit, not buffered', async () => {
  const { env, r2 } = environment();
  const big = new Uint8Array(MAX_BYTES + 1024);
  big[0] = 0x1f;
  big[1] = 0x8b;
  const result = await send(env, { body: big, headers: { 'content-length': '1000' } });
  assert.equal(result.status, 413);
  assert.equal(r2.size, 0);
});

test('ten reports a day per install ID; refused uploads do not count', async () => {
  const { env, r2 } = environment();
  const install = randomUUID();
  for (let i = 0; i < 5; i++) {
    assert.equal((await send(env, { body: Buffer.from('broken'), headers: { 'x-y2-install-id': install } })).status, 400);
  }
  for (let i = 0; i < PER_INSTALL_PER_DAY; i++) {
    assert.equal((await send(env, { headers: { 'x-y2-install-id': install } })).status, 201);
  }
  const over = await send(env, { headers: { 'x-y2-install-id': install } });
  assert.equal(over.status, 429);
  assert.equal(over.json.error, 'install_daily_limit');
  assert.equal(r2.size, PER_INSTALL_PER_DAY);
  // Another player on the same day is unaffected.
  assert.equal((await send(env, {})).status, 201);
});

test('thirty reports a day per address, counted under a salted daily hash', async () => {
  const { env, kv } = environment();
  for (let i = 0; i < PER_ADDRESS_PER_DAY; i++) assert.equal((await send(env, {})).status, 201);
  const over = await send(env, {});
  assert.equal(over.status, 429);
  assert.equal(over.json.error, 'address_daily_limit');
  assert.equal((await send(env, { headers: { 'cf-connecting-ip': '198.51.100.1' } })).status, 201);
  const counters = [...kv.keys()].filter((k) => k.startsWith('address:'));
  assert.equal(counters.length, 2);
  for (const name of counters) assert.match(name, /^address:[0-9a-f]{64}:2026-10-19$/);
  // Counters expire on their own.
  for (const name of counters) assert.ok(kv.get(name).options.expirationTtl > 0);
});

test('a failure inside the handler answers 500 without details', async () => {
  const { env } = environment();
  env.REPORTS.put = async () => { throw new Error('R2 is down: secret detail'); };
  const response = await worker.fetch(report(), env);
  assert.equal(response.status, 500);
  assert.deepEqual(await response.json(), { error: 'internal' });
});
