# demo

A single Dioxus application that exercises every workflow the `livekit-browser` crates support:
capture, publication, subscription, device switching, data streams, RPC, chat, transcription, and
local clip recording. It is the harness the Playwright compatibility gate drives, and it doubles as
a worked example of the crate API.

The token endpoint is unauthenticated, so this is a development harness rather than a production
application. Point it at a throwaway LiveKit project.

It has two supported server modes:

- **Native Dioxus fullstack with Axum**, against a local `livekit-server --dev` container or a
  LiveKit Cloud project.
- **Cloudflare Workers**, as a static SPA plus one `/api/token` Worker route, against LiveKit Cloud.

Nothing proxies LiveKit in either mode when Cloud is used: the browser connects straight to the
Cloud signaling URL. The local Caddy `/rtc*` reverse proxy exists only to give the development
LiveKit container a secure same-origin URL.

## Structure

| Path | Role |
| --- | --- |
| `src/client.rs` | The browser application: every guided workflow, mounted against `livekit-browser`. |
| `src/token.rs` | LiveKit access-token minting. Target-neutral, shared by both server modes. |
| `src/server.rs` | Native Dioxus fullstack server (Axum): `/api/token`, `/healthz`, `index.html` fallback. |
| `src/worker.rs` | Cloudflare Worker entry (`lib.rs`): the same two routes, nothing else. |
| `src/lib.rs` | Feature gating for the four builds. |
| `src/main.rs` | Binary shim that launches the client or the native server. |
| `public/` | Static files `dx` copies verbatim into the served root, per `[web.resource]` in `Dioxus.toml`. |

`livekit-api` is a dev-dependency only. Its `AccessToken::with_api_key` reads
`std::time::SystemTime::now()`, which panics on `wasm32-unknown-unknown`, so a Worker built against
it would abort on its first request. `src/token.rs` mints the same HS256 JWT with `hmac`/`sha2` and
takes its clock from `web-time`; the tests pin its claims and signature against `livekit-api` on
native targets.

## Configuration

The application reads three values. Only two of them are secrets.

| Name | Secret | Read by | Notes |
| --- | --- | --- | --- |
| `LIVEKIT_URL` | no | server, Worker | Signaling URL, returned to the browser in the token response. Required on Workers: unlike the Docker stack there is no LiveKit server on the deployment's own origin to fall back to. Committing it to `[vars]` is fine; see [below](#setting-livekit_url-without-committing-it) if you would rather not. |
| `LIVEKIT_API_KEY` | **yes** | server, Worker | Token signing. Defaults to `devkey`. |
| `LIVEKIT_API_SECRET` | **yes** | server, Worker | Token signing. Defaults to `secret`. Never reaches the browser. |

The `devkey`/`secret` defaults are LiveKit's public development pair, which only a local
`livekit-server --dev` accepts. A Cloud project needs its own.

`compose.yaml` also sets three `LIVEKIT_NON_*_IDENTITIES` variables. Those exist only so the
Playwright suite can drive permission-denial paths against the Docker stack; the Worker build does
not read them, so every participant on a Worker deployment receives the default grants.

## Run locally

Requires the root devenv shell, or a Rust toolchain with the `wasm32-unknown-unknown` target plus
the Dioxus CLI and Node.js.

```bash
cd demo
npm install
npm run assets        # stages the pinned LiveKit UMD bundle into public/
dx serve --fullstack
```

`npm run assets` copies `node_modules/livekit-client/dist/livekit-client.umd.js` into `public/`,
where `Dioxus.toml`'s `[web.resource]` entry expects it. Without it the page still mounts but
reports the browser SDK as missing.

`dx serve` prints its URL. Camera and microphone need a secure context, which `http://localhost`
satisfies. To run against a Cloud project rather than issuing tokens no LiveKit server will accept:

```bash
LIVEKIT_URL=wss://your-project.livekit.cloud \
LIVEKIT_API_KEY=your-api-key \
LIVEKIT_API_SECRET=your-api-secret \
dx serve --fullstack
```

For phone and LAN testing, which need a certificate, use the Docker stack: `./run-prototype.sh`
from the repository root. See [Demo Application](../docs/demo.md).

## Cloudflare Workers

### Install the secrets

`LIVEKIT_API_KEY` and `LIVEKIT_API_SECRET` are the only true secrets: they are what the token
endpoint signs with and must never reach the browser.

Authenticate first, either interactively or with a token in the environment:

```bash
cd demo
npx wrangler login                       # or: export CLOUDFLARE_API_TOKEN=...
```

Then set each secret. Wrangler prompts for the value and never echoes it:

```bash
npx wrangler secret put LIVEKIT_API_KEY
npx wrangler secret put LIVEKIT_API_SECRET
```

If the Worker does not exist yet, Wrangler offers to create it before storing the secret, so this
can be run before the first deploy. To script it instead of typing at a prompt, pipe the value on
stdin:

