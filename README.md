# dockspace

A self-hosted dashboard for docker-compose stacks — inspired by
[sencho](https://github.com/saelix) and similar tools (Portainer, Dockge),
built from scratch in Rust with the shared
[cybercore](https://github.com/cybercore-tech/cybercore) CYBERGRID theme
system and design tokens, matching the same `core`-branch pattern
[cyberdeck](https://github.com/cybercore-tech/cyberdeck) already uses.

<!-- screenshot goes here once one's taken -->

## What it does

Scans a root directory (`~/Containers` by default — see
[the layout this was built for](https://github.com/darkstardevx)) for
every subdirectory containing a `compose.yaml`/`docker-compose.yml`, and
for each one shows:

- live status (`docker compose ps`, refreshed every 4s via htmx polling) —
  Running / Partial / Stopped / **Unknown** (distinct on purpose: means
  `docker compose ps` itself failed — daemon unreachable, permission
  denied — not "definitely stopped") / **Working…** (an action is running
  in the background right now — see below)
- every service name and published port, parsed straight from the
  compose file's own YAML (both the short `"8081:8081"` and long
  `{target, published}` port syntaxes)
- Start / Stop / Restart that stay responsive even when the underlying
  `docker compose up` is slow (a cold image pull can take minutes) — the
  click returns immediately with a "Working…" state, the real command
  runs in the background, and the next poll shows the actual outcome. A
  failed action shows an error banner on the card instead of silently
  doing nothing.
- a **per-service breakdown** on each card (each container's own state,
  not just the rolled-up badge)
- a **filter box** on the dashboard — narrows the grid to stacks/services
  matching what you type, client-side, no round trip
- a log viewer: a manual-refresh snapshot (`docker compose logs --tail
  200`) plus a **live tail** button that opens a real Server-Sent-Events
  stream of `docker compose logs -f`
- a compose file **editor** — edit and Save writes straight back to that
  stack's `compose.yaml`, plus a **Validate** button ("Compose Doctor")
  that runs `docker compose config` against the edited content *before*
  you save, so a typo shows up as an inline error instead of a broken file
  on disk
- **+ New Stack** — creates a folder under the scan root with a starter
  `compose.yaml`, opens straight into the editor to fill it in
- a **Resources** page — every image/volume/network on the host, with a
  scoped prune button for each
- an **Activity** log — a recent-activity feed of every start/stop/
  restart/save/create/prune, ok or failed, with a hover tooltip on
  failures showing the real command output

These are dockspace's take on the parts of
[sencho](https://github.com/studio-saelix/sencho)'s feature set that make
sense for a single-machine, no-auth, personal dashboard — sencho itself
goes considerably further (fleet management across nodes, RBAC/SSO,
Trivy vulnerability scanning, Blueprints, S3 archives, a host console) for
teams managing more than one box.

Every real Docker interaction shells out to the actual `docker` CLI —
same approach as [cyberfleet](https://github.com/cybercore-tech/cyberfleet)'s
git integration: exact behavior parity with running the command yourself,
instead of reimplementing Docker Engine API semantics against a crate that
has to track every daemon version.

## Theming

Uses Cybercore's shared theme-document catalog and design tokens:

- `cybercore::tokens::CSS` — typography/spacing/radii/motion, served at
  `/vendor/tokens.css`
- `ThemeCatalog` — built-in and custom themes served at
  `/api/cybergrid/themes` and `/api/cybergrid/css/:id`; the top-bar picker
  shares its selection with other Cybercore apps.
- The **Theme Studio** link opens the standalone visual editor at
  `http://127.0.0.1:8761/`.
- Open pages refresh shared theme and appearance changes while visible;
  installing or removing themes refreshes the picker automatically.

Dockspace pins Cybercore's upstream theme-catalog commit because the API is
newer than the current crates.io release. Once the next Cybercore crate version
is published, this dependency can move back to crates.io.

None of Dockspace's own CSS hardcodes a color — everything reads
`var(--bg)`, `var(--acid)`, etc., so the selected theme reskins the whole
dashboard, not just an accent.

## Run it

```bash
cargo run --release
# → http://127.0.0.1:7070
```

Binds to **127.0.0.1 only**, on purpose — see [SECURITY.md](SECURITY.md).
Point it at a different root with `DOCKSPACE_ROOT=/path/to/stacks cargo run`.

## Requirements

- Rust (edition 2021)
- `docker` + `docker compose` on `$PATH`, and your user in the `docker`
  group (or otherwise able to reach `/var/run/docker.sock`) — dockspace
  doesn't request or use elevated privileges itself, it just shells out to
  whatever `docker` command your own shell could already run.

## Known limitations (v1)

- No authentication — see [SECURITY.md](SECURITY.md).
- The compose editor is a plain textarea — no syntax highlighting, just
  the Validate button's preflight check.
- No container exec/shell access yet (sencho's "host console").
- New stacks always start from the same generic Alpine placeholder — no
  template picker (e.g. "start from a Postgres stack") yet.
- Single machine only — no fleet/multi-node management.
- No image-update checking (comparing a running image's digest against
  what's on the registry) or auto-heal/auto-update policies.
- The activity log is in-memory only — it resets on restart.

## Docs

- [SECURITY.md](SECURITY.md) — no-auth caveat, loopback-only binding, what to do if you find a real issue
- [STATEMENT.md](STATEMENT.md) — how this project is built, AI's role in it

## License

MIT
