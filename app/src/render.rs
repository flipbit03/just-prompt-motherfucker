//! Turning the embedded manifesto and the signature list into one HTML page.
//!
//! `page` is a pure function of its inputs. That is deliberate: rendering from
//! the database on every request is fine at this scale, and keeping the render
//! pure means putting a cache in front of it later is a small diff rather than
//! a rewrite.

use std::fmt::Write as _;
use std::sync::OnceLock;

use pulldown_cmark::{Options, Parser, html};
use sha2::{Digest, Sha256};

use crate::db::Signature;

/// The manifesto is compiled into the binary; the binary is the website.
/// `include_str!` registers the file as a build dependency, so editing the
/// markdown rebuilds automatically with no build script.
const MANIFESTO_MD: &str = include_str!("../../manifesto/MANIFESTO.md");
const STYLE: &str = include_str!("../assets/style.css");

const TITLE: &str = "Just Prompt, Motherfucker";
const SUBTITLE: &str = "Do you speak it?";
const DESCRIPTION: &str = "We are tired of harness engineering, agent orchestration, \
                           spec-driven development and everything else getting in the \
                           way of shipping software.";
const REPO_URL: &str = "https://github.com/flipbit03/just-prompt-motherfucker";

/// sha256 of the manifesto exactly as this build embeds it. Stored with every
/// signature so "what did #47 actually sign" stays answerable after an edit.
/// Never shown in the UI.
pub fn manifesto_sha() -> &'static str {
    static SHA: OnceLock<String> = OnceLock::new();
    SHA.get_or_init(|| {
        Sha256::digest(MANIFESTO_MD.as_bytes())
            .iter()
            .fold(String::new(), |mut acc, b| {
                let _ = write!(acc, "{b:02x}");
                acc
            })
    })
}

/// The manifesto rendered once, on first use, and reused for every request.
fn manifesto_html() -> &'static str {
    static HTML: OnceLock<String> = OnceLock::new();
    HTML.get_or_init(|| {
        // Without ENABLE_TABLES the "Our values" table renders as literal pipes.
        let mut opts = Options::empty();
        opts.insert(Options::ENABLE_TABLES);
        let mut out = String::new();
        html::push_html(&mut out, Parser::new_ext(MANIFESTO_MD, opts));
        out
    })
}

