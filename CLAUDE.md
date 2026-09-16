# CLAUDE.md

Yes, we know. See the manifesto.

- The website is one Rust binary. Keep it that way.
- `manifesto/MANIFESTO.md` is the only copy of the text. Never duplicate it.
- No JavaScript.
- No new dependency without a reason you would say out loud.
- Tests, clippy, fmt. That is the gate.
- Signature ordinals are permanent. Never renumber them.
- `jpmf.db` is the only irreplaceable thing here. Nothing automated touches it.

## Writing

Comments explain why something non-obvious is done, once, in a line. They do
not editorialise, restate the manifesto, or boast about what the code avoids
using. "Zero JavaScript", "no framework", "this is on purpose" and similar do
not belong in source, commit messages, or docs — the code already shows it.
Say what a reader could not work out for themselves, then stop.
