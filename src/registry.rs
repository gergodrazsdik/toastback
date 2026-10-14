// SPDX-License-Identifier: GPL-3.0-or-later
//! Small safe wrappers over the HKCU registry calls toastback needs.

use anyhow::{Context as _, Result};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_EXPAND_SZ, REG_OPTION_NON_VOLATILE, REG_SZ,
    REG_VALUE_TYPE, RRF_NOEXPAND, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegDeleteTreeW, RegGetValueW, RegOpenKeyExW, RegSetValueExW,
};
use windows::core::{HSTRING, PCWSTR};

/// A string value's type: `REG_SZ`, or `REG_EXPAND_SZ` (like the user `Path`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Plain,
    Expand,
}

pub fn key_exists(subkey: &str) -> bool {
    let mut key = HKEY::default();
    // SAFETY: valid predefined root, NUL-terminated string, out-pointer to a local HKEY.
    let err = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            &HSTRING::from(subkey),
            None,
            KEY_READ,
            &mut key,
        )
    };
    if err.is_ok() {
        // SAFETY: key was just opened.
        unsafe {
            let _ = RegCloseKey(key);
        }
    }
    err.is_ok()
}

/// Read a string value (`""` is the default value). `Ok(None)` when the key or value is missing.
pub fn get_string(subkey: &str, name: &str) -> Result<Option<(String, Kind)>> {
    let (subkey_w, name_w) = (HSTRING::from(subkey), HSTRING::from(name));
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND;
    let mut buf: Vec<u16> = vec![0; 256];
    loop {
        let mut bytes = (buf.len() * 2) as u32;
        let mut kind = REG_VALUE_TYPE::default();
        // SAFETY: buf holds `bytes` bytes; all strings are NUL-terminated HSTRINGs.
        let err = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                &subkey_w,
                &name_w,
                flags,
                Some(&mut kind),
                Some(buf.as_mut_ptr().cast()),
                Some(&mut bytes),
            )
        };
        if err == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if err == ERROR_MORE_DATA {
            buf.resize((bytes as usize).div_ceil(2) + 1, 0);
            continue;
        }
        err.ok()
            .with_context(|| format!(r"read HKCU\{subkey} {name:?}"))?;
        let len = (bytes as usize / 2).min(buf.len());
        let text = String::from_utf16_lossy(&buf[..len]);
        let text = text.trim_end_matches('\0').to_string();
        let kind = if kind == REG_EXPAND_SZ {
            Kind::Expand
        } else {
            Kind::Plain
        };
        return Ok(Some((text, kind)));
    }
}

/// Create the key if needed and set a string value (`""` is the default value).
pub fn set_string(subkey: &str, name: &str, value: &str, kind: Kind) -> Result<()> {
    let mut key = HKEY::default();
    // SAFETY: valid predefined root, NUL-terminated strings, out-pointer to a local HKEY.
    unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &HSTRING::from(subkey),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        )
        .ok()
        .with_context(|| format!(r"create HKCU\{subkey}"))?;
    }
    let data: Vec<u8> = value
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect();
    let reg_kind = match kind {
        Kind::Plain => REG_SZ,
        Kind::Expand => REG_EXPAND_SZ,
    };
    // SAFETY: key was opened above; data is a NUL-terminated UTF-16 buffer.
    let res = unsafe { RegSetValueExW(key, &HSTRING::from(name), None, reg_kind, Some(&data)) };
    // SAFETY: key is a valid open handle.
    unsafe {
        let _ = RegCloseKey(key);
    }
    res.ok()
        .with_context(|| format!(r"set HKCU\{subkey} {name:?}"))
}

/// Delete a key and everything below it. A missing key is fine.
pub fn delete_tree(subkey: &str) -> Result<()> {
    // SAFETY: valid predefined root and a NUL-terminated string.
    let err = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(subkey)) };
    if err == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    err.ok().with_context(|| format!(r"delete HKCU\{subkey}"))
}