```bash
printf '%s' "$LIVEKIT_API_KEY" | npx wrangler secret put LIVEKIT_API_KEY
```

Verify and manage them with:

```bash
npx wrangler secret list
npx wrangler secret delete LIVEKIT_API_KEY
```

Secrets are stored per Worker and are not readable back — only overwritten. They are separate from
local development values, which live in `.dev.vars`.

### Setting `LIVEKIT_URL` without committing it

`LIVEKIT_URL` is not sensitive, so a `[vars]` block in [`wrangler.toml`](wrangler.toml) is a
perfectly good home for it. If you would rather keep the project URL out of the repository, there
are three ways, and they are not equally robust:

```bash
# 1. As a secret. Recommended.
npx wrangler secret put LIVEKIT_URL

# 2. Per deploy, overriding the config.
npm run deploy -- --var LIVEKIT_URL:wss://your-project.livekit.cloud

# 3. Set once in the Cloudflare dashboard, then deploy with --keep-vars.
npm run deploy -- --keep-vars
```

Prefer the first. Secrets and vars land in the same `env` object at runtime, and the Worker reads
`env.var()` with a fallback to `env.secret()`, so nothing in the code changes. More importantly,
Wrangler's own help states it plainly:

> When not used (or set to false), Wrangler will delete all vars before setting those found in the
> Wrangler configuration. […] Note that secrets are never deleted by deployments.

So a value stored as a secret survives every deploy unconditionally, while a var set out of band is
erased by the next deploy that forgets `--keep-vars`. Option 2 is deterministic but has to be
repeated on every deploy, and the Dagger `worker deploy` target cannot pass `--var` today — so if
you deploy through Dagger, option 1 is the only one that works.

Whichever you choose, leaving `[vars]` unset in `wrangler.toml` is what makes options 1 and 3 safe:
an uncommented `[vars] LIVEKIT_URL` would overwrite them on the next deploy.

### Local development values

`wrangler dev` has no secret store, so `.dev.vars` carries all three values locally. Copy the
committed template and fill it in; `.dev.vars` and `.env` are both gitignored:

```bash
cp .env.dist .dev.vars
npm run dev
```

### Deploy

```bash
npm run deploy
```

`npm run build`, which `npm run dev` and `npm run deploy` both run first, bundles the client into
`dist/public` and compiles the `worker` feature into `dist/worker`. Wrangler is handed that finished
bundle rather than building one itself, so the Dagger pipeline and a plain `npx wrangler deploy`
agree on layout. Cloudflare serves the client from its edge asset store; only `/api/token` and
`/healthz` reach the Worker, per `run_worker_first` in `wrangler.toml`.

## Run with Dagger

Dagger builds and runs everything in containers, with no local Rust, `dx`, Node, or Wrangler:

```bash
cd demo

dagger call build                 # full-stack bundle
dagger call service up            # dx serve
dagger call worker build          # Cloudflare Worker bundle
dagger call worker dev up         # wrangler dev
dagger call worker deploy --account-id=<id> --api-token=env://CLOUDFLARE_API_TOKEN
```

`dagger check` runs the two build targets. The `dev` target mounts whatever `--env-file` points at
as `.dev.vars`, so the same file serves both paths:

```bash
dagger call worker dev --env-file=.dev.vars up
```

Cloudflare credentials are explicit arguments to `deploy` rather than entries in `.env.dist`.

CI deploys automatically ([`demo.yaml`](../.github/workflows/demo.yaml)): pushes to `main` roll out
to production, and pull requests upload a preview version, with its URLs posted as a PR comment. Both
jobs need `CLOUDFLARE_ACCOUNT_ID` and `CLOUDFLARE_API_TOKEN` repository secrets; preview only runs
for same-repo PRs, since fork and Dependabot PRs cannot read the secrets.

## Build-only checks

```bash
cd demo

# Token minting, shared by both server modes. Pinned against livekit-api.
cargo test --no-default-features --features token

# Browser client.
cargo check --features raw-js-contract --target wasm32-unknown-unknown

# Native fullstack server.
cargo check --no-default-features --features server

# Cloudflare Worker.
cargo check --no-default-features --features worker --target wasm32-unknown-unknown
```

This crate is its own Cargo workspace, so these only resolve from this directory.

## Notes

- `dx` copies everything under `public/` verbatim into the served root and watches it for reload.
  Do not rely on `[application] asset_dir` for this; in Dioxus 0.7 that key only affects
  `dx components add`.
- The pinned browser SDK is installed from npm and committed via `package-lock.json`, never fetched
  from a CDN at runtime. See [Browser Compatibility](../docs/browser-compatibility.md).
- Worker SSR is intentionally not part of this demo: the app is a static SPA plus one API route, so
  there is nothing for a server renderer to do that the asset bundle does not already cover.
