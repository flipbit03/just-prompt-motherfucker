# infra/rustible

Rustible workspace for the machine this site runs on.

```sh
cd infra/rustible
rustible inventory check                 # validate hosts.kdl
rustible playbook run playbooks/jpmf.rs --check   # dry run, changes nothing
rustible playbook run playbooks/jpmf.rs           # apply
```

It owns only what a deploy cannot renew on its own:

- the `jpmf` user and its home
- the deploy workflow's public key in `~jpmf/.ssh/authorized_keys`, plus
  flipbit03's GitHub keys so a human can log in as `jpmf` directly
- `loginctl enable-linger jpmf`, without which the user service stops at
  logout and does not return after a reboot
- `/etc/caddy/conf.d/jpmf.caddy`

Everything else is written on every release by `.github/workflows/deploy.yml`:
the binary, `~jpmf/.config/systemd/user/jpmf.service`, and `.env`. The
database is written by the app and by nothing else.

**Precondition:** `/etc/caddy/Caddyfile` must import `/etc/caddy/conf.d/*.caddy`
and that directory must exist. The Caddyfile belongs to the machine's own
provisioning, so this playbook writes one file into `conf.d/` and never touches
the Caddyfile itself.