fn esc(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for c in raw.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// 1204 -> "1,204". The list is meant to get long enough for this to matter.
fn thousands(n: i64) -> String {
    let digits = n.abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

fn head(base_url: &str, s: &mut String) {
    let url = format!("{base_url}/");
    s.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n");
    s.push_str("<meta charset=\"utf-8\">\n");
    s.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    let _ = writeln!(s, "<title>{}</title>", esc(TITLE));
    let _ = writeln!(
        s,
        "<meta name=\"description\" content=\"{}\">",
        esc(DESCRIPTION)
    );
    let _ = writeln!(s, "<link rel=\"canonical\" href=\"{}\">", esc(&url));

    // Open Graph. Every crawler gets the full page body too — this markup only
    // exists to make the unfurl card look like something.
    //
    // TODO: og:image once the 1200x630 PNG exists. Deliberately omitted rather
    // than pointed at a URL that 404s, because Facebook caches a bad first
    // scrape more or less forever.
    s.push_str("<meta property=\"og:type\" content=\"website\">\n");
    let _ = writeln!(s, "<meta property=\"og:url\" content=\"{}\">", esc(&url));
    let _ = writeln!(s, "<meta property=\"og:title\" content=\"{}\">", esc(TITLE));
    let _ = writeln!(
        s,
        "<meta property=\"og:description\" content=\"{}\">",
        esc(SUBTITLE)
    );
    let _ = writeln!(
        s,
        "<meta property=\"og:site_name\" content=\"{}\">",
        esc(TITLE)
    );
    s.push_str("<meta name=\"twitter:card\" content=\"summary_large_image\">\n");

    let _ = write!(s, "<style>\n{STYLE}</style>\n");
    s.push_str("</head>\n<body>\n");
}

fn footer(s: &mut String) {
    s.push_str("<footer>\n<p>\n");
    let _ = writeln!(
        s,
        "The whole website is one Rust binary. <a href=\"{REPO_URL}\">Read it</a>.<br>"
    );
    s.push_str(
        "The manifesto is CC&nbsp;BY&nbsp;4.0. The code is MIT.\n</p>\n</footer>\n</body>\n</html>\n",
    );
}

/// The front page: the manifesto, then everyone who has signed it.
pub fn page(base_url: &str, count: i64, signers: &[Signature]) -> String {
    let mut s = String::with_capacity(32 * 1024);
    head(base_url, &mut s);

    s.push_str("<main>\n");
    s.push_str(manifesto_html());

    s.push_str("<section class=\"signatures\">\n<h2>Also signed:</h2>\n");
    let word = if count == 1 {
        "signature"
    } else {
        "signatures"
    };
    let _ = writeln!(
        s,
        "<p class=\"count\">{} {word} and counting</p>",
        thousands(count)
    );

    if signers.is_empty() {
        s.push_str("<p class=\"nobody\">Nobody yet. Be the first.</p>\n");
    } else {
        s.push_str("<ol class=\"signers\">\n");
        for sig in signers {
            let login = esc(&sig.login);
            let _ = writeln!(
                s,
                "<li><span class=\"n\">#{}</span><a href=\"https://github.com/{login}\" \
                 rel=\"nofollow ugc\">{login}</a></li>",
                sig.ordinal
            );
        }
        s.push_str("</ol>\n");
    }

    // A form POST rather than a link: a GET endpoint would be followed by every
    // crawler and link prefetcher on the internet, each one bounced to GitHub
    // for nothing. Still zero JavaScript.
    s.push_str("<form method=\"post\" action=\"/sign\">\n<button>Sign it</button>\n</form>\n");
    s.push_str("</section>\n</main>\n");

    footer(&mut s);
    s
}

/// A small standalone page for the paths that are not the manifesto.
pub fn notice(base_url: &str, heading: &str, body: &str) -> String {
    let mut s = String::with_capacity(8 * 1024);
    head(base_url, &mut s);
    let _ = write!(
        s,
        "<main>\n<h1>{}</h1>\n<p>{}</p>\n<p><a href=\"/\">Back to the manifesto</a></p>\n</main>\n",
        esc(heading),
        esc(body)
    );
    footer(&mut s);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifesto_renders_its_table() {
        // ENABLE_TABLES is easy to drop and the failure is silent-ish: the
        // values table degrades into a paragraph of pipe characters.
        let html = manifesto_html();
        assert!(html.contains("<table>"), "values table did not render");
        assert!(!html.contains("| Autonomous agents |"));
    }

    #[test]
    fn manifesto_sha_is_stable_and_hex() {
        let sha = manifesto_sha();
        assert_eq!(sha.len(), 64);
        assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(sha, manifesto_sha());
    }

    #[test]
    fn empty_list_still_shows_the_count() {
        let html = page("http://localhost:8100", 2, &[]);
        assert!(html.contains("2 signatures and counting"));
        assert!(html.contains("Nobody yet"));
    }

    #[test]
    fn signers_render_with_permanent_ordinals() {
        let signers = vec![
            Signature {
                ordinal: 3,
                login: "someone".into(),
            },
            Signature {
                ordinal: 7,
                login: "later".into(),
            },
        ];
        let html = page("http://localhost:8100", 4, &signers);
        assert!(html.contains("#3"));
        // A gap, because #4..#6 unsigned. The numbers must not be renumbered.
        assert!(html.contains("#7"));
        assert!(html.contains("rel=\"nofollow ugc\""));
    }

    #[test]
    fn logins_are_escaped() {
        let signers = vec![Signature {
            ordinal: 3,
            login: "<script>".into(),
        }];
        let html = page("http://localhost:8100", 3, &signers);
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn thousands_separates() {
        assert_eq!(thousands(2), "2");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_204), "1,204");
        assert_eq!(thousands(1_000_000), "1,000,000");
    }
}
