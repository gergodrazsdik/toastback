// SPDX-License-Identifier: GPL-3.0-or-later
//! Toast XML (escaped, testable on the host) and showing/removing toasts (Windows).

use crate::spec::{BODY_ACTION, OnClick, Scenario, Sound, Spec, Style};

pub const AUMID: &str = "Toastback";
pub const GROUP: &str = "toastback";

/// Escape text for XML element content and double-quoted attributes.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // XML 1.0 forbids most control characters, even escaped
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            '\u{fffe}' | '\u{ffff}' => {}
            c => out.push(c),
        }
    }
    out
}

/// `toastback://act?id=…&a=…&t=…`. Ids and tokens are `[A-Za-z0-9._-]`, so nothing needs encoding.
pub fn activation_uri(id: &str, action: &str, token: &str) -> String {
    format!("toastback://act?id={id}&a={action}&t={token}")
}

/// Images to put on the toast, already copied to the cache.
#[derive(Default)]
pub struct Images<'a> {
    pub icon: Option<&'a str>,
    pub image: Option<&'a str>,
}

/// The toast's tag. Windows compares tags exactly, while file names ignore case: `Deploy` and
/// `deploy` share one pending file, so they have to be one toast too.
pub fn tag(id: &str) -> String {
    id.to_ascii_lowercase()
}

