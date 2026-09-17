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

/// The share card: the page's hero at 1200x630, the one size every scraper
/// accepts. Georgia Bold 96pt over Georgia Italic 44pt, in the page's colours.
/// Redraw it when the title or subtitle changes.
pub const OG_IMAGE: &[u8] = include_bytes!("../assets/og.png");

/// Title, subtitle and description are read out of the manifesto rather than
/// restated here. Restating them is what let the card keep advertising an old
/// subtitle after the document had changed.
struct FrontMatter {
    title: String,
    subtitle: String,
    description: String,
}

/// Markdown paragraphs: blank-line separated, soft wraps joined.
fn paragraphs(md: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut para = String::new();
    for line in md.lines() {
        if line.trim().is_empty() {
            if !para.is_empty() {
                out.push(std::mem::take(&mut para));
            }
        } else {
            if !para.is_empty() {
                para.push(' ');
            }
            para.push_str(line.trim());
        }
    }
    if !para.is_empty() {
        out.push(para);
    }
    out
}

fn front_matter() -> &'static FrontMatter {
    static FM: OnceLock<FrontMatter> = OnceLock::new();
    FM.get_or_init(|| {
        let paras = paragraphs(MANIFESTO_MD);

        let title = paras
            .iter()
            .find_map(|p| p.strip_prefix("# "))
            .unwrap_or("Just Prompt, Motherfucker")
            .to_string();

        // The one emphasised line under the title.
        let subtitle = paras
            .iter()
            .find(|p| p.starts_with('*') && p.ends_with('*') && !p.starts_with("**"))
            .map(|p| p.trim_matches('*').trim().to_string())
            .unwrap_or_default();

        // The first paragraph of actual prose becomes the search snippet. The
        // HTML comment at the top of the manifesto is a paragraph too, and
        // starts with none of the markers below, so it is excluded by name.
        let description = paras
            .iter()
            .find(|p| {
                !p.starts_with('#')
                    && !p.starts_with('*')
                    && !p.starts_with('|')
                    && !p.starts_with("<!--")
            })
            .map(|p| p.replace(['*', '_'], ""))
            .unwrap_or_default();

        FrontMatter {
            title,
            subtitle,
            description,
        }
    })
}
pub const REPO: &str = "flipbit03/just-prompt-motherfucker";

/// The GitHub mark, from primer/octicons (MIT). Inlined so the footer costs no
/// extra request and no third party sees who visits.
const GITHUB_MARK: &str = r##"<svg viewBox="0 0 16 16" width="16" height="16" fill="currentColor" aria-hidden="true"><path d="M6.766 11.328c-2.063-.25-3.516-1.734-3.516-3.656 0-.781.281-1.625.75-2.188-.203-.515-.172-1.609.063-2.062.625-.078 1.468.25 1.968.703.594-.187 1.219-.281 1.985-.281.765 0 1.39.094 1.953.265.484-.437 1.344-.765 1.969-.687.218.422.25 1.515.046 2.047.5.593.766 1.39.766 2.203 0 1.922-1.453 3.375-3.547 3.64.531.344.89 1.094.89 1.954v1.625c0 .468.391.734.86.547C13.781 14.359 16 11.53 16 8.03 16 3.61 12.406 0 7.984 0 3.563 0 0 3.61 0 8.031a7.88 7.88 0 0 0 5.172 7.422c.422.156.828-.125.828-.547v-1.25c-.219.094-.5.156-.75.156-1.031 0-1.64-.562-2.078-1.609-.172-.422-.36-.672-.719-.719-.187-.015-.25-.093-.25-.187 0-.188.313-.328.625-.328.453 0 .844.281 1.25.86.313.452.64.655 1.031.655s.641-.14 1-.5c.266-.265.47-.5.657-.656"/></svg>"##;

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

fn to_html(md: &str) -> String {
    // Without ENABLE_TABLES the values table renders as literal pipes.
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    let mut out = String::new();
    html::push_html(&mut out, Parser::new_ext(md, opts));
    out
}

