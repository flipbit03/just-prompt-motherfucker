# Just Prompt, Motherfucker

A manifesto. Read it at **https://www.just-prompt-motherfucker.com** — or read
[`manifesto/MANIFESTO.md`](manifesto/MANIFESTO.md), which is the same text: the
site does not have a copy, it compiles that file in.

Sign it with your GitHub account and your handle joins the list at the bottom
of the page, with the number you signed at. The numbers are permanent.

## How it is built

The whole website is one Rust binary. No framework, no bundler, no container,
no JavaScript. The manifesto is `include_str!`'d at compile time and the
signatures live in one SQLite table next to the binary.

This is on purpose. A document arguing that most of the scaffolding is
optional should not arrive wrapped in scaffolding.

```
manifesto/   the text, and translations if anyone sends one
app/         the entire website: one crate, one binary
infra/       the machine it runs on, and the systemd unit
```

## Running it locally

```sh
cd app
cargo run          # http://localhost:8100
```

It creates `jpmf.db` in the working directory on first run. Configuration is
three environment variables, all with working defaults for local use:

| variable | default | what it is |
| --- | --- | --- |
| `JPMF_BIND` | `127.0.0.1:8100` | address to listen on |
| `JPMF_DB` | `jpmf.db` | path to the SQLite file |
| `JPMF_BASE_URL` | `http://localhost:8100` | canonical URL, used in links and OAuth |

To take a snapshot of the database — safe to run while the site is serving:

```sh
jpmf backup ~/jpmf-$(date +%F).db
```

## Translations

Pull requests welcome. Add `manifesto/MANIFESTO.<language-code>.md`. The English
text is canonical; it is the one signatures are counted against.

## Licence

The manifesto is [CC BY 4.0](manifesto/LICENSE) — copy it, translate it, put it
on your own site, just credit Leandro and Cadu. The code is [MIT](LICENSE).
