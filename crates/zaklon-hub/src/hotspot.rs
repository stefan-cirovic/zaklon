//! "Make a Wi-Fi network": the laptop becomes the Wi-Fi network for the
//! household's phones when there is no router (a power cut, a cabin). Uses
//! Windows' own Mobile hotspot through PowerShell and WinRT, so nothing
//! extra is installed. The network is called "Zaklon" and has a password the
//! hub keeps.

use serde::{Deserialize, Serialize};

pub const SSID: &str = "Zaklon";
pub const SETTING_PASSPHRASE: &str = "hotspot_passphrase";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HotspotState {
    /// Windows can make a hotspot on this computer.
    pub supported: bool,
    pub on: bool,
    pub ssid: String,
    pub passphrase: String,
    pub clients: u32,
    pub error: Option<String>,
}

/// Easy to read out and type on a phone: no 0/O, 1/l.
pub fn new_passphrase() -> String {
    use rand::Rng;
    const CHARS: &[u8] = b"abcdefghijkmnpqrstuvwxyz23456789";
    let mut rng = rand::thread_rng();
    (0..10).map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char).collect()
}

/// Passphrases we make: lower-case letters and digits, 8 to 63 long. Anything
/// else (for example from a backup someone else made) is replaced, so no
/// text but ours ever reaches the PowerShell script.
pub fn is_safe_passphrase(p: &str) -> bool {
    (8..=63).contains(&p.len()) && p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

/// What a phone camera understands as "join this Wi-Fi".
pub fn wifi_qr(ssid: &str, passphrase: &str) -> String {
    let esc = |s: &str| s.replace('\\', "\\\\").replace(';', "\\;").replace(',', "\\,").replace(':', "\\:").replace('"', "\\\"");
    format!("WIFI:T:WPA;S:{};P:{};;", esc(ssid), esc(passphrase))
}

#[cfg(windows)]
const PRELUDE: &str = r#"
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Runtime.WindowsRuntime
$null = [Windows.Networking.Connectivity.NetworkInformation, Windows.Networking.Connectivity, ContentType = WindowsRuntime]
$null = [Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager, Windows.Networking.NetworkOperators, ContentType = WindowsRuntime]
$asTaskOp = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' })[0]
$asTaskAction = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncAction' })[0]
function AwaitOp($op, $type) { $t = $asTaskOp.MakeGenericMethod($type).Invoke($null, @($op)); $t.Wait(-1) | Out-Null; $t.Result }
function AwaitAction($a) { $t = $asTaskAction.Invoke($null, @($a)); $t.Wait(-1) | Out-Null }
# No Wi-Fi adapter, no hotspot (802.11 adapters report physical medium 9).
if (-not (Get-NetAdapter -Physical -ErrorAction SilentlyContinue | Where-Object { $_.NdisPhysicalMedium -eq 9 })) { throw 'no Wi-Fi adapter' }
# The hotspot hangs off a network connection; with no internet, any known connection will do.
$profile = [Windows.Networking.Connectivity.NetworkInformation]::GetInternetConnectionProfile()
if ($null -eq $profile) { $profile = [Windows.Networking.Connectivity.NetworkInformation]::GetConnectionProfiles() | Select-Object -First 1 }
if ($null -eq $profile) { throw 'no network connection to make a hotspot from' }
$tm = [Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager]::CreateFromConnectionProfile($profile)
function Report($err) {
  $cfg = $tm.GetCurrentAccessPointConfiguration()
  [pscustomobject]@{ supported = $true; on = ($tm.TetheringOperationalState -eq 'On'); ssid = $cfg.Ssid; passphrase = $cfg.Passphrase; clients = [uint32]$tm.ClientCount; error = $err } | ConvertTo-Json -Compress
}
"#;

#[cfg(windows)]
fn run(script: &str) -> HotspotState {
    let full = format!("try {{\n{PRELUDE}\n{script}\n}} catch {{ [pscustomobject]@{{ supported = $false; on = $false; ssid = ''; passphrase = ''; clients = 0; error = $_.Exception.Message }} | ConvertTo-Json -Compress }}");
    // The script goes in as -EncodedCommand (UTF-16LE, base64): PowerShell
    // reads a script from stdin line by line, which breaks multi-line blocks.
    use base64::Engine;
    let utf16: Vec<u8> = full.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    let encoded = base64::engine::general_purpose::STANDARD.encode(utf16);
    let mut cmd = std::process::Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", &encoded])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // no console window
    }
    let out = cmd.output();
    match out {
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stdout);
            let line = text.lines().rev().find(|l| l.trim_start().starts_with('{')).unwrap_or_default();
            serde_json::from_str(line).unwrap_or_else(|_| HotspotState { error: Some("Windows did not answer about the hotspot".into()), ..Default::default() })
        }
        Err(e) => HotspotState { error: Some(e.to_string()), ..Default::default() },
    }
}

