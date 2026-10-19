# Error report receiver

Players that opted in send scrubbed error reports (`tools/platform/y2_platform/report.py`) to a Cloudflare
Worker at `https://reports.schulzcode.dev/v1/report`, which keeps them in a private R2 bucket for 180 days. The
maintainer downloads them with `fetch.py`, on the PC or on the home server. Design and privacy rules:
[ERROR-REPORTS-PLAN](../../docs/planning/ERROR-REPORTS-PLAN.md); user guide: [ERROR-REPORTS](../../docs/ERROR-REPORTS.md).

| File | What it is |
| --- | --- |
| `worker/src/index.js` | The Worker: `POST /v1/report` only, no read route, no IP stored |
| `worker/wrangler.toml` | Route, R2 and KV bindings (fill in the KV namespace ID once) |
| `worker/test/worker.test.js` | `node --test` in `worker/` (Node 20+, no packages) |
| `new_upload_key.py` | Makes the build-wide upload key, once |
| `fetch.py` | Downloads new reports from R2 (S3 API, standard library only) |

## What the Worker checks

Upload key (equal-time compare), schema 1, UUID install and report IDs, a known trigger, the release string,
`Content-Length` ≤ 8 MiB, the body read with a hard 8 MiB limit, gzip magic, the body's SHA-256. Then the limits,
counted only for valid reports: 10 a day per install ID, 30 a day per address (a salted hash that changes daily and
expires; the address itself is never stored). A report ID it already stored answers 200 without a second copy. Stored
as `reports/<UTC day>/<install ID>/<report ID>.tar.gz` with schema, trigger, release, size and SHA-256 as metadata.
Request logs are off (`[observability] enabled = false`).

The player sends only with reports on, on Wi-Fi, with a trusted clock, no update running and at least 15 % battery
(or charging); certificates are checked and redirects never followed. It retries after 5 min, 15 min, 1 h, 3 h,
then every 6 h; after a 429 at the next UTC day; it drops a report the Worker refuses or that is 3 days old.

## Deploy (owner, once)

Needs Node 20 or newer (`npx` fetches wrangler) and the Cloudflare account that holds `schulzcode.dev`.

1. **R2:** Cloudflare dashboard → R2 (enabling R2 may ask for a payment method even for the free tier) → create
   the bucket **`y2linux-reports`** (location automatic). Bucket → Settings → Object lifecycle rules → add a rule:
   prefix `reports/`, delete objects **180 days** after upload.
2. **Log in** from `tools/reports/worker`:
   ```sh
   cd tools/reports/worker
   node --test                      # 11 tests, must pass
   npx wrangler login
   ```
3. **KV namespace** for the rate counters:
   ```sh
   npx wrangler kv namespace create RATES
   ```
   Put the printed `id` into `wrangler.toml` in place of `REPLACE_WITH_KV_NAMESPACE_ID` and commit it (an ID is
   not a secret).
4. **Secrets:**
   ```sh
   python3 ../new_upload_key.py                                   # writes ~/.local/share/y2linux/report-upload.json
   python3 ../new_upload_key.py --print | npx wrangler secret put UPLOAD_KEY
   python3 -c "import secrets; print(secrets.token_urlsafe(32))" | npx wrangler secret put RATE_SALT
   ```
5. **Deploy:** `npx wrangler deploy`. The `custom_domain` route creates the DNS record for
   `reports.schulzcode.dev` and its certificate (a minute or two).
6. **Check it answers** (no key, so it must refuse):
   ```sh
   curl -si -X POST https://reports.schulzcode.dev/v1/report | head -1      # HTTP/2 403
   curl -si https://reports.schulzcode.dev/ | head -1                      # HTTP/2 404
   ```
7. **Read access for fetch.py:** R2 → Manage R2 API Tokens → create a token with **Object Read only**, applied to
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
