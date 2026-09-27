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

fn is_private_v4(v: std::net::Ipv4Addr) -> bool {
    let o = v.octets();
    v.is_private()
        || v.is_loopback()
        || v.is_link_local()
        || v.is_unspecified()
        || v.is_broadcast()
        || o[0] == 0 // "this network"
        || o[0] == 100 && (o[1] & 0xC0) == 64 // carrier-grade NAT, VPNs
        || o[0] == 198 && (o[1] & 0xFE) == 18 // benchmarking
        || o[0] >= 224 // multicast and reserved
}

/// Addresses inside the home network (or not addresses of the internet at all).
pub fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => is_private_v4(v),
        IpAddr::V6(v) => {
            // IPv4 wrapped in IPv6 (::ffff:a.b.c.d and the NAT64 prefix 64:ff9b::/96).
            if let Some(v4) = v.to_ipv4_mapped() {
                return is_private_v4(v4);
            }
            let seg = v.segments();
            let o = v.octets();
            // The NAT64 prefix and the old IPv4-compatible form (::a.b.c.d).
            if (seg[0] == 0x64 && seg[1] == 0xff9b && seg[2..6].iter().all(|x| *x == 0)) || seg[..6].iter().all(|x| *x == 0) {
                return is_private_v4(std::net::Ipv4Addr::new(o[12], o[13], o[14], o[15]));
            }
            // 6to4 (2002:a.b.c.d::/48) carries an IPv4 address too.
            if seg[0] == 0x2002 && is_private_v4(std::net::Ipv4Addr::new(o[2], o[3], o[4], o[5])) {
                return true;
            }
            v.is_loopback() || v.is_unspecified() || v.is_multicast() || (seg[0] & 0xfe00) == 0xfc00 || (seg[0] & 0xffc0) == 0xfe80
        }
    }
}

/// Resolves names like the system does, but never to an address inside the
/// home network. Used for every connection, so redirects and a name that
/// answers differently the second time are covered too.
struct PublicOnly;

impl reqwest::dns::Resolve for PublicOnly {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            let addrs: Vec<std::net::SocketAddr> = tokio::net::lookup_host((host.as_str(), 0)).await?.filter(|a| !is_private_ip(a.ip())).collect();
            if addrs.is_empty() {
                return Err("that address is inside the home network or unknown".into());
            }
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// Only ordinary public web addresses.
pub fn is_public_url(url: &reqwest::Url) -> bool {
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    match url.host() {
        Some(url::Host::Domain(d)) => {
            let d = d.trim_end_matches('.').to_ascii_lowercase();
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
        // A proxy from the environment would look names up itself, past `PublicOnly`.
        .no_proxy()
        .dns_resolver(std::sync::Arc::new(PublicOnly))
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
        let host = parsed.host_str().unwrap_or_default().to_string();
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
        // Read at most MAX_PAGE, however big the page is.
        let mut res = res;
        let mut bytes: Vec<u8> = Vec::new();
        while bytes.len() < MAX_PAGE {
            match res.chunk().await {
                Ok(Some(c)) => bytes.extend_from_slice(&c),
                _ => break,
            }
        }
        bytes.truncate(MAX_PAGE);
        let html = String::from_utf8_lossy(&bytes).into_owned();
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
        assert!(!ok("http://[::ffff:192.168.1.1]/"), "IPv4 inside IPv6");
        assert!(!ok("http://[64:ff9b::c0a8:101]/"), "NAT64 form of 192.168.1.1");
        assert!(!ok("http://printer.local./"), "trailing dot");
        assert!(!ok("http://0.0.0.1/"));
        assert!(!ok("http://3232235777/"), "192.168.1.1 written as one number");
        assert!(ok("http://[2a00:1450:4001::200e]/"), "public IPv6");
        assert!(!ok("http://[::c0a8:101]/"), "IPv4-compatible form of 192.168.1.1");
        assert!(!ok("http://[2002:c0a8:101::1]/"), "6to4 around 192.168.1.1");
        assert!(ok("http://[2002:5db8:d822::1]/"), "6to4 around a public address");
        assert!(!ok("http://[::]/"));
    }

    #[tokio::test]
    async fn names_that_point_home_are_refused() {
        use reqwest::dns::Resolve;
        let r = PublicOnly.resolve("localhost".parse().unwrap()).await;
        assert!(r.is_err(), "localhost resolves only to loopback");
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
