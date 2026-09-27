//! "Make a Wi-Fi network": the laptop becomes the Wi-Fi network for the
//! household's phones when there is no router (a power cut, a cabin). Uses
//! Windows' own Mobile hotspot through PowerShell and WinRT, so nothing
//! extra is installed. The network is called "Zaklon" and has a password the
//! hub keeps. If the person had their own name and password set for Windows'
//! Mobile hotspot, they are kept and put back when Zaklon's network is
//! turned off.

use serde::{Deserialize, Serialize};

pub const SSID: &str = "Zaklon";
pub const SETTING_PASSPHRASE: &str = "hotspot_passphrase";
/// The person's own hotspot name and password from before Zaklon changed
/// them (JSON [`AccessPoint`]; empty once they are back).
pub const SETTING_PREVIOUS: &str = "hotspot_previous";

/// A name and password for Windows' Mobile hotspot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessPoint {
    pub ssid: String,
    pub passphrase: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HotspotState {
    /// Windows can make a hotspot on this computer.
    pub supported: bool,
    pub on: bool,
    pub ssid: String,
    pub passphrase: String,
    pub clients: u32,
    pub error: Option<String>,
    /// From `start`: the person's own name and password that Zaklon's
    /// replaced just now. Kept by the hub, never sent to the interface.
    #[serde(default, skip_serializing)]
    pub previous: Option<AccessPoint>,
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
# Some drivers never finish; give up after 30 s instead of waiting forever.
function AwaitOp($op, $type) { $t = $asTaskOp.MakeGenericMethod($type).Invoke($null, @($op)); if (-not $t.Wait(30000)) { throw 'Windows did not answer in time' }; $t.Result }
function AwaitAction($a) { $t = $asTaskAction.Invoke($null, @($a)); if (-not $t.Wait(30000)) { throw 'Windows did not answer in time' } }
# No Wi-Fi adapter, no hotspot (802.11 adapters report physical medium 9).
if (-not (Get-NetAdapter -Physical -ErrorAction SilentlyContinue | Where-Object { $_.NdisPhysicalMedium -eq 9 })) { throw 'no Wi-Fi adapter' }
# The hotspot hangs off a network connection; with no internet, any known connection will do.
$profile = [Windows.Networking.Connectivity.NetworkInformation]::GetInternetConnectionProfile()
if ($null -eq $profile) { $profile = [Windows.Networking.Connectivity.NetworkInformation]::GetConnectionProfiles() | Select-Object -First 1 }
if ($null -eq $profile) { throw 'no network connection to make a hotspot from' }
$tm = [Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager]::CreateFromConnectionProfile($profile)
function Report($err, $previous = $null) {
  $cfg = $tm.GetCurrentAccessPointConfiguration()
  [pscustomobject]@{ supported = $true; on = ($tm.TetheringOperationalState -eq 'On'); ssid = $cfg.Ssid; passphrase = $cfg.Passphrase; clients = [uint32]$tm.ClientCount; error = $err; previous = $previous } | ConvertTo-Json -Compress
}
"#;

/// A backstop for a PowerShell that hangs anyway; longer than the two 30 s
/// waits a script may do.
#[cfg(windows)]
const SCRIPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(90);

#[cfg(windows)]
fn run(script: &str) -> HotspotState {
    // `$previous` (set by `start` before it changes anything) is reported on
    // failure too, so the person's own settings are never lost.
    let full = format!("$previous = $null\ntry {{\n{PRELUDE}\n{script}\n}} catch {{ [pscustomobject]@{{ supported = $false; on = $false; ssid = ''; passphrase = ''; clients = 0; error = $_.Exception.Message; previous = $previous }} | ConvertTo-Json -Compress }}");
    match crate::powershell::run(&full, SCRIPT_TIMEOUT) {
        Ok(out) => serde_json::from_str(crate::powershell::last_json_line(&out))
            .unwrap_or_else(|_| HotspotState { error: Some("Windows did not answer about the hotspot".into()), ..Default::default() }),
        Err(e) => HotspotState { error: Some(e), ..Default::default() },
    }
}

#[cfg(windows)]
pub fn status() -> HotspotState {
    run("Report $null")
}

/// Name the network "Zaklon" with our password, then switch it on. When
/// that replaces the person's own name and password, the result carries
/// them in `previous`, for [`stop`] to put back.
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
  if ($cfg.Ssid -ne '{SSID}') {{ $previous = [pscustomobject]@{{ ssid = $cfg.Ssid; passphrase = $cfg.Passphrase }} }}
  $cfg.Ssid = '{SSID}'
  $cfg.Passphrase = '{pass}'
  AwaitAction ($tm.ConfigureAccessPointAsync($cfg))
}}
if ($tm.TetheringOperationalState -ne 'On') {{
  $r = AwaitOp ($tm.StartTetheringAsync()) ([Windows.Networking.NetworkOperators.NetworkOperatorTetheringOperationResult])
  if ($r.Status -ne 'Success') {{ Report ("" + $r.Status + " " + $r.AdditionalErrorMessage).Trim() $previous; return }}
}}
Report $null $previous
"#
    ))
}

