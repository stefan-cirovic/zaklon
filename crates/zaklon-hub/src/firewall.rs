//! Windows Firewall: phones must be let in. The installer runs without
//! administrator rights, so it cannot add a rule; Windows asks on first
//! start instead, and a dismissed question leaves "block" rules behind.
//! Settings (and Home) check this and offer to fix it (Windows asks for consent).
//!
//! The rules Zaklon adds let in only what phones use, and only for this
//! program: the hub's TLS port and the install page (TCP), the discovery
//! beacon and DNS-SD (UDP). Where they apply:
//! - Private networks (a home Wi-Fi): from any address.
//! - Public networks: only from the addresses Windows' Mobile hotspot gives
//!   the devices that join it ([`HOTSPOT_SUBNET`]). Windows may file the
//!   hotspot's own adapter under Public, and "Make a Wi-Fi network" must keep
//!   working; a café or hotel Wi-Fi (also Public) must not see the hub.
//! - Domain networks (an employer's) are left out.
//!
//! A home Wi-Fi that Windows treats as Public (the default for a new network
//! on Windows 11) therefore keeps phones out. The check reports that
//! (`public_network`), and Settings offers to make that network Private
//! ([`make_private`], again with Windows' consent): only the real network
//! adapters Windows files as Public, never the laptop's own hotspot or a
//! virtual adapter, and never a Domain network.

use serde::{Deserialize, Serialize};

/// The addresses Windows' Mobile hotspot hands out.
pub const HOTSPOT_SUBNET: &str = "192.168.137.0/24";
/// DNS-SD (multicast DNS): how phones find the hub by themselves.
pub const MDNS_PORT: u16 = 5353;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FirewallState {
    /// Checked on Windows; elsewhere there is nothing to do.
    pub checked: bool,
    pub firewall_on: bool,
    /// An enabled inbound rule allows this program.
    pub allowed: bool,
    /// An enabled inbound rule blocks this program (a dismissed prompt).
    pub blocked: bool,
    /// This computer is on a network (other than its own hotspot), but no
    /// allow rule applies to any of them: for example a home Wi-Fi that
    /// Windows treats as Public.
    #[serde(default)]
    pub public_network: bool,
    pub error: Option<String>,
}

impl FirewallState {
    /// Phones can reach the hub as far as the firewall is concerned.
    pub fn ok(&self) -> bool {
        !self.checked || !self.firewall_on || (self.allowed && !self.blocked && !self.public_network)
    }
}

/// The ports phones use, from the hub's configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ports {
    pub tcp: Vec<u16>,
    pub udp: Vec<u16>,
}

impl Ports {
    pub fn of(cfg: &zaklon_core::Config) -> Self {
        let mut tcp = vec![cfg.port, cfg.install_port];
        let mut udp = vec![cfg.beacon_port, MDNS_PORT];
        tcp.sort_unstable();
        tcp.dedup();
        udp.sort_unstable();
        udp.dedup();
        Self { tcp, udp }
    }
}

