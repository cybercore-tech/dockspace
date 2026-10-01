# Security Policy

Dockspace shells out to `docker compose` to start/stop/restart real
containers, and shows compose file contents and logs verbatim. **There is
no authentication in v1.** It binds to `127.0.0.1` only by design (see
`src/main.rs`) — anyone who can reach that port on your machine can control
every docker stack under the scanned root. That's an acceptable trade-off
for a single-user box reached locally or over an SSH tunnel; it is **not**
safe to rebind to a public address or expose through a reverse proxy
without adding real auth first.

Other things worth knowing:
- Stack names come straight from directory names on disk and are used to
  build a `docker compose -f <path>` argument — this assumes the scanned
  root (`~/Containers` by default) only ever contains directories you
  control. Don't point `DOCKSPACE_ROOT` at anything else.
- The compose/logs viewers HTML-escape their output before rendering, but
  they render exactly what's on disk / in `docker compose logs` — treat
  that view as no more trusted than running those commands yourself.

## Reporting a vulnerability

Email **cybercore.sh+security@gmail.com**. Include the affected file/commit, a minimal
repro, and what you'd expect instead. Please don't post exploit details in
a public issue until a fix has shipped.

## Supported versions

Only `main` is supported at this stage — no tagged releases yet.
