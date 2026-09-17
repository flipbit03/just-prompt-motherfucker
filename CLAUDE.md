# CLAUDE.md

Yes, we know. See the manifesto.

- The website is one Rust binary. Keep it that way.
- `manifesto/MANIFESTO.md` is the only copy of the text. Never duplicate it.
- No JavaScript.
- No new dependency without a reason you would say out loud.
- Tests, clippy, fmt. That is the gate.
- The numbers beside names are positions, derived from `signed_at` at render
  time. Nothing stores a display number. Removing a signature closes the gap.
- `jpmf.db` is the only irreplaceable thing here. Nothing automated touches it.
- Releases are named `YYYY.MM.DD`, and `YYYY.MM.DD.N` for the second and any
  later release on the same day. No `v`, no release notes. Cutting one deploys
  to production.

## Known, accepted

Cancelling on GitHub's consent screen redirects to the **production** callback
rather than localhost. GitHub honours the `redirect_uri` we send when the user
approves and ignores it when they deny, falling back to the app's first
registered callback URL — which is the production one. Production is
unaffected; only local Cancel is odd. A second OAuth App would fix it and is
not worth a second set of credentials.

## Writing

Comments explain why something non-obvious is done, once, in a line. They do
not editorialise, restate the manifesto, or boast about what the code avoids
using. "Zero JavaScript", "no framework", "this is on purpose" and similar do
not belong in source, commit messages, or docs — the code already shows it.
Say what a reader could not work out for themselves, then stop.
