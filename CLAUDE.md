# CLAUDE.md

Yes, we know. See the manifesto.

- The website is one Rust binary. Keep it that way.
- `manifesto/MANIFESTO.md` is the only copy of the text. Never duplicate it.
- No JavaScript. Not "a little". None.
- No new dependency without a reason you would say out loud.
- Tests, clippy, fmt. That is the gate.
- Signature ordinals are permanent. Never renumber them.
- `jpmf.db` is the only irreplaceable thing here. Nothing automated touches it.
