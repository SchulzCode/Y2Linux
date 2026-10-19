# Error report receiver

Players that opted in send scrubbed error reports (`tools/platform/y2_platform/report.py`) to a Cloudflare
Worker at `https://reports.schulzcode.dev/v1/report`, which keeps them in a private R2 bucket for 180 days. The
maintainer downloads them with `fetch.py`, on the PC or on the home server. Design and privacy rules:
[ERROR-REPORTS-PLAN](../../docs/planning/ERROR-REPORTS-PLAN.md); user guide: [ERROR-REPORTS](../../docs/ERROR-REPORTS.md).

| File | What it is |
| --- | --- |
| `worker/src/index.js` | The Worker: `POST /v1/report` only, no read route, no IP stored |
| `worker/wrangler.toml` | Route, R2 binding, the `Quota` Durable Object |
| `worker/test/worker.test.js` | `node --test` in `worker/` (Node 20+, no packages) |
| `new_upload_key.py` | Makes the build-wide upload key, once |
| `fetch.py` | Downloads new reports from R2 (S3 API, standard library only) |

## What the Worker checks

Upload key (equal-time compare), schema 1, UUID install and report IDs, a known trigger, the release string,
`Content-Length` ≤ 2 MiB, the body read with a hard 2 MiB limit, gzip magic, the body's SHA-256. A report it already
stored today answers 200 without a second copy. Then the daily budget, counted only for valid reports, in one Durable
Object (`Quota`: exact counts, one request at a time, earlier days deleted): **100 reports and 40 MiB a day for the
whole service**, 10 a day per install ID, 30 a day per network (IPv4 address or IPv6 /64, as a salted hash that
changes daily; the address itself is never stored). Stored as `reports/<UTC day>/<install ID>/<report ID>.tar.gz`
with schema, trigger, release, size and SHA-256 as metadata. Only `reports.schulzcode.dev` (no workers.dev or preview
URL); request logs are off.

## What it can cost (Free plan)

The upload key is in every image, so anyone can extract it. What protects the account is that nothing can grow
without a bound:

| Service | Free allowance | Past it | This receiver at most |
| --- | --- | --- | --- |
| Workers requests | 100 000 a day (all Workers of the account), 10 ms CPU each | error 1027, no charge | a flood is stopped earlier by the WAF rate limit (deploy step 6) |
| Durable Objects (SQLite) | 100 000 requests, 100 000 rows written a day, 5 GB | errors, no charge | 1 request and ≤ 3 rows per stored report: ≤ 100 a day |
| R2 storage | 10 GB-month, then $0.015/GB-month | **billed** | 40 MiB a day × 180 days ≈ 7 GiB, with the lifecycle rule (deploy step 1) |
| R2 class A (PutObject, ListObjects) | 1 million a month, then $4.50/million | **billed** | ≤ 100 puts a day + `fetch.py` lists ≈ 4 000 a month |
| R2 class B (HeadObject, GetObject) | 10 million a month | **billed** | one head per valid upload (bounded by the Worker's 100 000 requests a day: ≤ 3 million a month) and the downloads |

Workers and Durable Objects on the Free plan stop at their limits instead of billing; R2 bills, so the budget keeps
R2 inside its free tier even under a sustained attack. Reports over the budget get 429 and the player tries again the
next UTC day (it gives up after three days). The account stays on the Workers Free plan: on Workers Paid the first
two rows would bill too.

The player sends only with reports on, on Wi-Fi, with a trusted clock, no update running and at least 15 % battery
(or charging); certificates are checked and redirects never followed. It retries after 5 min, 15 min, 1 h, 3 h,
then every 6 h; after a 429 at the next UTC day; it drops a report the Worker refuses or that is 3 days old.

## Deploy (owner, once)

Needs Node 20 or newer (`npx` fetches wrangler) and the Cloudflare account that holds `schulzcode.dev`.

1. **R2:** Cloudflare dashboard → R2 (enabling R2 may ask for a payment method even for the free tier) → create
   the bucket **`y2linux-reports`** (location automatic, Standard storage, no public access). Then the 180-day
   expiry (step 2 logs in first):
   ```sh
   npx wrangler r2 bucket lifecycle add y2linux-reports delete-after-180-days reports/ --expire-days 180
   npx wrangler r2 bucket lifecycle list y2linux-reports
   ```
2. **Log in** from `tools/reports/worker`:
   ```sh
   cd tools/reports/worker
   node --test                      # 11 tests, must pass
   npx wrangler login
   ```
3. Every command below runs **in `tools/reports/worker`** (wrangler reads `wrangler.toml` from there).
4. **Secrets:**
   ```sh
   python3 ../new_upload_key.py                                   # writes ~/.local/share/y2linux/report-upload.json
   python3 ../new_upload_key.py --print | npx wrangler secret put UPLOAD_KEY
   python3 -c "import secrets; print(secrets.token_urlsafe(32))" | npx wrangler secret put RATE_SALT
   ```
5. **Deploy:** `npx wrangler deploy`. It creates the `Quota` Durable Object (migration `v1`) and the
   `reports.schulzcode.dev` custom domain with its certificate (a minute or two).
6. **Rate limit in front of the Worker** (stops a flood before it uses Worker requests): dashboard →
   `schulzcode.dev` → Security → WAF → Rate limiting rules → create: *Hostname equals `reports.schulzcode.dev`*,
   counted per IP, **10 requests per 10 seconds**, action **Block** for 10 seconds. The Free plan allows one such
   rule. A player sends a report at most every few minutes.
7. **Billing alert:** dashboard → Notifications → Add → *Usage Based Billing* → product R2 storage, notify above
   **8 GB**. Nothing should ever reach it; if it does, something is wrong.
8. **Check it answers** (no key, so it must refuse):
   ```sh
   curl -si -X POST https://reports.schulzcode.dev/v1/report | head -1      # HTTP/2 403
   curl -si https://reports.schulzcode.dev/ | head -1                      # HTTP/2 404
   ```
9. **Read access for fetch.py:** R2 → Manage R2 API Tokens → create a token with **Object Read only**, applied to
   the bucket `y2linux-reports`. Save what it shows (the access key and secret are shown once) as
   `~/.config/y2linux/r2-reports.json`, mode 600:
   ```json
   {"account_id": "<account ID>", "access_key_id": "<access key ID>", "secret_access_key": "<secret>",
    "bucket": "y2linux-reports"}
   ```
   Then `python3 tools/reports/fetch.py --list` lists the bucket.

From then on every release build includes the upload key (`tools/production/build.py` prints
`reports: upload key included`), and its players show the error report switch, Send Report Now and the one-time
question. A build without the key file saves reports only. Keep a backup of
`~/.local/share/y2linux/report-upload.json`: a new key needs the Worker secret changed and a release, and players
on older releases lose uploads until they update.

## Reading reports

```sh
python3 tools/reports/fetch.py                  # new reports into out/reports/<day>/<install>/<report>.tar.gz
python3 tools/reports/fetch.py --list           # what is in the bucket, new ones marked
tar -xzf out/reports/2026-10-20/<install>/<report>.tar.gz -C /tmp/r && less /tmp/r/report.json
```

On the home server, the same script on a timer pulls reports into a folder there (copy `fetch.py` and the token
file; for example hourly with `--into /srv/y2linux-reports`). It only reads; R2's lifecycle rule deletes reports
after 180 days. A user who asks for their reports to be deleted gives their install ID: delete
`reports/*/<install ID>/` in the dashboard (and locally).