/// The program path goes in as base64 (see [`crate::powershell`]): a
/// folder name with a quote in it can never end the string and run as code,
/// least of all in the elevated script.
#[cfg_attr(not(windows), allow(dead_code))]
fn status_script(exe: &str) -> String {
    format!(
        r#"$ErrorActionPreference = 'SilentlyContinue'
$exe = {exe}
$on = [bool](Get-NetFirewallProfile | Where-Object {{ $_.Enabled -eq 'True' }})
$off = @(Get-NetFirewallProfile | Where-Object {{ $_.Enabled -ne 'True' }} | ForEach-Object {{ [string]$_.Name }})
$rules = @(Get-NetFirewallApplicationFilter -Program $exe | Get-NetFirewallRule | Where-Object {{ $_.Enabled -eq 'True' -and $_.Direction -eq 'Inbound' }})
$allow = @($rules | Where-Object {{ $_.Action -eq 'Allow' }})
# The networks phones may come from; this laptop's own hotspot has its own rule.
$hot = @(Get-NetIPAddress -AddressFamily IPv4 | Where-Object {{ $_.IPAddress -like '192.168.137.*' }} | ForEach-Object {{ $_.InterfaceIndex }})
$nets = @(Get-NetConnectionProfile | Where-Object {{ $hot -notcontains $_.InterfaceIndex }})
$profiles = @{{ 'DomainAuthenticated' = 'Domain'; 'Private' = 'Private'; 'Public' = 'Public' }}
$bits = @{{ 'Domain' = 1; 'Private' = 2; 'Public' = 4 }}
$open = @($nets | Where-Object {{
  $name = $profiles[[string]$_.NetworkCategory]; $bit = $bits[$name]
  ($off -contains $name) -or [bool]($allow | Where-Object {{ $p = [int]$_.Profile; ($p -eq 0 -or ($p -band $bit)) -and (@(($_ | Get-NetFirewallAddressFilter).RemoteAddress) -contains 'Any') }})
}})
[pscustomobject]@{{ checked = $true; firewall_on = $on; allowed = ($allow.Count -gt 0); blocked = [bool]($rules | Where-Object {{ $_.Action -eq 'Block' }}); public_network = ($nets.Count -gt 0 -and $open.Count -eq 0); error = $null }} | ConvertTo-Json -Compress
"#,
        exe = crate::powershell::text(exe)
    )
}

/// Runs elevated: removes block rules for this program and Zaklon's older
/// rules, then adds the rules described at the top of this file.
#[cfg_attr(not(windows), allow(dead_code))]
fn allow_script(exe: &str, ports: &Ports) -> String {
    let list = |p: &[u16]| p.iter().map(u16::to_string).collect::<Vec<_>>().join(",");
    format!(
        r#"$exe = {exe}
$tcp = @({tcp})
$udp = @({udp})
Get-NetFirewallApplicationFilter -Program $exe -ErrorAction SilentlyContinue | Get-NetFirewallRule | Where-Object {{ $_.Direction -eq 'Inbound' -and $_.Action -eq 'Block' }} | Remove-NetFirewallRule -ErrorAction SilentlyContinue
Get-NetFirewallRule -DisplayName 'Zaklon app (program*' -ErrorAction SilentlyContinue | Remove-NetFirewallRule
New-NetFirewallRule -DisplayName 'Zaklon app (program TCP)' -Direction Inbound -Action Allow -Program $exe -Protocol TCP -LocalPort $tcp -Profile Private | Out-Null
New-NetFirewallRule -DisplayName 'Zaklon app (program UDP)' -Direction Inbound -Action Allow -Program $exe -Protocol UDP -LocalPort $udp -Profile Private | Out-Null
New-NetFirewallRule -DisplayName 'Zaklon app (program TCP, Wi-Fi from this laptop)' -Direction Inbound -Action Allow -Program $exe -Protocol TCP -LocalPort $tcp -Profile Public -RemoteAddress '{HOTSPOT_SUBNET}' | Out-Null
New-NetFirewallRule -DisplayName 'Zaklon app (program UDP, Wi-Fi from this laptop)' -Direction Inbound -Action Allow -Program $exe -Protocol UDP -LocalPort $udp -Profile Public -RemoteAddress '{HOTSPOT_SUBNET}' | Out-Null
"#,
        exe = crate::powershell::text(exe),
        tcp = list(&ports.tcp),
        udp = list(&ports.udp),
    )
}

