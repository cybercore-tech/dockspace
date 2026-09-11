# dockspace

A self-hosted dashboard for docker-compose stacks — inspired by
[sencho](https://github.com/saelix) and similar tools (Portainer, Dockge),
built from scratch in Rust with the shared
[cybercore](https://github.com/darkstardevx/cybercore) CYBERGRID theme
system and design tokens, matching the same `core`-branch pattern
[cyberdeck](https://github.com/darkstardevx/cyberdeck) already uses.

<!-- screenshot goes here once one's taken -->

## What it does

Scans a root directory (`~/Containers` by default — see
[the layout this was built for](https://github.com/darkstardevx)) for
every subdirectory containing a `compose.yaml`/`docker-compose.yml`, and
for each one shows:

- live status (`docker compose ps`, refreshed every 4s via htmx polling) —
  Running / Partial / Stopped / **Unknown** (distinct on purpose: means
  `docker compose ps` itself failed — daemon unreachable, permission
  denied — not "definitely stopped")
- every service name and published port, parsed straight from the
  compose file's own YAML (both the short `"8081:8081"` and long
  `{target, published}` port syntaxes)
- one-click Start / Stop / Restart
- a log viewer (`docker compose logs --tail 200`, manual refresh)
- a read-only compose file viewer

Every real Docker interaction shells out to the actual `docker` CLI —
same approach as [cyberfleet](https://github.com/darkstardevx/cyberfleet)'s
git integration: exact behavior parity with running the command yourself,
instead of reimplementing Docker Engine API semantics against a crate that
has to track every daemon version.

## Theming

Pulls in `cybercore` (pinned `v0.3.0`) for both halves of its design
system:

- `cybercore::tokens::CSS` — typography/spacing/radii/motion, served at
  `/vendor/tokens.css`
- `cybercore::schema` — the 12-theme CYBERGRID color palette, served at
  `/api/cybergrid/themes` (list) and `/api/cybergrid/css/:name` (a ready
  `:root{}` block). The dropdown in the top bar switches themes live and
  remembers your pick in `localStorage`.

None of dockspace's own CSS hardcodes a color — everything reads
`var(--bg)`, `var(--acid)`, etc., so every one of the 12 themes reskins the
whole dashboard, not just an accent.

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
- Logs are a snapshot (manual refresh), not a live tail. A real live tail
  (SSE, `docker compose logs -f` piped through) is a natural next step.
- Compose files are read-only in the viewer — no in-browser editing yet.
- No container exec/shell access yet.
- No "create a new stack from a template" flow yet.

## Docs

- [SECURITY.md](SECURITY.md) — no-auth caveat, loopback-only binding, what to do if you find a real issue
- [STATEMENT.md](STATEMENT.md) — how this project is built, AI's role in it

## License

MIT