#[cfg(windows)]
pub fn status() -> HotspotState {
    run("Report $null")
}

/// Name the network "Zaklon" with our password, then switch it on.
#[cfg(windows)]
pub fn start(passphrase: &str) -> HotspotState {
    if !is_safe_passphrase(passphrase) {
        return HotspotState { error: Some("unsafe passphrase".into()), ..Default::default() };
    }
    let pass = passphrase;
    run(&format!(
        r#"
$cfg = $tm.GetCurrentAccessPointConfiguration()
if ($cfg.Ssid -ne '{SSID}' -or $cfg.Passphrase -ne '{pass}') {{
  $cfg.Ssid = '{SSID}'
  $cfg.Passphrase = '{pass}'
  AwaitAction ($tm.ConfigureAccessPointAsync($cfg))
}}
if ($tm.TetheringOperationalState -ne 'On') {{
  $r = AwaitOp ($tm.StartTetheringAsync()) ([Windows.Networking.NetworkOperators.NetworkOperatorTetheringOperationResult])
  if ($r.Status -ne 'Success') {{ Report ("" + $r.Status + " " + $r.AdditionalErrorMessage).Trim(); return }}
}}
Report $null
"#
    ))
}

#[cfg(windows)]
pub fn stop() -> HotspotState {
    run(
        r#"
if ($tm.TetheringOperationalState -eq 'On') {
  $r = AwaitOp ($tm.StopTetheringAsync()) ([Windows.Networking.NetworkOperators.NetworkOperatorTetheringOperationResult])
  if ($r.Status -ne 'Success') { Report ("" + $r.Status + " " + $r.AdditionalErrorMessage).Trim(); return }
}
Report $null
"#,
    )
}

#[cfg(not(windows))]
pub fn status() -> HotspotState {
    HotspotState { error: Some("only on Windows for now".into()), ..Default::default() }
}
#[cfg(not(windows))]
pub fn start(_passphrase: &str) -> HotspotState {
    status()
}
#[cfg(not(windows))]
pub fn stop() -> HotspotState {
    status()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passphrases_are_easy_and_valid() {
        let p = new_passphrase();
        assert_eq!(p.len(), 10, "WPA2 needs at least 8");
        assert!(p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
        assert!(!p.contains(['0', 'o', '1', 'l']));
    }

    #[test]
    fn only_our_kind_of_passphrase_is_used() {
        assert!(is_safe_passphrase("abcd2345ef"));
        assert!(!is_safe_passphrase("short"));
        assert!(!is_safe_passphrase("aaaaaaaa\u{2019}; calc; \u{2019}"));
        assert!(!is_safe_passphrase("aaaaaaaa'; calc; '"));
        assert!(!is_safe_passphrase("ABCDEFGH"));
    }

    #[test]
    fn wifi_qr_escapes() {
        assert_eq!(wifi_qr("Zaklon", "abc;def"), "WIFI:T:WPA;S:Zaklon;P:abc\\;def;;");
    }

    /// Reading the state changes nothing; safe to run anywhere.
    #[test]
    #[cfg(windows)]
    fn status_can_be_read() {
        let s = status();
        assert!(s.supported || s.error.is_some(), "{s:?}");
    }
}
