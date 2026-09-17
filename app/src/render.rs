//! The manifesto and the signature list, as one HTML page. `page` is pure, so
//! a cache can be dropped in front of it later without restructuring.

use std::fmt::Write as _;
use std::sync::OnceLock;

use pulldown_cmark::{Options, Parser, html};
use sha2::{Digest, Sha256};

use crate::db::{self, Signatory};

/// `include_str!` registers the file as a build dependency, so editing the
/// markdown triggers a rebuild without a build script.
const MANIFESTO_MD: &str = include_str!("../../manifesto/MANIFESTO.md");
const STYLE: &str = include_str!("../assets/style.css");

const TITLE: &str = "Just Prompt, Motherfucker";
const SUBTITLE: &str = "Do you speak it?";
const DESCRIPTION: &str = "We are tired of harness engineering, agent orchestration, \
                           spec-driven development and everything else getting in the \
                           way of shipping software.";
const REPO_URL: &str = "https://github.com/flipbit03/just-prompt-motherfucker";

/// sha256 of the manifesto as this build embeds it. Stored per signature so
/// the exact text someone signed stays identifiable after an edit.
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
        // Without ENABLE_TABLES the values table renders as literal pipes.
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

/// 1204 -> "1,204".
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

    // TODO: og:image once the 1200x630 PNG exists. Omitted rather than
    // pointed at a 404 — Facebook caches a bad first scrape indefinitely.
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

/// Display number for the nth entry of the roll, counting from zero. The
/// founders hold the first positions, so the list carries on after them.
pub fn rank(index: usize) -> usize {
    db::FOUNDERS.len() + 1 + index
}

/// The front page: the manifesto, then everyone who has signed it.
pub fn page(base_url: &str, csrf: &str, roll: &[Signatory]) -> String {
    let count = (db::FOUNDERS.len() + roll.len()) as i64;
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

    if roll.is_empty() {
        s.push_str("<p class=\"nobody\">Nobody yet. Be the first.</p>\n");
    } else {
        s.push_str("<ol class=\"signers\">\n");
        for (i, sig) in roll.iter().enumerate() {
            let n = rank(i);
            let login = esc(&sig.login);
            let _ = writeln!(
                s,
                "<li id=\"s{n}\"><span class=\"n\">#{n}</span><a href=\"https://github.com/{login}\" \
                 rel=\"nofollow ugc\">{login}</a></li>",
            );
        }
        s.push_str("</ol>\n");
    }

    // POST, not a link: a GET would be followed by crawlers and prefetchers.
    // The hidden field is the twin of the jpmf_csrf cookie.
    let _ = write!(
        s,
        "<form method=\"post\" action=\"/sign\">\n\
         <input type=\"hidden\" name=\"csrf\" value=\"{}\">\n\
         <button>Sign it</button>\n</form>\n",
        esc(csrf)
    );
    let _ = write!(
        s,
        "<form method=\"post\" action=\"/unsign\" class=\"unsign\">\n\
         <input type=\"hidden\" name=\"csrf\" value=\"{}\">\n\
         <button type=\"submit\">Signed by mistake? Remove my signature</button>\n</form>\n",
        esc(csrf)
    );
    s.push_str("</section>\n</main>\n");

    footer(&mut s);
    s
}

/// Confirmation before removal: ordinals are permanent, so unsigning gives up
/// a number for good and should not happen on a single click.
pub fn confirm_unsign(base_url: &str, login: &str, ordinal: i64, token: &str) -> String {
    let mut s = String::with_capacity(8 * 1024);
    head(base_url, &mut s);
    s.push_str("<main>\n<h1>Remove your signature?</h1>\n");
    let _ = writeln!(
        s,
        "<p>GitHub says you are <strong>{}</strong>, currently <strong>#{ordinal}</strong>.</p>",
        esc(login)
    );
    s.push_str(
        "<p>The numbers are positions, oldest signature first. Remove yours and \
         everyone below moves up; sign again later and you join the end of the \
         list rather than returning to this spot.</p>\n",
    );
    let _ = write!(
        s,
        "<form method=\"post\" action=\"/unsign/confirm\">\n\
         <input type=\"hidden\" name=\"token\" value=\"{}\">\n\
         <button>Yes, remove #{ordinal}</button>\n</form>\n",
        esc(token)
    );
    s.push_str("<p><a href=\"/\">No, keep it</a></p>\n</main>\n");
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

    fn roll(logins: &[&str]) -> Vec<Signatory> {
        logins
            .iter()
            .enumerate()
            .map(|(i, l)| Signatory {
                github_id: i as i64 + 1,
                login: (*l).to_string(),
            })
            .collect()
    }

    #[test]
    fn manifesto_renders_its_table() {
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
    fn an_empty_roll_still_counts_the_founders() {
        let html = page("http://localhost:8100", "csrf0", &[]);
        assert!(html.contains("2 signatures and counting"));
        assert!(html.contains("Nobody yet"));
    }

    #[test]
    fn the_list_is_numbered_contiguously_after_the_founders() {
        let html = page("http://localhost:8100", "csrf0", &roll(&["a", "b", "c"]));
        assert!(html.contains("5 signatures and counting"));
        let list = signer_list(&html);
        for n in ["#3", "#4", "#5"] {
            assert!(list.contains(n), "missing {n}");
        }
        assert!(!list.contains("#6"), "numbering ran past the end");
        assert!(!list.contains("#2"), "numbering overlapped the founders");
        // The count always equals the last number in the list.
        assert!(list.contains("id=\"s5\""));
        assert!(list.contains("rel=\"nofollow ugc\""));
    }

    /// The stylesheet is full of things like `#444a53`, so assertions about
    /// numbering have to look inside the list and nowhere else.
    fn signer_list(html: &str) -> String {
        let start = html.find("<ol class=\"signers\">").expect("no list");
        let end = html[start..].find("</ol>").expect("unclosed list") + start;
        html[start..end].to_string()
    }

    #[test]
    fn positions_do_not_depend_on_any_stored_number() {
        // Whoever is oldest is #3, whatever their github_id happens to be.
        let mut r = roll(&["oldest", "newest"]);
        r[0].github_id = 9_999_999;
        r[1].github_id = 1;
        let list = signer_list(&page("http://localhost:8100", "csrf0", &r));
        let third = list.find("#3").unwrap();
        let fourth = list.find("#4").unwrap();
        assert!(third < fourth);
        assert!(list[third..fourth].contains("oldest"));
        assert!(list[fourth..].contains("newest"));
    }

    #[test]
    fn logins_are_escaped() {
        let html = page("http://localhost:8100", "csrf0", &roll(&["<script>"]));
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