/// The manifesto's last line with a colon on it, so that it introduces the
/// names rather than merely preceding them.
fn introducing(md: &str) -> String {
    let lines: Vec<&str> = md.lines().collect();
    let Some(last) = lines.iter().rposition(|l| !l.trim().is_empty()) else {
        return md.to_string();
    };
    let mut out = String::with_capacity(md.len() + 1);
    for (i, line) in lines.iter().enumerate() {
        if i == last {
            out.push_str(line.trim_end());
            out.push(':');
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

/// Rendered twice and cached: as written, and with the closing line turned
/// into an introduction. Which one the page uses depends on whether anybody
/// has signed.
fn manifesto_html(introduces: bool) -> &'static str {
    static PLAIN: OnceLock<String> = OnceLock::new();
    static INTRO: OnceLock<String> = OnceLock::new();
    if introduces {
        INTRO.get_or_init(|| to_html(&introducing(MANIFESTO_MD)))
    } else {
        PLAIN.get_or_init(|| to_html(MANIFESTO_MD))
    }
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

/// 934 -> "934", 1_250 -> "1.2k", 109_500 -> "110k", 2_400_000 -> "2.4m".
fn compact(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => scaled(n, 1_000, 'k'),
        _ => scaled(n, 1_000_000, 'm'),
    }
}

fn scaled(n: u64, unit: u64, suffix: char) -> String {
    let whole = n / unit;
    if whole >= 10 {
        // Past ten there is no room for a decimal: round to the nearest unit.
        return format!("{}{suffix}", (n + unit / 2) / unit);
    }
    match (n % unit) * 10 / unit {
        0 => format!("{whole}{suffix}"),
        tenth => format!("{whole}.{tenth}{suffix}"),
    }
}

fn head(base_url: &str, stars: Option<u64>, s: &mut String) {
    let url = format!("{base_url}/");
    s.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n");
    s.push_str("<meta charset=\"utf-8\">\n");
    s.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    let fm = front_matter();
    let _ = writeln!(s, "<title>{}</title>", esc(&fm.title));
    let _ = writeln!(
        s,
        "<meta name=\"description\" content=\"{}\">",
        esc(&fm.description)
    );
    let _ = writeln!(s, "<link rel=\"canonical\" href=\"{}\">", esc(&url));

    s.push_str("<meta property=\"og:type\" content=\"website\">\n");
    let _ = writeln!(s, "<meta property=\"og:url\" content=\"{}\">", esc(&url));
    // Title and subtitle together: the whole message wherever the card's
    // image does not load. The tab keeps the bare title.
    let card = format!("{}. {}", esc(&fm.title), esc(&fm.subtitle));
    let _ = writeln!(s, "<meta property=\"og:title\" content=\"{card}\">");
    let _ = writeln!(
        s,
        "<meta property=\"og:description\" content=\"{}\">",
        esc(&fm.description)
    );
    let _ = writeln!(
        s,
        "<meta property=\"og:site_name\" content=\"{}\">",
        esc(&fm.title)
    );
    // Facebook and LinkedIn render the large card on the first scrape only
    // when the dimensions are declared.
    let _ = writeln!(
        s,
        "<meta property=\"og:image\" content=\"{base_url}/og.png\">\n\
         <meta property=\"og:image:width\" content=\"1200\">\n\
         <meta property=\"og:image:height\" content=\"630\">\n\
         <meta property=\"og:image:type\" content=\"image/png\">\n\
         <meta property=\"og:image:alt\" content=\"{card}\">"
    );
    // X falls back to the og: values today; spelling them out costs nothing
    // and does not depend on that staying true.
    let _ = writeln!(
        s,
        "<meta name=\"twitter:card\" content=\"summary_large_image\">\n\
         <meta name=\"twitter:title\" content=\"{card}\">\n\
         <meta name=\"twitter:description\" content=\"{}\">\n\
         <meta name=\"twitter:image\" content=\"{base_url}/og.png\">\n\
         <meta name=\"twitter:image:alt\" content=\"{card}\">",
        esc(&fm.description)
    );

    let _ = write!(s, "<style>\n{STYLE}</style>\n");
    s.push_str("</head>\n<body>\n");

    let _ = write!(
        s,
        "<a class=\"gh\" href=\"https://github.com/{REPO}\">{GITHUB_MARK}<span>"
    );
    // No number until a fetch succeeds: a missing answer and a repository
    // nobody has starred should not look the same.
    match stars {
        Some(n) => {
            let _ = writeln!(s, "{}</span></a>", compact(n));
        }
        None => s.push_str("GitHub</span></a>\n"),
    }
}

fn close(s: &mut String) {
    s.push_str("</body>\n</html>\n");
}

/// Display number for the nth entry of the roll, counting from zero. The
/// founders hold the first positions, so the list carries on after them.
pub fn rank(index: usize) -> usize {
    db::FOUNDERS.len() + 1 + index
}

/// The front page: the manifesto, then everyone who has signed it.
pub fn page(base_url: &str, csrf: &str, stars: Option<u64>, roll: &[Signatory]) -> String {
    let count = (db::FOUNDERS.len() + roll.len()) as i64;
    let mut s = String::with_capacity(32 * 1024);
    head(base_url, stars, &mut s);

    s.push_str("<main>\n");
    s.push_str(manifesto_html(!roll.is_empty()));

    s.push_str("<section class=\"signatures\">\n");

    if !roll.is_empty() {
        s.push_str("<ol class=\"signers\">\n");
        for (i, sig) in roll.iter().enumerate() {
            let n = rank(i);
            let login = esc(&sig.login);
            let _ = writeln!(
                s,
                "<li id=\"s{n}\"><a href=\"https://github.com/{login}\" rel=\"nofollow ugc\">\
                 <span class=\"n\">#{n}</span>{login}</a></li>",
            );
        }
        s.push_str("</ol>\n");
    }

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

    // POST, not a link: a GET would be followed by crawlers and prefetchers.
    // The hidden field is the twin of the jpmf_csrf cookie.
    let _ = write!(
        s,
        "<form method=\"post\" action=\"/sign\" class=\"sign\">\n\
         <input type=\"hidden\" name=\"csrf\" value=\"{}\">\n\
         <button>{GITHUB_MARK}<span>Sign it</span></button>\n</form>\n",
        esc(csrf)
    );
    let _ = write!(
        s,
        "<form method=\"post\" action=\"/unsign\" class=\"unsign\">\n\
         <input type=\"hidden\" name=\"csrf\" value=\"{}\">\n\
         <button type=\"submit\">Remove my signature</button>\n</form>\n",
        esc(csrf)
    );
    s.push_str("</section>\n</main>\n");

    close(&mut s);
    s
}

/// Confirmation before removal: ordinals are permanent, so unsigning gives up
/// a number for good and should not happen on a single click.
pub fn confirm_unsign(
    base_url: &str,
    stars: Option<u64>,
    login: &str,
    ordinal: i64,
    token: &str,
) -> String {
    let mut s = String::with_capacity(8 * 1024);
    head(base_url, stars, &mut s);
    s.push_str("<main>\n<h1>Remove your signature?</h1>\n");
    let _ = writeln!(
        s,
        "<p>GitHub says you are <strong>{}</strong>, currently <strong>#{ordinal}</strong>.</p>",
        esc(login)
    );
    s.push_str(
        "<p>If you remove your signature now and sign again later, you'll join \
         the end of the list rather than returning to this spot.</p>\n",
    );
    let _ = write!(
        s,
        "<form method=\"post\" action=\"/unsign/confirm\">\n\
         <input type=\"hidden\" name=\"token\" value=\"{}\">\n\
         <button>Yes, remove #{ordinal}</button>\n</form>\n",
        esc(token)
    );
    s.push_str("<p><a href=\"/\">No, keep it</a></p>\n</main>\n");
    close(&mut s);
    s
}

/// A small standalone page for the paths that are not the manifesto.
pub fn notice(base_url: &str, stars: Option<u64>, heading: &str, body: &str) -> String {
    let mut s = String::with_capacity(8 * 1024);
    head(base_url, stars, &mut s);
    let _ = write!(
        s,
        "<main>\n<h1>{}</h1>\n<p>{}</p>\n<p><a href=\"/\">Back to the manifesto</a></p>\n</main>\n",
        esc(heading),
        esc(body)
    );
    close(&mut s);
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
                signed_at: format!("2026-01-{:02} 00:00:00.000", i + 1),
            })
            .collect()
    }

    #[test]
    fn manifesto_renders_its_table() {
        let html = manifesto_html(false);
        assert!(html.contains("<table>"), "values table did not render");
        assert!(!html.contains("| Autonomous agents |"));
    }

    /// The card advertised "Do you speak it?" for a while after the manifesto
    /// said "Do you prompt it?", because the strings lived in two places.
    #[test]
    fn head_metadata_comes_from_the_manifesto() {
        let fm = front_matter();
        assert!(MANIFESTO_MD.contains(&format!("# {}", fm.title)));
        assert!(MANIFESTO_MD.contains(&format!("*{}*", fm.subtitle)));
        assert!(!fm.description.is_empty());
        assert!(!fm.description.contains('*'), "emphasis must be stripped");

        let html = page("http://localhost:8100", "csrf0", None, &[]);
        assert!(html.contains(&format!("<title>{}</title>", fm.title)));
        assert!(html.contains(&format!(
            "<meta property=\"og:description\" content=\"{}\">",
            fm.description
        )));
    }

    /// The manifesto opens with an HTML comment explaining that its first two
    /// lines feed the page metadata. That comment must not become the metadata.
    #[test]
    fn the_manifesto_comment_is_not_mistaken_for_prose() {
        assert!(MANIFESTO_MD.starts_with("<!--"));
        let fm = front_matter();
        assert!(!fm.description.starts_with("<!--"));
        assert!(!fm.description.contains("load-bearing"));
        assert!(!fm.title.is_empty() && !fm.subtitle.is_empty());
    }

    /// Facebook and LinkedIn render the large card on a first scrape only when
    /// the dimensions travel with the image, so the declared size has to be
    /// the size of the bytes actually served.
    #[test]
    fn the_card_image_is_declared_with_its_real_dimensions() {
        let fm = front_matter();
        let html = page("http://localhost:8100", "csrf0", None, &[]);
        assert!(
            html.contains("<meta property=\"og:image\" content=\"http://localhost:8100/og.png\">")
        );
        assert!(html.contains("<meta property=\"og:image:width\" content=\"1200\">"));
        assert!(html.contains("<meta property=\"og:image:height\" content=\"630\">"));
        assert!(html.contains("<meta property=\"og:image:type\" content=\"image/png\">"));
        assert!(html.contains("<meta property=\"og:image:alt\""));

        // X reads its own tags first; each mirrors the og: value beside it.
        assert!(
            html.contains("<meta name=\"twitter:image\" content=\"http://localhost:8100/og.png\">")
        );
        // The card title carries the subtitle too: it is the whole message
        // wherever the image does not load. The tab keeps the bare title.
        let card_title = format!("{}. {}", fm.title, fm.subtitle);
        assert!(html.contains(&format!("<title>{}</title>", fm.title)));
        assert!(html.contains(&format!(
            "<meta property=\"og:title\" content=\"{card_title}\">"
        )));
        assert!(html.contains(&format!(
            "<meta name=\"twitter:title\" content=\"{card_title}\">"
        )));
        assert!(html.contains(&format!(
            "<meta name=\"twitter:description\" content=\"{}\">",
            fm.description
        )));
        assert!(html.contains("<meta name=\"twitter:image:alt\""));
        assert!(!html.contains("twitter:site"));

        // PNG: signature, then IHDR with width and height as big-endian u32.
        assert!(OG_IMAGE.starts_with(b"\x89PNG\r\n\x1a\n"));
        let dim = |at: usize| u32::from_be_bytes(OG_IMAGE[at..at + 4].try_into().unwrap());
        assert_eq!((dim(16), dim(20)), (1200, 630));
        // WhatsApp drops thumbnails past a few hundred KB.
        assert!(OG_IMAGE.len() < 200 * 1024);
    }

    #[test]
    fn paragraphs_join_soft_wraps() {
        let md = "# T\n\n*sub*\n\nfirst line\nsecond line\n\nnext";
        assert_eq!(
            paragraphs(md),
            ["# T", "*sub*", "first line second line", "next"]
        );
    }

    #[test]
    fn manifesto_sha_is_stable_and_hex() {
        let sha = manifesto_sha();
        assert_eq!(sha.len(), 64);
        assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(sha, manifesto_sha());
    }

    /// With nobody signed there is no list and no placeholder — the closing
    /// line stands on its own and the button is the only invitation.
    #[test]
    fn an_empty_roll_renders_no_list_at_all() {
        let html = page("http://localhost:8100", "csrf0", None, &[]);
        assert!(html.contains("2 signatures and counting"));
        assert!(!html.contains("<ol class=\"signers\">"));
        assert!(!html.contains("Nobody yet"));
        assert!(html.contains("action=\"/sign\""));
    }

    #[test]
    fn the_list_is_numbered_contiguously_after_the_founders() {
        let html = page(
            "http://localhost:8100",
            "csrf0",
            Some(12),
            &roll(&["a", "b", "c"]),
        );
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
        let list = signer_list(&page("http://localhost:8100", "csrf0", None, &r));
        let third = list.find("#3").unwrap();
        let fourth = list.find("#4").unwrap();
        assert!(third < fourth);
        assert!(list[third..fourth].contains("oldest"));
        assert!(list[fourth..].contains("newest"));
    }

    /// The manifesto's last line introduces the names when there are names, so
    /// its shape matters as much as the title's does.
    #[test]
    fn the_closing_line_gains_a_colon_only_when_someone_has_signed() {
        let plain = page("http://localhost:8100", "csrf0", None, &[]);
        let signed = page("http://localhost:8100", "csrf0", None, &roll(&["a"]));

        let last = MANIFESTO_MD
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap()
            .trim_end();
        assert!(plain.contains(last));
        assert!(!plain.contains(&format!("{last}:")));
        assert!(signed.contains(&format!("{last}:")));
    }

    #[test]
    fn introducing_only_touches_the_last_line() {
        let md = "# T\n\nbody\n\nlast line\n";
        assert_eq!(introducing(md), "# T\n\nbody\n\nlast line:\n");
    }

    #[test]
    fn the_sign_button_carries_the_github_mark() {
        let html = page("http://localhost:8100", "csrf0", None, &[]);
        let form = html.split("action=\"/sign\"").nth(1).expect("no sign form");
        let button = form.split("</form>").next().unwrap();
        assert!(button.contains("<svg"), "no mark on the sign button");
        assert!(button.contains("Sign it"));
    }

    #[test]
    fn logins_are_escaped() {
        let html = page("http://localhost:8100", "csrf0", None, &roll(&["<script>"]));
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn a_missing_star_count_shows_no_number() {
        let html = page("http://localhost:8100", "csrf0", None, &[]);
        assert!(
            html.contains(">GitHub</span>"),
            "should fall back to a label"
        );
        assert!(
            !html.contains(">0</span>"),
            "0 stars and no answer must differ"
        );
    }

    #[test]
    fn star_counts_are_compact() {
        assert_eq!(compact(0), "0");
        assert_eq!(compact(7), "7");
        assert_eq!(compact(999), "999");
        assert_eq!(compact(1_000), "1k");
        assert_eq!(compact(1_250), "1.2k");
        assert_eq!(compact(9_940), "9.9k");
        assert_eq!(compact(12_400), "12k");
        assert_eq!(compact(109_500), "110k");
        assert_eq!(compact(2_400_000), "2.4m");
    }

    #[test]
    fn thousands_separates() {
        assert_eq!(thousands(2), "2");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_204), "1,204");
        assert_eq!(thousands(1_000_000), "1,000,000");
    }
}
