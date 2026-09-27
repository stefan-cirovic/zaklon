//! "Start with Windows". The entry is the app's own value in
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, named like the app
//! (the uninstaller removes that name), with the program path in quotes: an
//! unquoted path with a space in it could start another program.
//!
//! It is on by default. The person's choice is kept only as an opt-out, in
//! `HKCU\Software\Zaklon`: outside the data folder, so neither an upgrade nor
//! a restored backup changes it. Every start of an installed copy puts the
//! entry back unless the person turned it off, because an upgrade that
//! uninstalls the old version first also removes the entry (the installer
//! puts it back too, see `windows/installer-hooks.nsh`, which uses the same
//! names). Development builds and copies that are not installed never touch
//! it, so they cannot take the installed app's place at the next logon.

use std::path::Path;

/// The value's name in the Run key: the product name, which the uninstaller removes.
const VALUE_NAME: &str = "Zaklon";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// Where Task Manager keeps its own on/off switch for the same entry.
const APPROVED_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
/// Task Manager's "enabled" (a disabled entry has a time stamp in the last 8 bytes).
const APPROVED_ENABLED: [u8; 12] = [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
const PREFS_KEY: &str = r"Software\Zaklon";
/// 1 when the person turned "Start with Windows" off.
const OPT_OUT_VALUE: &str = "AutostartOff";

/// Whether this copy looks after the entry: an installed release build
/// using its own data folder (not one pointed elsewhere with `ZAKLON_ROOT`).
pub fn managed() -> bool {
    cfg!(windows)
        && !cfg!(debug_assertions)
        && crate::desktop::install_dir().is_some()
        && std::env::var_os("ZAKLON_ROOT").is_none()
}

/// What the Run entry says: the program in quotes, started hidden in the tray.
fn command(exe: &Path) -> String {
    format!("\"{}\" {}", exe.display(), crate::desktop::MINIMIZED_ARG)
}

/// At start: put the entry back (or fix an old unquoted one) unless the
/// person turned it off.
pub fn ensure() {
    if !managed() {
        return;
    }
    if reg::get_dword(PREFS_KEY, OPT_OUT_VALUE) == Some(1) {
        tracing::info!("start with Windows is off (the person's choice)");
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    let wanted = command(&exe);
    if reg::get_string(RUN_KEY, VALUE_NAME).as_deref() == Some(wanted.as_str()) {
        return;
    }
    // Only the Run value: if the person switched Zaklon off in Task Manager, that stays.
    match reg::set_string(RUN_KEY, VALUE_NAME, &wanted) {
        Ok(()) => tracing::info!("start with Windows: entry set to {wanted}"),
        Err(e) => tracing::warn!("could not set start with Windows: {e}"),
    }
}

/// On, as far as Windows is concerned (Task Manager's switch included).
pub fn is_enabled() -> bool {
    if reg::get_string(RUN_KEY, VALUE_NAME).is_none() {
        return false;
    }
    match reg::get_binary(APPROVED_KEY, VALUE_NAME) {
        Some(bytes) if bytes.len() >= 8 => bytes[bytes.len() - 8..].iter().all(|b| *b == 0),
        _ => true,
    }
}

/// The tray switch: the person's own choice, kept for later starts.
pub fn set(on: bool) -> std::io::Result<()> {
    if on {
        let exe = std::env::current_exe()?;
        reg::set_string(RUN_KEY, VALUE_NAME, &command(&exe))?;
        if reg::key_exists(APPROVED_KEY) {
            reg::set_binary(APPROVED_KEY, VALUE_NAME, &APPROVED_ENABLED)?;
        }
        reg::delete_value(PREFS_KEY, OPT_OUT_VALUE)
    } else {
        reg::delete_value(RUN_KEY, VALUE_NAME)?;
        reg::set_dword(PREFS_KEY, OPT_OUT_VALUE, 1)
    }
}

/// Values under `HKEY_CURRENT_USER`.
#[cfg(windows)]
mod reg {
    use std::io;
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteKeyValueW, RegGetValueW, RegOpenKeyExW, RegSetValueExW, HKEY,
        HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_BINARY, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_ROUTINE_FLAGS,
        REG_SZ, REG_VALUE_TYPE, RRF_RT_REG_BINARY, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    fn check(code: WIN32_ERROR) -> io::Result<()> {
        if code == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(code as i32))
        }
    }

    /// The value's bytes, or None if it (or its key) does not exist.
    fn get(key: &str, name: &str, kind: REG_ROUTINE_FLAGS) -> Option<Vec<u8>> {
        let (key, name) = (wide(key), wide(name));
        // SAFETY: the strings are NUL-terminated; the buffer is as long as
        // the size passed, which RegGetValueW updates to what it wrote.
        unsafe {
            let mut len = 0u32;
            if RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), kind, null_mut(), null_mut(), &mut len)
                != ERROR_SUCCESS
            {
                return None;
            }
            let mut buf = vec![0u8; len as usize];
            if RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), kind, null_mut(), buf.as_mut_ptr().cast(), &mut len)
                != ERROR_SUCCESS
            {
                return None;
            }
            buf.truncate(len as usize);
            Some(buf)
        }
    }

    pub fn get_string(key: &str, name: &str) -> Option<String> {
        let bytes = get(key, name, RRF_RT_REG_SZ)?;
        let mut units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        while units.last() == Some(&0) {
            units.pop();
        }
        Some(String::from_utf16_lossy(&units))
    }

    pub fn get_dword(key: &str, name: &str) -> Option<u32> {
        let bytes = get(key, name, RRF_RT_REG_DWORD)?;
        Some(u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?))
    }

    pub fn get_binary(key: &str, name: &str) -> Option<Vec<u8>> {
        get(key, name, RRF_RT_REG_BINARY)
    }

    /// Creates the key if needed.
    fn set(key: &str, name: &str, kind: REG_VALUE_TYPE, data: &[u8]) -> io::Result<()> {
        let (key, name) = (wide(key), wide(name));
        // SAFETY: NUL-terminated strings, a data pointer with its length, and
        // the key handle is closed here.
        unsafe {
            let mut handle: HKEY = null_mut();
            check(RegCreateKeyExW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                0,
                null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                null(),
                &mut handle,
                null_mut(),
            ))?;
            let r = RegSetValueExW(handle, name.as_ptr(), 0, kind, data.as_ptr(), data.len() as u32);
            RegCloseKey(handle);
            check(r)
        }
    }

    pub fn set_string(key: &str, name: &str, value: &str) -> io::Result<()> {
        let bytes: Vec<u8> = wide(value).iter().flat_map(|u| u.to_le_bytes()).collect();
        set(key, name, REG_SZ, &bytes)
    }

    pub fn set_dword(key: &str, name: &str, value: u32) -> io::Result<()> {
        set(key, name, REG_DWORD, &value.to_le_bytes())
    }

    pub fn set_binary(key: &str, name: &str, value: &[u8]) -> io::Result<()> {
        set(key, name, REG_BINARY, value)
    }

    /// Deleting a value that is not there is fine.
    pub fn delete_value(key: &str, name: &str) -> io::Result<()> {
        let (key, name) = (wide(key), wide(name));
        // SAFETY: NUL-terminated strings.
        let r = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) };
        if r == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            check(r)
        }
    }

    pub fn key_exists(key: &str) -> bool {
        let key = wide(key);
        // SAFETY: a NUL-terminated string; the handle is closed when opened.
        unsafe {
            let mut handle: HKEY = null_mut();
            if RegOpenKeyExW(HKEY_CURRENT_USER, key.as_ptr(), 0, KEY_READ, &mut handle) == ERROR_SUCCESS {
                RegCloseKey(handle);
                true
            } else {
                false
            }
        }
    }
}

