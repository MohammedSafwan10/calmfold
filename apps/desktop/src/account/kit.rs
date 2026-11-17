//! The Ottero Cloud Emergency Kit as a printable HTML page.
//!
//! HTML rather than PDF: it needs no new dependency (a PDF writer such as
//! `printpdf` pulls in a font and layout stack), the browser's print dialog
//! prints it or saves it as PDF on every platform, and the text stays
//! selectable for copying. The page is self-contained: no scripts, no
//! network requests, monochrome so it prints well.

use ottero_crypto::Zeroizing;

pub struct KitContents<'a> {
    pub email: &'a str,
    pub secret_key: &'a str,
    /// Only at sign-up; it is never stored, so later kits can't include it.
    pub recovery_code: Option<&'a str>,
    pub created: &'a str,
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

#[must_use]
pub fn html(kit: &KitContents<'_>) -> Zeroizing<String> {
    let recovery = match kit.recovery_code {
        Some(code) => format!(
            r#"<section><h2>Recovery code</h2><p class="value">{}</p>
<p class="note">If you forget your password, this code is the only way back to your Ottero Cloud data.</p></section>"#,
            escape(code)
        ),
        None => r#"<section><h2>Recovery code</h2>
<p class="note">Shown once, when you created your account. Keep the first Emergency Kit you saved.</p></section>"#
            .to_owned(),
    };
    Zeroizing::new(format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'">
<title>Ottero Emergency Kit</title>
<style>
  :root {{ color-scheme: light; }}
  body {{ margin: 0; background: white; color: black; font: 15px/1.5 -apple-system, "Segoe UI", Inter, system-ui, sans-serif; }}
  main {{ max-width: 40rem; margin: 3rem auto; padding: 0 2rem; }}
  header {{ border-bottom: 2px solid black; padding-bottom: 1rem; margin-bottom: 2rem; }}
  h1 {{ font-size: 1.75rem; margin: 0; letter-spacing: -0.01em; }}
  .meta {{ opacity: 0.65; margin: 0.25rem 0 0; }}
  section {{ margin: 0 0 1.75rem; }}
  h2 {{ font-size: 0.8rem; text-transform: uppercase; letter-spacing: 0.08em; opacity: 0.65; margin: 0 0 0.35rem; }}
  .value {{ font: 600 1.2rem/1.4 "JetBrains Mono", ui-monospace, Menlo, Consolas, monospace; margin: 0; word-break: break-all; }}
  .blank {{ border-bottom: 1px solid black; height: 2rem; }}
  .note {{ margin: 0.35rem 0 0; opacity: 0.75; }}
  ol {{ padding-left: 1.25rem; }}
  li {{ margin-bottom: 0.4rem; }}
  footer {{ border-top: 1px solid black; padding-top: 1rem; margin-top: 2.5rem; opacity: 0.75; font-size: 0.85rem; }}
  @media print {{ main {{ margin: 0 auto; }} .hint {{ display: none; }} }}
</style>
</head>
<body>
<main>
<header>
<h1>Ottero Emergency Kit</h1>
<p class="meta">Created {created}</p>
</header>
<p class="hint">Print this page or save it as a PDF (File → Print), then delete this file from your computer.</p>
<section><h2>Email</h2><p class="value">{email}</p></section>
<section><h2>Secret Key</h2><p class="value">{secret}</p>
<p class="note">You need it with your password to sign in on a new device.</p></section>
<section><h2>Password</h2><div class="blank"></div>
<p class="note">Write it here by hand if you want. Ottero never stores it.</p></section>
{recovery}
<section><h2>Signing in on a new device</h2>
<ol>
<li>Open Ottero → Settings → Account → Sign in.</li>
<li>Enter your email and password.</li>
<li>When Ottero asks, type the Secret Key above.</li>
</ol></section>
<footer>Anyone with this kit and your password can open your Ottero Cloud data. Keep it with your important papers, not on this computer or in your email. Ottero can't recover it for you.</footer>
</main>
</body>
</html>
"#,
        created = escape(kit.created),
        email = escape(kit.email),
        secret = escape(kit.secret_key),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kit_holds_the_values_escaped_and_loads_nothing() {
        let page = html(&KitContents {
            email: "sam+<x>@example.com",
            secret_key: "O1-AAAAAA-BBBBB-CCCCC-DDDDD-EEEEE",
            recovery_code: Some("ABCDEFG-HJKMNPQ"),
            created: "November 17, 2026",
        });
        assert!(page.contains("O1-AAAAAA-BBBBB-CCCCC-DDDDD-EEEEE"));
        assert!(page.contains("ABCDEFG-HJKMNPQ"));
        assert!(page.contains("sam+&lt;x&gt;@example.com"));
        assert!(!page.contains("<script"));
        assert!(
            !page.contains("http://") && !page.contains("https://") && !page.contains("src="),
            "no remote resources"
        );
        let later = html(&KitContents {
            email: "a@b.co",
            secret_key: "O1-X",
            recovery_code: None,
            created: "today",
        });
        assert!(later.contains("Shown once"));
    }
}
