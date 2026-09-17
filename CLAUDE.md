# CLAUDE.md

A manifesto site. One Rust binary serves a page compiled from
`manifesto/MANIFESTO.md` and records GitHub-authenticated signatures in SQLite.

## Layout

```
manifesto/MANIFESTO.md   the text; the binary embeds it with include_str!
app/                     the whole website — one crate, ~2,000 lines
  src/main.rs            config, routes, handlers
  src/render.rs          markdown -> HTML, the page, the head metadata
  src/db.rs              schema and queries
  src/github.rs          OAuth against GitHub
  src/cookies.rs         the two CSRF defences
  assets/style.css       inlined into <head> at build time
infra/rustible/          provisions the host (Rustible, not Ansible)
infra/jpmf.service       systemd unit, shipped by the deploy
.github/workflows/       ci.yml, deploy.yml
```

## Stack

axum + tokio, rusqlite (`bundled`, so SQLite compiles in), pulldown-cmark,
reqwest (rustls), clap, sha2, getrandom. Rust edition 2024, pinned by
`rust-toolchain.toml`. Ships as a static musl binary, ~8 MB.

## Working on it

```sh
cd app
cargo run                                  # http://localhost:8100
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

CI runs exactly those three. Clippy is `-D warnings`; treat a warning as a
failure locally too.

Changing `manifesto/MANIFESTO.md` requires a rebuild — `include_str!` makes it a
build dependency, so `cargo run` picks it up automatically.

## How a request works

```
GET  /                  manifesto + signature list, rendered from SQLite per request
POST /sign              CSRF check -> set state cookie -> 303 to GitHub
POST /unsign            same, with the intent encoded in the state
GET  /auth/callback     verify state -> exchange code -> read account -> act
POST /unsign/confirm    consume a pending token -> delete the row
GET  /healthz           the deploy gates on this
GET  /robots.txt
```

Anything else answers a rendered 404.

### Signing is not a login

There is no session. The OAuth token is used once to read the account id and
dropped; nothing about a visitor is remembered between requests. `scope` is
empty, so GitHub grants only public profile data.

Removing a signature runs the same flow again — re-authenticating *is* how
someone proves the row is theirs. The `pending_unsign` row carries the proven
identity from the callback to the confirm button for five minutes, single use,
pinned to that signature's `signed_at` so a stale confirmation cannot delete a
signature made after it was issued.

Two separate CSRF defences, covering different holes:

- **`jpmf_state`** covers the callback. Compared whole against the query
  parameter; a forged query string cannot match a cookie the attacker could
  never set.
- **`jpmf_csrf`** plus a hidden form field covers the way *in*. Without it a
  cross-site POST could start a flow on a visitor's behalf — GitHub skips the
  consent screen for anyone who already authorised the app.

## Data

One table, `signatures`, defined in `app/src/db.rs`. Keyed by `github_id`,
because handles get renamed and ids do not.

WAL, `synchronous=NORMAL`. One connection behind a `Mutex`; never hold the lock
across an `.await`. Schema is `CREATE TABLE IF NOT EXISTS` at startup — no
migration framework.

## Invariants

- **`manifesto/MANIFESTO.md` is the only copy of the text, and its opening
  lines are load-bearing.** `render::front_matter` parses the file at startup:
  the `# ` line becomes `<title>` and `og:title`, the emphasised line under it
  becomes `og:description`, and the first paragraph of prose becomes the meta
  description search engines show. Change the *shape* of those lines — drop the
  emphasis, add a paragraph above the title, lead with a quote — and the page's
  metadata changes with it or empties out. The manifesto carries a comment
  saying so. These were Rust constants once; they drifted, and the share card
  advertised a subtitle the document no longer had.
- **The numbers beside names are positions**, computed at render time from
  `signed_at` order. Nothing stores a display number. Removing a signature
  closes the gap; the founders occupy the first positions so the list starts
  after them.
- **Founders are a const, not rows.** `db::FOUNDERS`. Nothing to insert and
  nothing to delete, so they cannot be revoked and both flows refuse early.
- **`jpmf.db` is irreplaceable.** Nothing automated touches it. Unlike the other
  services on that host, this folder is not disposable.
- **No JavaScript.**
- **No new dependency without a reason you would say out loud.**

## Deploying

Releases are named `YYYY.MM.DD`, or `YYYY.MM.DD.N` for a second release the same
day. No `v`, no release notes. Cutting one runs `deploy.yml`: musl build, scp to
`~jpmf/jpmf_deploy/incoming/`, rename over the running binary, write `.env` from
repository secrets, `systemctl --user restart`, then poll `/healthz`.

Secrets: `DEPLOY_SSH_KEY`, `JPMF_CLIENT_ID`, `JPMF_CLIENT_SECRET`.

[`infra/rustible/`](infra/rustible/) provisions what a deploy cannot renew — the `jpmf` user, the
deploy key, `enable-linger`, and `/etc/caddy/conf.d/jpmf.caddy`. Run it with
`rustible playbook run playbooks/jpmf.rs`; it is idempotent. Rustible is an
Ansible replacement whose playbooks are Rust: https://github.com/flipbit03/rustible The host's Caddyfile
imports `conf.d/*.caddy`, and this project owns exactly one file in there.

## Gotchas

Each of these cost time once.

- **scp cannot overwrite a running binary** (`ETXTBSY`). The deploy stages into
  `incoming/` and renames; a rename over a running executable is fine.
- **`systemctl --user` over non-interactive SSH** needs `XDG_RUNTIME_DIR`
  exported or it cannot find the bus.
- **Cookies must be `SameSite=Lax`, never `Strict`.** The OAuth callback is a
  cross-site top-level navigation from github.com, and `Strict` withholds
  cookies on exactly that.
- **Cancelling GitHub's consent screen redirects to the production callback**,
  not localhost. GitHub honours `redirect_uri` on approval and ignores it on
  denial, falling back to the app's first registered callback. Only local Cancel
  is affected. Accepted; a second OAuth App would fix it and is not worth a
  second set of credentials.

## Writing

Comments explain why something non-obvious is done, once, in a line. They do not
editorialise, restate the manifesto, or boast about what the code avoids using.
"Zero JavaScript", "no framework", "this is on purpose" and similar do not
belong in source, commit messages or docs — the code already shows it. Say what a reader could not work out for themselves,
then stop.
