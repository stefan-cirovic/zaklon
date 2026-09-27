//! Looking things up on the internet for the assistant, only when someone
//! switches it on for a conversation. Searches DuckDuckGo's plain HTML page
//! and reads a few result pages. Never opens addresses inside the home
//! network (no router pages, no other devices), even when a page redirects.

use std::net::IpAddr;
use std::time::Duration;

use tracing::info;

const SEARCH: &str = "https://html.duckduckgo.com/html/";
const MAX_PAGE: usize = 1_500_000;

pub struct WebPage {
    pub title: String,
    pub url: String,
    pub host: String,
    pub html: String,
}

fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => v.is_private() || v.is_loopback() || v.is_link_local() || v.is_unspecified() || v.is_broadcast() || v.octets()[0] == 100 && (v.octets()[1] & 0xC0) == 64,
        IpAddr::V6(v) => v.is_loopback() || v.is_unspecified() || (v.segments()[0] & 0xfe00) == 0xfc00 || (v.segments()[0] & 0xffc0) == 0xfe80,
    }
}

/// Only ordinary public web addresses.
pub fn is_public_url(url: &reqwest::Url) -> bool {
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    match url.host() {
        Some(url::Host::Domain(d)) => {
            let d = d.to_ascii_lowercase();
            !(d == "localhost" || d.ends_with(".localhost") || d.ends_with(".local") || d.ends_with(".lan") || d.ends_with(".home") || d.ends_with(".internal") || !d.contains('.'))
        }
        Some(url::Host::Ipv4(ip)) => !is_private_ip(IpAddr::V4(ip)),
        Some(url::Host::Ipv6(ip)) => !is_private_ip(IpAddr::V6(ip)),
        None => false,
    }
}

pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .connect_timeout(Duration::from_secs(6))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) Zaklon")
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() > 4 || !is_public_url(attempt.url()) {
                attempt.stop()
            } else {
                attempt.follow()
            }
        }))
        .build()
        .expect("http client")
}

/// Result links from DuckDuckGo's HTML page: (title, url).
pub fn parse_results(html: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for part in html.split("class=\"result__a\"").skip(1) {
        let Some(href) = part.split("href=\"").nth(1).and_then(|h| h.split('"').next()) else { continue };
        let href = href.replace("&amp;", "&");
        let target = match href.split("uddg=").nth(1) {
            Some(enc) => percent_decode(enc.split('&').next().unwrap_or_default()),
            None if href.starts_with("http") => href.clone(),
            None => continue,
        };
        let title_html = part.split_once('>').map(|(_, rest)| rest.split("</a>").next().unwrap_or_default()).unwrap_or_default();
        let title = crate::assistant::strip_html(title_html);
        if let Ok(u) = reqwest::Url::parse(&target) {
            if is_public_url(&u) && !u.host_str().unwrap_or_default().ends_with("duckduckgo.com") && !out.iter().any(|(_, x): &(String, String)| *x == target) {
                out.push((title.trim().to_string(), target));
            }
        }
    }
    out
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                match u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("zz"), 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Search, then read up to `pages` result pages that are public HTML.
pub async fn look_up(http: &reqwest::Client, query: &str, pages: usize) -> Vec<WebPage> {
    info!(query = %query, "online research: searching");
    let Ok(res) = http.post(SEARCH).form(&[("q", query)]).send().await else { return Vec::new() };
    let Ok(html) = res.text().await else { return Vec::new() };
    let mut out = Vec::new();
    for (title, url) in parse_results(&html).into_iter().take(pages + 3) {
        if out.len() >= pages {
            break;
        }
        let Ok(parsed) = reqwest::Url::parse(&url) else { continue };
        // The name must not lead into the home network either.
        let host = parsed.host_str().unwrap_or_default().to_string();
        let port = parsed.port_or_known_default().unwrap_or(443);
        let resolves_public = match tokio::net::lookup_host((host.as_str(), port)).await {
            Ok(addrs) => {
                let addrs: Vec<_> = addrs.collect();
                !addrs.is_empty() && addrs.iter().all(|a| !is_private_ip(a.ip()))
            }
            Err(_) => false,
        };
        if !resolves_public {
            continue;
        }
        info!(host = %host, "online research: reading a page");
        let Ok(res) = http.get(parsed.clone()).send().await else { continue };
        let html_type = res
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|t| t.contains("html"));
        if !res.status().is_success() || !html_type {
            continue;
        }
        let Ok(bytes) = res.bytes().await else { continue };
        let html = String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_PAGE)]).into_owned();
        out.push(WebPage { title: if title.is_empty() { host.clone() } else { title }, url, host, html });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_public_addresses() {
        let ok = |s: &str| is_public_url(&reqwest::Url::parse(s).unwrap());
        assert!(ok("https://sr.wikipedia.org/wiki/Voda"));
        assert!(!ok("http://192.168.1.1/admin"));
        assert!(!ok("http://127.0.0.1:8481/api/devices"));
        assert!(!ok("http://10.0.0.5/"));
        assert!(!ok("http://[::1]/"));
        assert!(!ok("http://router.local/"));
        assert!(!ok("http://localhost:8481/"));
        assert!(!ok("http://intranet/"));
        assert!(!ok("file:///C:/Windows/win.ini"));
        assert!(!ok("http://100.100.1.1/"), "carrier-grade NAT / VPN");
    }

    #[test]
    fn duckduckgo_results_are_read() {
        let html = r#"<a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.prakticnaekologija.rs%2Fkako%2Dprecistiti%2Dvodu%2F&amp;rut=77b3">Kako <b>prečistiti</b> vodu</a>
<a class="result__a" href="//duckduckgo.com/l/?uddg=http%3A%2F%2F192.168.1.1%2F&amp;rut=1">Router</a>
<a class="result__a" href="https://example.org/x">Example</a>"#;
        let r = parse_results(html);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0], ("Kako prečistiti vodu".to_string(), "https://www.prakticnaekologija.rs/kako-precistiti-vodu/".to_string()));
        assert_eq!(r[1].1, "https://example.org/x");
    }
}
