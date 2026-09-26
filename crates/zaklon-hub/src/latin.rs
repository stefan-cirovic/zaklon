//! Show Serbian Cyrillic library pages in Latin script, on the fly.
//! Only human-readable text changes (text nodes and the title/alt/aria-label
//! attributes); links, addresses, scripts and styles stay exactly as they are,
//! so every link and image keeps working. Nothing is changed on disk.

use lol_html::html_content::{Element, TextChunk};
use lol_html::{element, rewrite_str, text, RewriteStrSettings};
use zaklon_core::translit::{cyrillic_to_latin, has_cyrillic};

/// Readable attributes worth converting.
const TEXT_ATTRIBUTES: &[&str] = &["title", "alt", "aria-label", "placeholder"];

fn convert_text(t: &mut TextChunk) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let s = t.as_str();
    if has_cyrillic(s) {
        let latin = cyrillic_to_latin(s);
        // The text is already HTML-escaped; conversion only touches Cyrillic
        // letters, so it can be written back as-is.
        t.replace(&latin, lol_html::html_content::ContentType::Html);
    }
    Ok(())
}

fn convert_attributes(el: &mut Element) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    for name in TEXT_ATTRIBUTES {
        if let Some(v) = el.get_attribute(name) {
            if has_cyrillic(&v) {
                el.set_attribute(name, &cyrillic_to_latin(&v))?;
            }
        }
    }
    Ok(())
}

/// Convert an HTML page. Falls back to the original page if it cannot be parsed.
pub fn html_to_latin(html: &str) -> String {
    let result = rewrite_str(
        html,
        RewriteStrSettings {
            element_content_handlers: vec![
                text!("title", convert_text),
                text!("body", convert_text),
                text!("body *:not(script):not(style)", convert_text),
                element!("*", convert_attributes),
            ],
            ..RewriteStrSettings::new()
        },
    );
    match result {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("could not convert a page to Latin: {e}");
            html.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_text_but_not_links_or_scripts() {
        let page = r#"<html><head><title>Вода – Википедија</title>
<style>.вода{color:red}</style><script>var x = "вода";</script></head>
<body><h1 title="Вода">Вода</h1>
<p>Вода је <a href="./Хемијско_једињење" title="Хемијско једињење">хемијско једињење</a> &amp; течност.</p>
<img src="./Вода.jpg" alt="Чаша воде"></body></html>"#;
        let out = html_to_latin(page);
        assert!(out.contains("<title>Voda – Vikipedija</title>"), "{out}");
        assert!(out.contains(r#"<h1 title="Voda">Voda</h1>"#), "{out}");
        assert!(out.contains("Voda je "), "{out}");
        assert!(out.contains(">hemijsko jedinjenje</a>"), "{out}");
        assert!(out.contains("&amp; tečnost."), "escaping kept: {out}");
        assert!(out.contains(r#"href="./Хемијско_једињење""#), "links untouched: {out}");
        assert!(out.contains(r#"src="./Вода.jpg""#), "image address untouched: {out}");
        assert!(out.contains(r#"alt="Čaša vode""#), "{out}");
        assert!(out.contains(r#"var x = "вода";"#), "scripts untouched: {out}");
        assert!(out.contains(".вода{color:red}"), "styles untouched: {out}");
    }

    #[test]
    fn latin_pages_are_unchanged() {
        let page = "<html><body><p>Već latinica.</p></body></html>";
        assert_eq!(html_to_latin(page), page);
    }
}