/// Build the toast XML. Every piece of user text goes through [`escape`].
pub fn build_xml(spec: &Spec, id: &str, token: &str, images: &Images) -> String {
    let mut x = String::from("<toast");
    if spec.body_click() != OnClick::None {
        let uri = activation_uri(id, BODY_ACTION, token);
        x += &format!(r#" launch="{}" activationType="protocol""#, escape(&uri));
    }
    let scenario = match spec.scenario {
        Scenario::Default => None,
        Scenario::Reminder => Some("reminder"),
        Scenario::Alarm => Some("alarm"),
        Scenario::Urgent => Some("urgent"),
    };
    if let Some(s) = scenario {
        x += &format!(r#" scenario="{s}""#);
    }
    if spec.buttons.iter().any(|b| b.style.is_some()) {
        x += r#" useButtonStyle="true""#;
    }
    x += r#"><visual><binding template="ToastGeneric">"#;
    x += &format!("<text>{}</text>", escape(&spec.title));
    if let Some(body) = &spec.body {
        x += &format!("<text>{}</text>", escape(body));
    }
    if let Some(icon) = images.icon {
        x += &format!(
            r#"<image placement="appLogoOverride" src="{}"/>"#,
            escape(icon)
        );
    }
    if let Some(image) = images.image {
        x += &format!(r#"<image src="{}"/>"#, escape(image));
    }
    if let Some(a) = &spec.attribution {
        x += &format!(r#"<text placement="attribution">{}</text>"#, escape(a));
    }
    x += "</binding></visual>";

    if !spec.buttons.is_empty() {
        x += "<actions>";
        for b in &spec.buttons {
            let label = escape(&b.label);
            let style = match b.style {
                Some(Style::Success) => r#" hint-buttonStyle="Success""#,
                Some(Style::Critical) => r#" hint-buttonStyle="Critical""#,
                None => "",
            };
            if spec.run_for(&b.id).is_some() {
                let uri = escape(&activation_uri(id, &b.id, token));
                x += &format!(
                    r#"<action content="{label}" activationType="protocol" arguments="{uri}"{style}/>"#
                );
            } else {
                // nothing to run: let Windows just dismiss the toast, no process is started
                x += &format!(
                    r#"<action content="{label}" activationType="system" arguments="dismiss"{style}/>"#
                );
            }
        }
        x += "</actions>";
    }

    x += match spec.sound {
        Sound::Default => "",
        Sound::Silent => r#"<audio silent="true"/>"#,
        Sound::Reminder => r#"<audio src="ms-winsoundevent:Notification.Reminder"/>"#,
        Sound::Alarm if spec.scenario == Scenario::Alarm => {
            r#"<audio src="ms-winsoundevent:Notification.Looping.Alarm" loop="true"/>"#
        }
        Sound::Alarm => r#"<audio src="ms-winsoundevent:Notification.Looping.Alarm"/>"#,
    };
    x += "</toast>";
    x
}

#[cfg(windows)]
pub use imp::*;

#[cfg(windows)]
mod imp {
    use anyhow::{Context as _, Result};
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::Foundation::{DateTime, IReference, PropertyValue};
    use windows::UI::Notifications::{
        NotificationSetting, ToastNotification, ToastNotificationManager,
    };
    use windows::core::{HSTRING, Interface as _};

    use super::{AUMID, GROUP, tag};

    /// Seconds between 1601-01-01 (Windows epoch) and 1970-01-01.
    const EPOCH_DIFF: i64 = 11_644_473_600;

    pub fn show(xml: &str, id: &str, expires_at_unix: u64) -> Result<()> {
        let doc = XmlDocument::new()?;
        doc.LoadXml(&HSTRING::from(xml))
            .context("toast XML rejected")?;
        let toast = ToastNotification::CreateToastNotification(&doc)?;
        toast.SetTag(&HSTRING::from(tag(id)))?;
        toast.SetGroup(&HSTRING::from(GROUP))?;
        let ticks = (expires_at_unix as i64 + EPOCH_DIFF) * 10_000_000;
        let expiry: IReference<DateTime> = PropertyValue::CreateDateTime(DateTime {
            UniversalTime: ticks,
        })?
        .cast()?;
        toast.SetExpirationTime(&expiry)?;
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID))?
            .Show(&toast)
            .context("ToastNotifier.Show")?;
        Ok(())
    }

    /// Remove one toast from Action Center. Missing toasts are fine.
    pub fn remove(id: &str) -> Result<()> {
        ToastNotificationManager::History()?.RemoveGroupedTagWithId(
            &HSTRING::from(tag(id)),
            &HSTRING::from(GROUP),
            &HSTRING::from(AUMID),
        )?;
        Ok(())
    }

    /// Remove every toastback toast from Action Center.
    pub fn remove_all() -> Result<()> {
        ToastNotificationManager::History()?
            .RemoveGroupWithId(&HSTRING::from(GROUP), &HSTRING::from(AUMID))?;
        Ok(())
    }

    /// `Ok(None)` before the first toast was shown (Windows has no settings entry yet).
    pub fn setting() -> Result<Option<NotificationSetting>> {
        let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID))?;
        match notifier.Setting() {
            Ok(s) => Ok(Some(s)),
            Err(e) if e.code().0 as u32 == 0x8007_0490 => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_differing_in_case_share_a_tag() {
        assert_eq!(tag("Deploy"), tag("deploy"));
        assert_eq!(tag("mvwuj41y-z9zl"), "mvwuj41y-z9zl");
    }

    #[test]
    fn escape_drops_what_xml_forbids() {
        assert_eq!(escape("a\u{ffff}b\u{fffe}c\u{0}d\ne"), "abcd\ne");
    }
    use crate::spec::Spec;

    #[test]
    fn escapes_everything() {
        assert_eq!(
            escape(r#"<a href="x">&'</a>"#),
            "&lt;a href=&quot;x&quot;&gt;&amp;&apos;&lt;/a&gt;"
        );
        assert_eq!(escape("őű 🔔\x07\n"), "őű 🔔\n");
    }

    #[test]
    fn simple_toast_has_no_launch_or_actions() {
        let s = Spec::parse(r#"{"title": "Backup <done>", "body": "42 GB & more"}"#).unwrap();
        let x = build_xml(&s, "id1", "tok", &Images::default());
        assert_eq!(
            x,
            r#"<toast><visual><binding template="ToastGeneric"><text>Backup &lt;done&gt;</text><text>42 GB &amp; more</text></binding></visual></toast>"#
        );
    }

    #[test]
    fn buttons_and_body_click() {
        let s = Spec::parse(
            r#"{"title": "t", "scenario": "reminder", "sound": "silent",
                "buttons": [
                  {"id": "go", "label": "Go \"now\"", "style": "success", "run": {"cmd": "echo"}},
                  {"id": "later", "label": "Later"}
                ],
                "default": {"cmd": "echo default"}}"#,
        )
        .unwrap();
        let x = build_xml(
            &s,
            "id1",
            "tok",
            &Images {
                icon: Some(r"C:\c\id1-icon.png"),
                image: None,
            },
        );
        assert!(x.starts_with(
            r#"<toast launch="toastback://act?id=id1&amp;a=__body&amp;t=tok" activationType="protocol" scenario="reminder" useButtonStyle="true">"#
        ));
        assert!(x.contains(
            r#"<action content="Go &quot;now&quot;" activationType="protocol" arguments="toastback://act?id=id1&amp;a=go&amp;t=tok" hint-buttonStyle="Success"/>"#
        ));
        // no run of its own, but the default handler exists: still protocol
        assert!(x.contains(r#"arguments="toastback://act?id=id1&amp;a=later&amp;t=tok""#));
        assert!(x.contains(r#"<image placement="appLogoOverride" src="C:\c\id1-icon.png"/>"#));
        assert!(x.ends_with(r#"<audio silent="true"/></toast>"#));
    }

    #[test]
    fn button_without_any_run_dismisses() {
        let s = Spec::parse(r#"{"title": "t", "buttons": [{"id": "ok", "label": "OK"}]}"#).unwrap();
        let x = build_xml(&s, "id1", "tok", &Images::default());
        assert!(
            x.contains(r#"<action content="OK" activationType="system" arguments="dismiss"/>"#)
        );
    }
}