/// Switch the network off. If Zaklon's name is still set, put the person's
/// own name and password (`previous`) back.
#[cfg(windows)]
pub fn stop(previous: Option<&AccessPoint>) -> HotspotState {
    run(&stop_script(previous))
}

/// The person's name and password are their own text, so they go into the
/// script as base64 (see [`crate::powershell`]).
#[cfg_attr(not(windows), allow(dead_code))]
fn stop_script(previous: Option<&AccessPoint>) -> String {
    let restore = match previous {
        Some(p) => format!(
            r#"
$cfg = $tm.GetCurrentAccessPointConfiguration()
if ($cfg.Ssid -eq '{SSID}') {{
  $cfg.Ssid = {ssid}
  $cfg.Passphrase = {pass}
  AwaitAction ($tm.ConfigureAccessPointAsync($cfg))
}}
"#,
            ssid = crate::powershell::text(&p.ssid),
            pass = crate::powershell::text(&p.passphrase),
        ),
        None => String::new(),
    };
    format!(
        r#"
if ($tm.TetheringOperationalState -eq 'On') {{
  $r = AwaitOp ($tm.StopTetheringAsync()) ([Windows.Networking.NetworkOperators.NetworkOperatorTetheringOperationResult])
  if ($r.Status -ne 'Success') {{ Report ("" + $r.Status + " " + $r.AdditionalErrorMessage).Trim(); return }}
}}
{restore}
Report $null
"#
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
pub fn stop(_previous: Option<&AccessPoint>) -> HotspotState {
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
    fn the_persons_own_network_goes_back_safely() {
        let own = AccessPoint { ssid: "Ana\u{2019}s phone'; calc; '".into(), passphrase: "p\u{2018}ss\"$(calc)".into() };
        let script = stop_script(Some(&own));
        assert!(script.contains("ConfigureAccessPointAsync"));
        assert!(!script.contains("Ana") && !script.contains('\u{2019}') && !script.contains('\u{2018}') && !script.contains("$(calc)"), "{script}");
        assert!(!stop_script(None).contains("ConfigureAccessPointAsync"), "nothing to put back");

        // What `start` reports: the previous settings come in, but never go out to the interface.
        let s: HotspotState = serde_json::from_str(r#"{"supported":true,"on":true,"ssid":"Zaklon","passphrase":"abcd2345ef","clients":0,"error":null,"previous":{"ssid":"Home","passphrase":"secret123"}}"#).unwrap();
        assert_eq!(s.previous, Some(AccessPoint { ssid: "Home".into(), passphrase: "secret123".into() }));
        let out = serde_json::to_string(&s).unwrap();
        assert!(!out.contains("previous") && !out.contains("secret123"), "{out}");
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