/// Reads (changes nothing): the interfaces of the connected networks Windows
/// files as Public, leaving out this laptop's own hotspot and the adapters of
/// virtual machines, WSL, containers and VPNs (see
/// [`crate::discovery::VIRTUAL_HINTS`]): those are never where phones come from.
#[cfg_attr(not(windows), allow(dead_code))]
fn public_interfaces_script() -> String {
    let virtual_ = crate::discovery::VIRTUAL_HINTS.iter().map(|h| format!("'{h}'")).collect::<Vec<_>>().join(",");
    format!(
        r#"$ErrorActionPreference = 'SilentlyContinue'
$hot = @(Get-NetIPAddress -AddressFamily IPv4 | Where-Object {{ $_.IPAddress -like '192.168.137.*' }} | ForEach-Object {{ $_.InterfaceIndex }})
$virtual = @({virtual_})
$public = @(Get-NetConnectionProfile | Where-Object {{
  $alias = ([string]$_.InterfaceAlias).ToLower()
  ($hot -notcontains $_.InterfaceIndex) -and ([string]$_.NetworkCategory -eq 'Public') -and -not ($virtual | Where-Object {{ $alias.Contains($_) }})
}} | ForEach-Object {{ [int]$_.InterfaceIndex }})
[pscustomobject]@{{ interfaces = $public }} | ConvertTo-Json -Compress
"#
    )
}

/// One network this computer is connected to, as the system specification
/// shows it: the adapter's name ("Wi-Fi") and how Windows files the network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NetworkProfile {
    pub adapter: String,
    /// "Private", "Public" or "Domain".
    pub category: String,
}

/// Reads (changes nothing): the connected networks and how Windows files
/// each, leaving out this laptop's own hotspot and virtual adapters, as
/// [`public_interfaces_script`] does. The network's own name is not read.
#[cfg_attr(not(windows), allow(dead_code))]
fn profiles_script() -> String {
    let virtual_ = crate::discovery::VIRTUAL_HINTS.iter().map(|h| format!("'{h}'")).collect::<Vec<_>>().join(",");
    format!(
        r#"$ErrorActionPreference = 'SilentlyContinue'
$hot = @(Get-NetIPAddress -AddressFamily IPv4 | Where-Object {{ $_.IPAddress -like '192.168.137.*' }} | ForEach-Object {{ $_.InterfaceIndex }})
$virtual = @({virtual_})
$nets = @(Get-NetConnectionProfile | Where-Object {{
  $alias = ([string]$_.InterfaceAlias).ToLower()
  ($hot -notcontains $_.InterfaceIndex) -and -not ($virtual | Where-Object {{ $alias.Contains($_) }})
}} | ForEach-Object {{ [pscustomobject]@{{ adapter = [string]$_.InterfaceAlias; category = [string]$_.NetworkCategory }} }})
[pscustomobject]@{{ networks = $nets }} | ConvertTo-Json -Compress -Depth 3
"#
    )
}

/// The networks in the answer of [`profiles_script`] (PowerShell may write
/// a single one without the brackets). None when the answer is not one.
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_profiles(json: &str) -> Option<Vec<NetworkProfile>> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let list = match value.get("networks")? {
        serde_json::Value::Array(a) => a.clone(),
        serde_json::Value::Null => Vec::new(),
        one => vec![one.clone()],
    };
    let text = |v: &serde_json::Value, k: &str| v.get(k).and_then(|s| s.as_str()).unwrap_or_default().trim().to_string();
    Some(
        list.iter()
            .map(|n| {
                let category = match text(n, "category").as_str() {
                    "DomainAuthenticated" => "Domain".to_string(),
                    other => other.to_string(),
                };
                NetworkProfile { adapter: text(n, "adapter"), category }
            })
            .filter(|n| !n.category.is_empty())
            .collect(),
    )
}

/// The networks this computer is on and how Windows files each (Private,
/// Public, Domain). Takes PowerShell a second or more.
#[cfg(windows)]
pub fn network_profiles() -> Result<Vec<NetworkProfile>, String> {
    let out = crate::powershell::run(&profiles_script(), std::time::Duration::from_secs(60))?;
    parse_profiles(crate::powershell::last_json_line(&out)).ok_or_else(|| "Windows did not answer about its networks".to_string())
}

