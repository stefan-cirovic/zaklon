//! Windows Firewall: phones must be let in. The installer runs without
//! administrator rights, so it cannot add a rule; Windows asks on first
//! start instead, and a dismissed question leaves "block" rules behind.
//! Household checks this and offers to fix it (Windows asks for consent).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FirewallState {
    /// Checked on Windows; elsewhere there is nothing to do.
    pub checked: bool,
    pub firewall_on: bool,
    /// An enabled inbound rule allows this program.
    pub allowed: bool,
    /// An enabled inbound rule blocks this program (a dismissed prompt).
    pub blocked: bool,
    pub error: Option<String>,
}

impl FirewallState {
    /// Phones can reach the hub as far as the firewall is concerned.
    pub fn ok(&self) -> bool {
        !self.checked || !self.firewall_on || (self.allowed && !self.blocked)
    }
}

/// PowerShell single-quoted string.
#[cfg(windows)]
fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

#[cfg(windows)]
fn encoded(script: &str) -> String {
    use base64::Engine;
    let utf16: Vec<u8> = script.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    base64::engine::general_purpose::STANDARD.encode(utf16)
}

#[cfg(windows)]
fn exe() -> Result<String, String> {
    std::env::current_exe().map(|p| p.display().to_string()).map_err(|e| e.to_string())
}

#[cfg(windows)]
pub fn status() -> FirewallState {
    use std::os::windows::process::CommandExt;
    let exe = match exe() {
        Ok(e) => e,
        Err(e) => return FirewallState { error: Some(e), ..Default::default() },
    };
    let script = format!(
        r#"$ErrorActionPreference = 'SilentlyContinue'
$exe = {exe}
$on = [bool](Get-NetFirewallProfile | Where-Object {{ $_.Enabled -eq 'True' }})
$rules = Get-NetFirewallApplicationFilter -Program $exe | Get-NetFirewallRule | Where-Object {{ $_.Enabled -eq 'True' -and $_.Direction -eq 'Inbound' }}
[pscustomobject]@{{ checked = $true; firewall_on = $on; allowed = [bool]($rules | Where-Object {{ $_.Action -eq 'Allow' }}); blocked = [bool]($rules | Where-Object {{ $_.Action -eq 'Block' }}); error = $null }} | ConvertTo-Json -Compress
"#,
        exe = ps_quote(&exe)
    );
    let out = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", &encoded(&script)])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(0x0800_0000)
        .output();
    match out {
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stdout);
            let line = text.lines().rev().find(|l| l.trim_start().starts_with('{')).unwrap_or_default();
            serde_json::from_str(line).unwrap_or_else(|_| FirewallState { error: Some("Windows did not answer about the firewall".into()), ..Default::default() })
        }
        Err(e) => FirewallState { error: Some(e.to_string()), ..Default::default() },
    }
}

/// Let phones in: remove block rules for this program and add allow rules.
/// Windows shows its administrator consent prompt; this returns when the
/// person has answered (and the rules are in place, or not).
#[cfg(windows)]
pub fn allow() -> FirewallState {
    use std::os::windows::process::CommandExt;
    let exe = match exe() {
        Ok(e) => e,
        Err(e) => return FirewallState { error: Some(e), ..Default::default() },
    };
    let inner = format!(
        r#"$exe = {exe}
Get-NetFirewallApplicationFilter -Program $exe -ErrorAction SilentlyContinue | Get-NetFirewallRule | Where-Object {{ $_.Direction -eq 'Inbound' -and $_.Action -eq 'Block' }} | Remove-NetFirewallRule -ErrorAction SilentlyContinue
Get-NetFirewallRule -DisplayName 'Zaklon app (program*' -ErrorAction SilentlyContinue | Remove-NetFirewallRule
New-NetFirewallRule -DisplayName 'Zaklon app (program TCP)' -Direction Inbound -Action Allow -Protocol TCP -Program $exe -Profile Private,Public,Domain | Out-Null
New-NetFirewallRule -DisplayName 'Zaklon app (program UDP)' -Direction Inbound -Action Allow -Protocol UDP -Program $exe -Profile Private,Public,Domain | Out-Null
"#,
        exe = ps_quote(&exe)
    );
    // A normal PowerShell starts an elevated one (UAC prompt) and waits for it.
    let outer = format!(
        "try {{ Start-Process powershell.exe -Verb RunAs -WindowStyle Hidden -Wait -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-EncodedCommand','{}' }} catch {{ exit 1 }}",
        encoded(&inner)
    );
    let _ = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", &encoded(&outer)])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(0x0800_0000)
        .status();
    status()
}

#[cfg(not(windows))]
pub fn status() -> FirewallState {
    FirewallState::default()
}

#[cfg(not(windows))]
pub fn allow() -> FirewallState {
    FirewallState::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ok_means_phones_can_come_in() {
        let base = FirewallState { checked: true, firewall_on: true, ..Default::default() };
        assert!(!base.ok(), "no rule yet");
        assert!(FirewallState { allowed: true, ..base.clone() }.ok());
        assert!(!FirewallState { allowed: true, blocked: true, ..base.clone() }.ok(), "a block rule wins");
        assert!(FirewallState { firewall_on: false, ..base.clone() }.ok(), "firewall off");
        assert!(FirewallState::default().ok(), "not Windows");
    }

    /// Reading changes nothing; safe anywhere.
    #[test]
    #[cfg(windows)]
    fn status_can_be_read() {
        let s = status();
        assert!(s.checked || s.error.is_some(), "{s:?}");
    }

    #[test]
    #[cfg(windows)]
    fn quotes_are_escaped() {
        assert_eq!(ps_quote("C:\\O'Brien\\zaklon.exe"), "'C:\\O''Brien\\zaklon.exe'");
    }
}
