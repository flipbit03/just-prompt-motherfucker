# Just Prompt, Motherfucker

A manifesto: **https://www.just-prompt-motherfucker.com**

The text is [`manifesto/MANIFESTO.md`](manifesto/MANIFESTO.md). The site compiles
that file in, so there is only ever one copy of it.

Sign it with a GitHub account and your handle joins the list at the bottom of
the page.

## Running it

```sh
cd app
cargo run          # http://localhost:8100
```

Creates `jpmf.db` in the working directory. Signing additionally needs a GitHub
OAuth App with `http://localhost:8100/auth/callback` registered; put its
credentials in `app/.env` and load them first:

```sh
set -a; . ./.env; set +a    # JPMF_CLIENT_ID, JPMF_CLIENT_SECRET
cargo run
```

Without them the site serves the manifesto and signing answers 503.

`jpmf backup DEST` writes a snapshot, safe to run while the site is serving.

See [CLAUDE.md](CLAUDE.md) for the architecture.

## Translations

Pull requests welcome: add `manifesto/MANIFESTO.<language-code>.md`. The English
text is canonical and the one signatures are counted against.

## Licence

The manifesto is [CC BY 4.0](manifesto/LICENSE) — copy it, translate it, put it
on your own site, credit Carlos Eduardo Coelho and Leandro Proença. The code is
[MIT](LICENSE).