/// The interface numbers in the answer of [`public_interfaces_script`]
/// (PowerShell may write a single one without the brackets); anything else
/// is left out, so only whole numbers can reach the elevated script.
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_interfaces(json: &str) -> Vec<u32> {
    let value: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    let list = match value.get("interfaces") {
        Some(serde_json::Value::Array(a)) => a.clone(),
        Some(one) => vec![one.clone()],
        None => Vec::new(),
    };
    let mut out: Vec<u32> = list.iter().filter_map(|v| v.as_u64()).filter_map(|n| u32::try_from(n).ok()).collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Runs elevated: each of the given interfaces that Windows still files as
/// Public becomes Private. Only numbers go in; nothing else is changed (not
/// the firewall's rules, not a Domain network).
#[cfg_attr(not(windows), allow(dead_code))]
fn private_script(interfaces: &[u32]) -> String {
    let list = interfaces.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
    format!(
        r#"foreach ($i in @({list})) {{
  if (@(Get-NetConnectionProfile -InterfaceIndex $i -ErrorAction SilentlyContinue | Where-Object {{ [string]$_.NetworkCategory -eq 'Public' }}).Count -gt 0) {{
    Set-NetConnectionProfile -InterfaceIndex $i -NetworkCategory Private -ErrorAction SilentlyContinue
  }}
}}
"#
    )
}

/// A normal PowerShell that starts `script` in an elevated one (Windows
/// shows its consent prompt) and waits for it. The script goes in encoded.
#[cfg_attr(not(windows), allow(dead_code))]
fn elevated(script: &str) -> String {
    format!(
        "try {{ Start-Process powershell.exe -Verb RunAs -WindowStyle Hidden -Wait -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-EncodedCommand','{}' }} catch {{ exit 1 }}",
        crate::powershell::encode(script)
    )
}

/// Run `script` elevated; returns when the person has answered Windows'
/// question (and the script has run, or not).
#[cfg(windows)]
fn run_elevated(script: &str, what: &str) {
    // The person may take a while to answer Windows' question.
    if let Err(e) = crate::powershell::run(&elevated(script), std::time::Duration::from_secs(10 * 60)) {
        tracing::warn!("{what}: {e}");
    }
}

#[cfg(windows)]
fn exe() -> Result<String, String> {
    std::env::current_exe().map(|p| p.display().to_string()).map_err(|e| e.to_string())
}

#[cfg(windows)]
fn read_status(exe: &str) -> FirewallState {
    match crate::powershell::run(&status_script(exe), std::time::Duration::from_secs(60)) {
        Ok(out) => serde_json::from_str(crate::powershell::last_json_line(&out))
            .unwrap_or_else(|_| FirewallState { error: Some("Windows did not answer about the firewall".into()), ..Default::default() }),
        Err(e) => FirewallState { error: Some(e), ..Default::default() },
    }
}

#[cfg(windows)]
pub fn status() -> FirewallState {
    match exe() {
        Ok(exe) => read_status(&exe),
        Err(e) => FirewallState { error: Some(e), ..Default::default() },
    }
}

/// Let phones in (see the top of this file). Windows shows its
/// administrator consent prompt; this returns when the person has answered
/// (and the rules are in place, or not).
#[cfg(windows)]
pub fn allow(ports: &Ports) -> FirewallState {
    let exe = match exe() {
        Ok(e) => e,
        Err(e) => return FirewallState { error: Some(e), ..Default::default() },
    };
    run_elevated(&allow_script(&exe, ports), "changing the firewall");
    read_status(&exe)
}

/// The connected networks Windows files as Public (see [`public_interfaces_script`]).
#[cfg(windows)]
fn public_interfaces() -> Result<Vec<u32>, String> {
    let out = crate::powershell::run(&public_interfaces_script(), std::time::Duration::from_secs(60))?;
    Ok(parse_interfaces(crate::powershell::last_json_line(&out)))
}

/// Treat the network(s) this laptop is on as private, for a home network
/// Windows files as Public (see the top of this file). Windows shows its
/// administrator consent prompt; this returns the firewall's state after the
/// person has answered. With no such network there is nothing to ask.
#[cfg(windows)]
pub fn make_private() -> FirewallState {
    let exe = match exe() {
        Ok(e) => e,
        Err(e) => return FirewallState { error: Some(e), ..Default::default() },
    };
    match public_interfaces() {
        Ok(interfaces) if !interfaces.is_empty() => {
            tracing::info!(?interfaces, "making the network private");
            run_elevated(&private_script(&interfaces), "making the network private");
        }
        Ok(_) => tracing::info!("no network Windows treats as public"),
        Err(e) => tracing::warn!("finding the public networks: {e}"),
    }
    read_status(&exe)
}

