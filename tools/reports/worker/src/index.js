// SPDX-License-Identifier: GPL-2.0-only
// Y2Linux error report receiver (docs/planning/ERROR-REPORTS-PLAN.md, R3).
//
// One route: POST /v1/report. It accepts a player's scrubbed report (a gzip
// tar, at most 8 MiB), checks it and stores it in the private R2 bucket. It has
// no read endpoint and stores no IP address: rate limits key on the install ID
// and on a salted, daily-changing hash of the address that expires with the day.

export const MAX_BYTES = 8 * 1024 * 1024;
export const PER_INSTALL_PER_DAY = 10;
export const PER_ADDRESS_PER_DAY = 30;
const SCHEMA = '1';
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const SHA256 = /^[0-9a-f]{64}$/;
const TRIGGERS = new Set(['manual', 'kernel_crash', 'kernel_warning', 'unclean_restart', 'reborn_crash',
  'reborn_failure', 'update_failure', 'sleep_failure', 'radio_failure', 'display_failure', 'storage_error']);
const RELEASE = /^[0-9A-Za-z.+-]{1,64}$/;
const DAY_SECONDS = 86400;

function answer(status, body) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json', 'cache-control': 'no-store' },
  });
}

function hex(buffer) {
  return [...new Uint8Array(buffer)].map((b) => b.toString(16).padStart(2, '0')).join('');
}

async function sha256(data) {
  return hex(await crypto.subtle.digest('SHA-256', data));
}

// Equal-time comparison: the upload key must not leak through timing.
function same(a, b) {
  const x = new TextEncoder().encode(a);
  const y = new TextEncoder().encode(b);
  let diff = x.length ^ y.length;
  for (let i = 0; i < Math.max(x.length, y.length); i++) diff |= (x[i] ?? 0) ^ (y[i] ?? 0);
  return diff === 0;
}

// Read at most `limit` bytes; null when the body is larger (no full buffering).
async function readLimited(request, limit) {
  if (!request.body) return new Uint8Array();
  const reader = request.body.getReader();
  const chunks = [];
  let size = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.byteLength;
    if (size > limit) {
      await reader.cancel();
      return null;
    }
    chunks.push(value);
  }
  const body = new Uint8Array(size);
  let offset = 0;
  for (const chunk of chunks) {
    body.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return body;
}

// A counter per key and day in KV; approximate under concurrency, which is
// enough for abuse limits.
async function overLimit(kv, key, limit) {
  const count = Number((await kv.get(key)) ?? '0');
  if (count >= limit) return true;
  await kv.put(key, String(count + 1), { expirationTtl: 2 * DAY_SECONDS });
  return false;
}

export async function handle(request, env, now = new Date()) {
  const url = new URL(request.url);
  if (url.pathname !== '/v1/report') return answer(404, { error: 'not_found' });
  if (request.method !== 'POST') return answer(405, { error: 'method_not_allowed' });
  const h = (name) => request.headers.get(name) ?? '';
  if (!env.UPLOAD_KEY || !same(h('x-y2-upload-key'), env.UPLOAD_KEY)) return answer(403, { error: 'forbidden' });
  if (h('x-y2-schema') !== SCHEMA) return answer(400, { error: 'schema' });
  const install = h('x-y2-install-id');
  const report = h('x-y2-report-id');
  const digest = h('x-y2-sha256');
  const trigger = h('x-y2-trigger');
  const release = h('x-y2-release');
  if (!UUID.test(install) || !UUID.test(report)) return answer(400, { error: 'id' });
  if (!SHA256.test(digest)) return answer(400, { error: 'sha256' });
  if (!TRIGGERS.has(trigger)) return answer(400, { error: 'trigger' });
  if (release && !RELEASE.test(release)) return answer(400, { error: 'release' });
  const declared = Number(h('content-length'));
  if (!Number.isFinite(declared) || declared <= 0) return answer(411, { error: 'length_required' });
  if (declared > MAX_BYTES) return answer(413, { error: 'too_large' });

  const day = now.toISOString().slice(0, 10);
  // A retried upload of a stored report is a success, not a second copy.
  if (await env.RATES.get(`seen:${report}`)) return answer(200, { report_id: report, status: 'duplicate' });

  const body = await readLimited(request, MAX_BYTES);
  if (body === null) return answer(413, { error: 'too_large' });
  if (body.byteLength !== declared) return answer(400, { error: 'length_mismatch' });
  if (body[0] !== 0x1f || body[1] !== 0x8b) return answer(400, { error: 'not_gzip' });
  if ((await sha256(body)) !== digest) return answer(400, { error: 'sha256_mismatch' });

  // Only a valid report counts towards the limits: a broken upload that the
  // player retries must not use up its day.
  if (await overLimit(env.RATES, `install:${install}:${day}`, PER_INSTALL_PER_DAY)) {
    return answer(429, { error: 'install_daily_limit' });
  }
  const address = request.headers.get('cf-connecting-ip') ?? '';
  const addressKey = await sha256(new TextEncoder().encode(`${env.RATE_SALT ?? ''}|${day}|${address}`));
  if (await overLimit(env.RATES, `address:${addressKey}:${day}`, PER_ADDRESS_PER_DAY)) {
    return answer(429, { error: 'address_daily_limit' });
  }

  const key = `reports/${day}/${install}/${report}.tar.gz`;
  await env.REPORTS.put(key, body, {
    httpMetadata: { contentType: 'application/gzip' },
    customMetadata: { schema: SCHEMA, trigger, release, bytes: String(body.byteLength), sha256: digest },
  });
  await env.RATES.put(`seen:${report}`, key, { expirationTtl: 7 * DAY_SECONDS });
  return answer(201, { report_id: report, status: 'stored' });
}

export default {
  async fetch(request, env) {
    try {
      return await handle(request, env);
    } catch {
      return answer(500, { error: 'internal' });
    }
  },
};