/// Elsewhere there is no Run key; nothing is ever on.
#[cfg(not(windows))]
mod reg {
    use std::io;

    pub fn get_string(_key: &str, _name: &str) -> Option<String> {
        None
    }
    pub fn get_dword(_key: &str, _name: &str) -> Option<u32> {
        None
    }
    pub fn get_binary(_key: &str, _name: &str) -> Option<Vec<u8>> {
        None
    }
    pub fn set_string(_key: &str, _name: &str, _value: &str) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }
    pub fn set_dword(_key: &str, _name: &str, _value: u32) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }
    pub fn set_binary(_key: &str, _name: &str, _value: &[u8]) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }
    pub fn delete_value(_key: &str, _name: &str) -> io::Result<()> {
        Ok(())
    }
    pub fn key_exists(_key: &str) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_program_path_is_quoted() {
        let exe = Path::new(r"D:\My Apps\Zaklon\zaklon-app.exe");
        assert_eq!(command(exe), r#""D:\My Apps\Zaklon\zaklon-app.exe" --minimized"#);
    }

    #[test]
    fn test_builds_never_manage_the_entry() {
        assert!(!managed(), "a test (debug, not installed) build must not touch the Run key");
    }

    /// Writes only under a key of its own, removed at the end.
    #[test]
    #[cfg(windows)]
    fn registry_values_round_trip() {
        let key = format!(r"Software\Zaklon-tests-{}", std::process::id());
        assert_eq!(reg::get_string(&key, "s"), None);
        assert!(!reg::key_exists(&key));
        reg::set_string(&key, "s", "\"C:\\Ana\u{2019}s\\zaklon-app.exe\" --minimized").unwrap();
        assert!(reg::key_exists(&key));
        assert_eq!(reg::get_string(&key, "s").as_deref(), Some("\"C:\\Ana\u{2019}s\\zaklon-app.exe\" --minimized"));
        reg::set_dword(&key, "d", 1).unwrap();
        assert_eq!(reg::get_dword(&key, "d"), Some(1));
        assert_eq!(reg::get_string(&key, "d"), None, "a number is not text");
        reg::set_binary(&key, "b", &APPROVED_ENABLED).unwrap();
        assert_eq!(reg::get_binary(&key, "b").as_deref(), Some(&APPROVED_ENABLED[..]));
        for name in ["s", "d", "b", "never-there"] {
            reg::delete_value(&key, name).unwrap();
        }
        assert_eq!(reg::get_dword(&key, "d"), None);
        let wide: Vec<u16> = key.encode_utf16().chain(Some(0)).collect();
        // SAFETY: a NUL-terminated string.
        unsafe { windows_sys::Win32::System::Registry::RegDeleteTreeW(windows_sys::Win32::System::Registry::HKEY_CURRENT_USER, wide.as_ptr()) };
        assert!(!reg::key_exists(&key));
    }
}