#[cfg(not(windows))]
pub fn status() -> FirewallState {
    FirewallState::default()
}

#[cfg(not(windows))]
pub fn allow(_ports: &Ports) -> FirewallState {
    FirewallState::default()
}

#[cfg(not(windows))]
pub fn make_private() -> FirewallState {
    FirewallState::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_profiles_are_read_without_changing_anything() {
        let script = profiles_script();
        assert!(!script.contains("Set-") && !script.contains("New-") && !script.contains("Remove-"), "{script}");
        assert!(!script.contains(".Name"), "the network's own name is not read");
        let profile = |adapter: &str, category: &str| NetworkProfile { adapter: adapter.into(), category: category.into() };
        assert_eq!(
            parse_profiles(r#"{"networks":[{"adapter":"Wi-Fi","category":"Private"},{"adapter":"Ethernet","category":"DomainAuthenticated"}]}"#),
            Some(vec![profile("Wi-Fi", "Private"), profile("Ethernet", "Domain")])
        );
        // One network, written without the brackets; none; not an answer.
        assert_eq!(parse_profiles(r#"{"networks":{"adapter":"Wi-Fi","category":"Public"}}"#), Some(vec![profile("Wi-Fi", "Public")]));
        assert_eq!(parse_profiles(r#"{"networks":[]}"#), Some(vec![]));
        assert_eq!(parse_profiles("WARNING"), None);
        #[cfg(windows)]
        {
            let nets = network_profiles().expect("Windows answers");
            assert!(nets.iter().all(|n| ["Private", "Public", "Domain"].contains(&n.category.as_str())), "{nets:?}");
        }
    }

    #[test]
    fn ok_means_phones_can_come_in() {
        let base = FirewallState { checked: true, firewall_on: true, ..Default::default() };
        assert!(!base.ok(), "no rule yet");
        assert!(FirewallState { allowed: true, ..base.clone() }.ok());
        assert!(!FirewallState { allowed: true, blocked: true, ..base.clone() }.ok(), "a block rule wins");
        assert!(!FirewallState { allowed: true, public_network: true, ..base.clone() }.ok(), "no rule for this network");
        assert!(FirewallState { firewall_on: false, ..base.clone() }.ok(), "firewall off");
        assert!(FirewallState::default().ok(), "not Windows");
    }

    #[test]
    fn rules_cover_only_the_hubs_ports() {
        let root = std::env::temp_dir().join(format!("zaklon-fw-{}", std::process::id()));
        let cfg = zaklon_core::Config::load_or_init(&root).unwrap();
        let ports = Ports::of(&cfg);
        let _ = std::fs::remove_dir_all(&root);
        let mut tcp = vec![cfg.port, cfg.install_port];
        tcp.sort_unstable();
        assert_eq!(ports.tcp, tcp);
        assert!(ports.udp.contains(&cfg.beacon_port) && ports.udp.contains(&MDNS_PORT));
        assert!(!ports.tcp.contains(&cfg.local_port), "the window's port stays on this computer");

        let script = allow_script("C:\\Zaklon\\zaklon-app.exe", &ports);
        assert_eq!(script.matches("New-NetFirewallRule").count(), 4);
        assert_eq!(script.matches("-LocalPort $").count(), 4, "every rule is limited to ports");
        assert!(!script.contains("Domain"), "no rule for an employer's network");
        assert_eq!(script.matches("-Profile Public -RemoteAddress '192.168.137.0/24'").count(), 2);
        assert_eq!(script.matches("-Profile Private |").count(), 2);
    }

    #[test]
    fn the_program_path_never_appears_in_a_script() {
        let exe = "D:\\Ana\u{2019}s apps\\x\u{2019}; calc; \u{2019}\\zaklon-app.exe";
        let ports = Ports { tcp: vec![8480, 8484], udp: vec![5353, 8485] };
        for script in [status_script(exe), allow_script(exe, &ports)] {
            assert!(!script.contains('\u{2019}') && !script.contains("Ana"), "{script}");
        }
    }

    /// Building the scripts only: making a network private is never run in a test.
    #[test]
    fn making_private_changes_only_the_public_networks_found() {
        let script = private_script(&[12, 5]);
        assert!(script.contains("foreach ($i in @(12,5))"), "{script}");
        assert_eq!(script.matches("Set-NetConnectionProfile").count(), 1, "one change, for each network in turn");
        assert!(script.contains("Set-NetConnectionProfile -InterfaceIndex $i -NetworkCategory Private"), "{script}");
        assert!(script.contains("NetworkCategory -eq 'Public' }).Count -gt 0"), "only a network Windows still files as Public");
        assert!(!script.contains("Domain"), "an employer's network is never touched");
        assert!(!script.contains("Firewall"), "the firewall's rules stay as they are");
        assert!(!script.contains("Remove-") && !script.contains("New-"), "{script}");

        let elevated = elevated(&script);
        assert!(elevated.contains("-Verb RunAs"), "Windows asks for consent");
        assert!(!elevated.contains("Set-NetConnectionProfile"), "the script goes in encoded");
        assert!(elevated.contains(&crate::powershell::encode(&script)));
    }

    #[test]
    fn the_networks_to_change_are_read_safely() {
        let script = public_interfaces_script();
        assert!(!script.contains("Set-") && !script.contains("Remove-") && !script.contains("New-"), "reading only: {script}");
        assert!(script.contains("-eq 'Public'"));
        assert!(script.contains("'192.168.137.*'"), "the laptop's own hotspot is left out");
        for hint in crate::discovery::VIRTUAL_HINTS {
            assert!(hint.chars().all(|c| c.is_ascii_lowercase() || c == '-'), "{hint} stays a plain word in the script");
            assert!(script.contains(&format!("'{hint}'")), "{hint} adapters are left out");
        }

        assert_eq!(parse_interfaces(r#"{"interfaces":[12,5]}"#), vec![5, 12]);
        assert_eq!(parse_interfaces(r#"{"interfaces":7}"#), vec![7], "PowerShell may write one number without brackets");
        assert_eq!(parse_interfaces(r#"{"interfaces":[3,3]}"#), vec![3]);
        assert!(parse_interfaces(r#"{"interfaces":[]}"#).is_empty());
        assert!(parse_interfaces(r#"{"interfaces":null}"#).is_empty());
        assert!(parse_interfaces("").is_empty());
        assert!(parse_interfaces("WARNING: not JSON").is_empty());
        // Only whole numbers reach the elevated script.
        assert_eq!(parse_interfaces(r#"{"interfaces":[-1, 3.5, "4); calc", 4294967296, 9]}"#), vec![9]);
    }

    /// Reading changes nothing; safe anywhere.
    #[test]
    #[cfg(windows)]
    fn public_networks_can_be_listed() {
        assert!(public_interfaces().is_ok());
    }

    /// Reading changes nothing; safe anywhere.
    #[test]
    #[cfg(windows)]
    fn status_can_be_read() {
        let s = status();
        assert!(s.checked || s.error.is_some(), "{s:?}");
    }

    /// A path with a typographic apostrophe (and code after it) is just a
    /// path: the script still runs and simply finds no rules for it.
    #[test]
    #[cfg(windows)]
    fn a_quote_in_the_path_is_harmless() {
        let s = read_status("D:\\Ana\u{2019}s apps\\x\u{2019}; exit 7; \u{2019}\\zaklon-app.exe");
        assert!(s.error.is_none(), "{s:?}");
        assert!(s.checked && !s.allowed && !s.blocked, "{s:?}");
    }
}
