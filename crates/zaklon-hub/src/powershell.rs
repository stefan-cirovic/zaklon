//! Running Windows PowerShell scripts safely. The script goes in as
//! `-EncodedCommand` (PowerShell reads a script from stdin line by line,
//! which breaks multi-line blocks). Text from outside (a program path, a
//! network name) never appears in the script itself: it goes in as base64
//! and is decoded inside, so no character in it can end a quoted string.
//! Quoting would not be enough: PowerShell also treats the typographic
//! quotes ‘ ’ ‚ ‛ as quote characters. A script that hangs is ended after a
//! timeout, so a waiting request (and its thread) always comes back.

#![cfg_attr(not(windows), allow(dead_code))]

/// UTF-16LE, base64: the form `-EncodedCommand` expects.
pub fn encode(script: &str) -> String {
    use base64::Engine;
    let utf16: Vec<u8> = script.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    base64::engine::general_purpose::STANDARD.encode(utf16)
}

/// A PowerShell expression whose value is `s`. Only base64 characters
/// appear in the script.
pub fn text(s: &str) -> String {
    format!("([Text.Encoding]::Unicode.GetString([Convert]::FromBase64String('{}')))", encode(s))
}

/// Run a script with no window and return what it printed. An error if
/// PowerShell could not start or the script did not finish within
/// `timeout` (it is then ended).
#[cfg(windows)]
pub fn run(script: &str, timeout: std::time::Duration) -> Result<String, String> {
    use std::io::Read;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    let mut child = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", &encode(script)])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(0x0800_0000) // no console window
        .spawn()
        .map_err(|e| e.to_string())?;
    // Read on another thread, so a script that prints a lot cannot block on a full pipe.
    let mut stdout = child.stdout.take().ok_or("no output from PowerShell")?;
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout.read_to_end(&mut out);
        out
    });
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(100)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Windows did not answer in time".into());
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    let out = reader.join().unwrap_or_default();
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// The last line of the output that looks like a JSON object (the scripts
/// end by printing one; anything before it is ignored).
pub fn last_json_line(output: &str) -> &str {
    output.lines().rev().find(|l| l.trim_start().starts_with('{')).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_never_contains_the_text_itself() {
        let hostile = "D:\\Ana\u{2019}s apps\\x\u{2019}; calc; \u{2018}'\"$(calc)`\\zaklon-app.exe";
        let expr = text(hostile);
        let (head, rest) = expr.split_once('\'').expect("a quoted literal");
        let (literal, tail) = rest.split_once('\'').expect("a closing quote");
        assert_eq!(head, "([Text.Encoding]::Unicode.GetString([Convert]::FromBase64String(");
        assert_eq!(tail, ")))");
        assert!(literal.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=')), "{literal}");
    }

    #[test]
    fn last_json_line_skips_noise() {
        assert_eq!(last_json_line("WARNING: x\n{\"a\":1}\r\n\n"), "{\"a\":1}");
        assert_eq!(last_json_line("nothing"), "");
    }

    /// PowerShell gets back exactly the text, typographic quotes included.
    #[test]
    #[cfg(windows)]
    fn text_round_trips_through_powershell() {
        use base64::Engine;
        let original = "D:\\Ana\u{2019}s apps\\x\u{2019}; Write-Output pwned; \u{2018}\\\u{0107}\u{0161}\u{0436}\\zaklon-app.exe";
        let script = format!("[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes({}))", text(original));
        let out = run(&script, std::time::Duration::from_secs(60)).expect("PowerShell runs");
        let expected = base64::engine::general_purpose::STANDARD.encode(original.as_bytes());
        assert_eq!(out.trim(), expected, "{out}");
        assert!(!out.contains("pwned"));
    }

    #[test]
    #[cfg(windows)]
    fn a_hanging_script_is_ended() {
        let started = std::time::Instant::now();
        let r = run("Start-Sleep -Seconds 30", std::time::Duration::from_secs(2));
        assert!(r.is_err());
        assert!(started.elapsed() < std::time::Duration::from_secs(20));
    }
}
