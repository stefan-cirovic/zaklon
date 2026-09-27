//! "Is there a newer Zaklon?" Once a day, when enabled (the default, and a
//! switch in Household), the hub asks GitHub for the latest release. It only
//! tells people; downloading and installing is their choice. Without
//! internet it stays quiet.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::HubState;

pub const RELEASES_API: &str = "https://api.github.com/repos/stefan-cirovic/zaklon/releases/latest";
pub const RELEASES_PAGE: &str = "https://github.com/stefan-cirovic/zaklon/releases";
const EVERY: Duration = Duration::from_secs(24 * 3600);

#[derive(Debug, Clone, Default, Serialize)]
pub struct UpdateState {
    pub enabled: bool,
    pub current: String,
    pub latest: Option<String>,
    pub newer: bool,
    pub url: Option<String>,
    /// RFC 3339 of the last successful check.
    pub checked_at: Option<String>,
    pub error: Option<String>,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// "v1.2.3" or "1.2.3" -> (1, 2, 3); anything else is not a version.
pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let core = s.split(['-', '+']).next()?;
    let mut it = core.split('.').map(|p| p.parse::<u64>().ok());
    let v = (it.next()??, it.next().unwrap_or(Some(0))?, it.next().unwrap_or(Some(0))?);
    Some(v)
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

pub struct Updates {
    state: Mutex<UpdateState>,
    http: reqwest::Client,
}

impl Updates {
    pub fn new(enabled: bool) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(UpdateState { enabled, current: env!("CARGO_PKG_VERSION").into(), ..Default::default() }),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                .user_agent(concat!("Zaklon/", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("http client"),
        })
    }

    pub fn state(&self) -> UpdateState {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    pub fn set_enabled(&self, on: bool) {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).enabled = on;
    }

    /// Ask GitHub now.
    pub async fn check(&self) -> UpdateState {
        let res = self.http.get(RELEASES_API).header("accept", "application/vnd.github+json").send().await;
        let outcome: Result<Option<Release>, String> = match res {
            Ok(r) if r.status() == reqwest::StatusCode::NOT_FOUND => Ok(None), // no release published yet
            Ok(r) if r.status().is_success() => r.json::<Release>().await.map(Some).map_err(|e| e.to_string()),
            Ok(r) => Err(format!("GitHub replied {}", r.status())),
            Err(e) => Err(if e.is_connect() || e.is_timeout() { "no internet".into() } else { e.to_string() }),
        };
        let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        match outcome {
            Ok(release) => {
                st.error = None;
                st.checked_at = Some(crate::backup::now_rfc3339());
                match release.filter(|r| !r.draft && !r.prerelease) {
                    Some(r) => {
                        st.newer = is_newer(&r.tag_name, &st.current);
                        st.latest = Some(r.tag_name.trim_start_matches(['v', 'V']).to_string());
                        st.url = Some(r.html_url.unwrap_or_else(|| RELEASES_PAGE.into()));
                        if st.newer {
                            info!(latest = %r.tag_name, "a newer Zaklon is available");
                        }
                    }
                    None => {
                        st.newer = false;
                        st.latest = None;
                        st.url = None;
                    }
                }
            }
            Err(e) => st.error = Some(e),
        }
        st.clone()
    }

    /// Check once a day while enabled.
    pub fn start(self: &Arc<Self>, _state: Arc<HubState>) {
        let me = self.clone();
        tokio::spawn(async move {
            // Let the hub settle first.
            tokio::time::sleep(Duration::from_secs(60)).await;
            loop {
                if me.state().enabled {
                    me.check().await;
                }
                tokio::time::sleep(EVERY).await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare() {
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("0.2"), Some((0, 2, 0)));
        assert_eq!(parse_version("1.0.0-rc.1"), Some((1, 0, 0)));
        assert_eq!(parse_version("latest"), None);
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("v1.0.0", "0.9.12"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("v0.0.9", "0.1.0"));
        assert!(!is_newer("nonsense", "0.1.0"));
    }
}
