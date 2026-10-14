// SPDX-License-Identifier: GPL-3.0-or-later
//! M0 spikes: throwaway code to check the platform assumptions in PLAN.md §10.
//! Results go to docs/decisions/000-spikes.md; this module is removed in M1.

use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use windows::Data::Xml::Dom::XmlDocument;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
use windows::Win32::Foundation::{HWND, LPARAM, S_OK, WPARAM};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW,
};
use windows::Win32::UI::Controls::{
    TASKDIALOG_BUTTON, TASKDIALOG_NOTIFICATIONS, TASKDIALOGCONFIG, TDF_ALLOW_DIALOG_CANCELLATION,
    TDN_CREATED, TaskDialogIndirect,
};
use windows::Win32::UI::WindowsAndMessaging::{
    HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE, SetForegroundWindow, SetWindowPos,
};
use windows::core::{HRESULT, HSTRING, PCWSTR};

const AUMID: &str = "Toastback";

pub fn run(which: &str) -> Result<u8> {
    match which {
        // Spike 2: stdout/stderr/exit code through WSL interop from a GUI-subsystem exe.
        "stdio" => {
            println!("spike-id-123");
            eprintln!("this went to stderr");
            Ok(3)
        }
        "register" => register().map(|()| 0),
        "unregister" => unregister().map(|()| 0),
        "toast" => toast().map(|()| 0),
        "dialog" => dialog().map(|()| 0),
        _ => bail!("unknown spike {which}"),
    }
}

fn state_dir() -> Result<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA not set")?;
    Ok(PathBuf::from(base).join("toastback"))
}

/// Spike 3+4 setup: copy the exe to the state dir, register the AUMID and the protocol.
fn register() -> Result<()> {
    let bin = state_dir()?.join("bin");
    std::fs::create_dir_all(&bin)?;
    let exe = bin.join("toastback.exe");
    // read + write, not fs::copy, so no Zone.Identifier stream is carried over
    std::fs::write(&exe, std::fs::read(std::env::current_exe()?)?)?;
    let icon = bin.join("icon.png");
    std::fs::write(&icon, include_bytes!("../res/icon.png"))?;

    let aumid_key = format!(r"Software\Classes\AppUserModelId\{AUMID}");
    set_value(&aumid_key, "DisplayName", "toastback")?;
    set_value(&aumid_key, "IconUri", &icon.to_string_lossy())?;
    set_value(r"Software\Classes\toastback", "", "URL:toastback")?;
    set_value(r"Software\Classes\toastback", "URL Protocol", "")?;
    set_value(
        r"Software\Classes\toastback\shell\open\command",
        "",
        &format!("\"{}\" activate \"%1\"", exe.display()),
    )?;
    println!("registered {}", exe.display());
    Ok(())
}

fn unregister() -> Result<()> {
    for key in [
        format!(r"Software\Classes\AppUserModelId\{AUMID}"),
        r"Software\Classes\toastback".to_string(),
    ] {
        // SAFETY: plain registry call with a valid predefined key and a NUL-terminated string.
        let err = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(key.as_str())) };
        println!("delete {key}: {err:?}");
    }
    Ok(())
}

fn set_value(subkey: &str, name: &str, value: &str) -> Result<()> {
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
        .with_context(|| format!("create HKCU\\{subkey}"))?;
    }
    let data: Vec<u8> = value
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect();
    let name = HSTRING::from(name);
    // SAFETY: key was opened above; data is a NUL-terminated UTF-16 buffer.
    let res = unsafe { RegSetValueExW(key, &name, None, REG_SZ, Some(&data)) };
    // SAFETY: key is a valid open handle.
    unsafe {
        let _ = RegCloseKey(key);
    }
    res.ok().with_context(|| format!("set HKCU\\{subkey}"))?;
    Ok(())
}

/// Spike 3+4: show a toast through the registry-only AUMID, with protocol-activated buttons.
fn toast() -> Result<()> {
    let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID))?;
    println!("notifier setting: {:?}", notifier.Setting());
    let xml = r#"<toast launch="toastback://act?id=spike&amp;a=__body&amp;t=x" activationType="protocol">
  <visual><binding template="ToastGeneric">
    <text>toastback spike őű 🔔</text>
    <text>Click a button: it should run toastback.exe activate with no console flash.</text>
  </binding></visual>
  <actions>
    <action content="Yes" activationType="protocol" arguments="toastback://act?id=spike&amp;a=yes&amp;t=x" hint-buttonStyle="Success"/>
    <action content="Dismiss" activationType="system" arguments="dismiss"/>
  </actions>
</toast>"#;
    let doc = XmlDocument::new()?;
    doc.LoadXml(&HSTRING::from(xml))?;
    let toast = ToastNotification::CreateToastNotification(&doc)?;
    toast.SetTag(&HSTRING::from("spike"))?;
    toast.SetGroup(&HSTRING::from("toastback"))?;
    notifier.Show(&toast)?;
    println!("shown");
    Ok(())
}

/// Spike 4: protocol activation lands here; log the URI so the click can be verified.
pub fn activate(uri: &str) -> Result<()> {
    let dir = state_dir()?;
    std::fs::create_dir_all(&dir)?;
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("spike.log"))?;
    writeln!(f, "{secs} activate {uri}")?;
    Ok(())
}

unsafe extern "system" fn dialog_callback(
    hwnd: HWND,
    msg: TASKDIALOG_NOTIFICATIONS,
    _w: WPARAM,
    _l: LPARAM,
    _ref: isize,
) -> HRESULT {
    if msg == TDN_CREATED {
        // SAFETY: hwnd is the dialog window handed to us by TaskDialogIndirect.
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE,
            );
            let _ = SetForegroundWindow(hwnd);
        }
    }
    S_OK
}

/// Spike 5: TaskDialogIndirect with custom buttons (needs the Common Controls v6 manifest).
fn dialog() -> Result<()> {
    let title = HSTRING::from("toastback spike");
    let main = HSTRING::from("Reboot now? őű 🔔");
    let content = HSTRING::from("TaskDialog spike: pick a button, the choice is printed.");
    let labels = [HSTRING::from("Reboot"), HSTRING::from("Later")];
    let buttons = [
        TASKDIALOG_BUTTON {
            nButtonID: 100,
            pszButtonText: PCWSTR(labels[0].as_ptr()),
        },
        TASKDIALOG_BUTTON {
            nButtonID: 101,
            pszButtonText: PCWSTR(labels[1].as_ptr()),
        },
    ];
    let config = TASKDIALOGCONFIG {
        cbSize: size_of::<TASKDIALOGCONFIG>() as u32,
        dwFlags: TDF_ALLOW_DIALOG_CANCELLATION,
        pszWindowTitle: PCWSTR(title.as_ptr()),
        pszMainInstruction: PCWSTR(main.as_ptr()),
        pszContent: PCWSTR(content.as_ptr()),
        cButtons: buttons.len() as u32,
        pButtons: buttons.as_ptr(),
        pfCallback: Some(dialog_callback),
        ..Default::default()
    };
    let mut chosen = 0i32;
    // SAFETY: config and every string/button it points to outlive the call.
    unsafe { TaskDialogIndirect(&config, Some(&mut chosen), None, None)? };
    println!("chosen button: {chosen}");
    Ok(())
}
