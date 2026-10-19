// SPDX-License-Identifier: GPL-2.0-only
// Y2Linux error report receiver (docs/planning/ERROR-REPORTS-PLAN.md, R3).
//
// One route: POST /v1/report. It accepts a player's scrubbed report (a gzip
// tar, at most 2 MiB), checks it and stores it in the private R2 bucket. It has
// no read endpoint and stores no IP address.
//
// Cost (tools/reports/README.md): Workers and Durable Objects on the Free plan
// only fail past their limits; R2 bills beyond its free tier. The upload key is
// public (it is in every image), so the protection is the Quota object: one
// strongly consistent daily budget for all stored reports (DAILY_REPORTS,
// DAILY_BYTES; with the bucket's 180-day expiry at most ≈7 GiB, inside R2's
// free 10 GB), and per install ID and per network (a salted hash of the IPv4
// address or IPv6 /64 that changes daily) within it.

export const MAX_BYTES = 2 * 1024 * 1024;
export const PER_INSTALL_PER_DAY = 10;
export const PER_ADDRESS_PER_DAY = 30;
export const DAILY_REPORTS = 100;
export const DAILY_BYTES = 40 * 1024 * 1024;
const SCHEMA = '1';
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const SHA256 = /^[0-9a-f]{64}$/;
const TRIGGERS = new Set(['manual', 'kernel_crash', 'kernel_warning', 'unclean_restart', 'reborn_crash',
  'reborn_failure', 'update_failure', 'sleep_failure', 'radio_failure', 'display_failure', 'storage_error']);
const RELEASE = /^[0-9A-Za-z.+-]{1,64}$/;

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

// The network an address belongs to: an IPv4 address itself, an IPv6 /64 (one
// connection hands out a whole /64, so single IPv6 addresses cost nothing).
export function network(address) {
  if (!address.includes(':')) return address;
  const [head, tail = ''] = address.toLowerCase().split('::');
  const left = head ? head.split(':') : [];
  const right = address.includes('::') && tail ? tail.split(':') : [];
  const groups = address.includes('::')
    ? [...left, ...Array(Math.max(0, 8 - left.length - right.length)).fill('0'), ...right]
    : left;
  return groups.slice(0, 4).map((g) => g.replace(/^0+(?=.)/, '')).join(':') + '::/64';
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

// The daily budget, in one Durable Object: its storage calls run one request
// at a time, so the counts are exact however many uploads arrive at once.
// Everything from earlier days is deleted on the first request of a new day.
export class Quota {
  constructor(state) {
    this.storage = state.storage;
  }

  async fetch(request) {
    const { day, install, address, bytes } = await request.json();
    if ((await this.storage.get('day')) !== day) {
      await this.storage.deleteAll();
      await this.storage.put('day', day);
    }
    const total = (await this.storage.get('total')) ?? { reports: 0, bytes: 0 };
    const installs = (await this.storage.get(`install:${install}`)) ?? 0;
    const addresses = (await this.storage.get(`address:${address}`)) ?? 0;
    let error = null;
    if (total.reports >= DAILY_REPORTS || total.bytes + bytes > DAILY_BYTES) error = 'daily_budget';
    else if (installs >= PER_INSTALL_PER_DAY) error = 'install_daily_limit';
    else if (addresses >= PER_ADDRESS_PER_DAY) error = 'address_daily_limit';
    if (!error) {
      await this.storage.put({
        total: { reports: total.reports + 1, bytes: total.bytes + bytes },
        [`install:${install}`]: installs + 1,
        [`address:${address}`]: addresses + 1,
      });
    }
    return new Response(JSON.stringify({ ok: !error, error }), { headers: { 'content-type': 'application/json' } });
  }
}

async function admit(env, fields) {
  const quota = env.QUOTA.get(env.QUOTA.idFromName('daily'));
  const response = await quota.fetch('https://quota/admit', { method: 'POST', body: JSON.stringify(fields) });
  return response.json();
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

  const body = await readLimited(request, MAX_BYTES);
  if (body === null) return answer(413, { error: 'too_large' });
  if (body.byteLength !== declared) return answer(400, { error: 'length_mismatch' });
  if (body[0] !== 0x1f || body[1] !== 0x8b) return answer(400, { error: 'not_gzip' });
  if ((await sha256(body)) !== digest) return answer(400, { error: 'sha256_mismatch' });

  const day = now.toISOString().slice(0, 10);
  const key = `reports/${day}/${install}/${report}.tar.gz`;
  // A retried upload of a stored report is a success, not a second copy (an
  // R2 lookup is a free-tier class B read; nothing is counted).
  if (await env.REPORTS.head(key)) return answer(200, { report_id: report, status: 'duplicate' });
  // Only a valid report counts: a broken upload the player retries uses nothing.
  const address = await sha256(new TextEncoder().encode(
    `${env.RATE_SALT ?? ''}|${day}|${network(request.headers.get('cf-connecting-ip') ?? '')}`));
  const admitted = await admit(env, { day, install, address, bytes: body.byteLength });
  if (!admitted.ok) return answer(429, { error: admitted.error });

  await env.REPORTS.put(key, body, {
    httpMetadata: { contentType: 'application/gzip' },
    customMetadata: { schema: SCHEMA, trigger, release, bytes: String(body.byteLength), sha256: digest },
  });
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
