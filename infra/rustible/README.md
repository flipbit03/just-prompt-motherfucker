# infra/rustible

Rustible workspace for the one machine this runs on.

Not yet initialised — `rustible init` goes here. It owns only what a deploy
cannot renew on its own:

- the `jpmf` user and its `authorized_keys` entry for the deploy workflow
- `loginctl enable-linger jpmf`, so the user service survives logout and starts
  at boot
- `/etc/caddy/conf.d/jpmf.caddy` — apex 301s to www, www reverse-proxies to
  `127.0.0.1:8100`, with `encode zstd gzip`

Everything else — the binary, the systemd unit, the `.env` — is written by
`.github/workflows/deploy.yml` on every release, and `jpmf.db` is written by
nothing but the app itself.
