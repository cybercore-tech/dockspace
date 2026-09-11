# A note on how this gets built

I use AI assistance to help build and maintain these projects, and it shows
up in the commit history on the ones that are public. I'm not going to
pretend otherwise, and I'm not going to bury it either.

**What's mine:** the idea for dockspace came from actually needing it — I'd
just spent a session organizing every docker-compose stack I run into one
place and realized I wanted a dashboard for it, modeled on tools I'd
already used (sencho) but built to match how my own projects actually work:
Rust, the same cybercore design tokens every other Cybercore surface uses,
no database, no accounts, just read the filesystem and shell out to the
real `docker compose` binary. Deciding what it should do, what it
shouldn't (no auth in v1, loopback-only, and I said so plainly in
SECURITY.md instead of pretending that's fine for anything but a
single-user box) — that's mine. I spent 30+ years as an auto tech before
this; the trade teaches you the tool in your hand doesn't make the
diagnosis, you do.

**What the AI does:** grunt work — the axum routing boilerplate, the YAML
parsing, catching a real bug (an axum version-syntax mismatch that quietly
404'd every action route and the theme endpoint, which is why the whole
dashboard rendered unstyled the first time it ran) by actually running the
thing and looking at what came back, not just reading the code and assuming
it was right.

**Why say it:** I'd rather you know going in than find out later. Working
smart isn't a shortcut — it's the point. Judge the code; that's always been
the right way to do it anyway.

— darkstardevx
