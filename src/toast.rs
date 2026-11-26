// SPDX-License-Identifier: GPL-3.0-or-later
//! Toast XML (escaped, testable on the host) and showing/removing toasts (Windows).

use crate::spec::{BODY_ACTION, Progress, Scenario, Sound, Spec, Style, ToastDuration};

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
    // Every click reaches toastback, also one with nothing to run: it records the answer for
    // `toastback wait` (ADR 0018). The handler then runs nothing.
    let uri = activation_uri(id, BODY_ACTION, token);
    let mut x = format!(
        r#"<toast launch="{}" activationType="protocol""#,
        escape(&uri)
    );
    let scenario = match spec.scenario {
        Scenario::Default => None,
        Scenario::Reminder => Some("reminder"),
        Scenario::Alarm => Some("alarm"),
        Scenario::Urgent => Some("urgent"),
    };
    if let Some(s) = scenario {
        x += &format!(r#" scenario="{s}""#);
    }
    if spec.duration == ToastDuration::Long {
        x += r#" duration="long""#;
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
    if let Some(p) = &spec.progress {
        // bound to the toast's data, so `toastback update` can change it in place
        x += r#"<progress value="{progressValue}" status="{progressStatus}""#;
        if p.title.is_some() {
            x += r#" title="{progressTitle}""#;
        }
        if p.label.is_some() {
            x += r#" valueStringOverride="{progressValueString}""#;
        }
        x += "/>";
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
            let uri = escape(&activation_uri(id, &b.id, token));
            x += &format!(
                r#"<action content="{label}" activationType="protocol" arguments="{uri}"{style}/>"#
            );
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

/// The data a progress bar is bound to: Windows needs `progressStatus`, so it's always there.
pub fn progress_data(p: &Progress) -> Vec<(&'static str, String)> {
    let mut data = vec![
        ("progressValue", p.value.to_xml_value()),
        ("progressStatus", p.status.clone().unwrap_or_default()),
    ];
    if let Some(t) = &p.title {
        data.push(("progressTitle", t.clone()));
    }
    if let Some(l) = &p.label {
        data.push(("progressValueString", l.clone()));
    }
    data
}

#[cfg(windows)]
pub use imp::*;

#[cfg(windows)]
mod imp {
    use anyhow::{Context as _, Result};
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::Foundation::{DateTime, IReference, PropertyValue};
    use windows::UI::Notifications::{
        NotificationData, NotificationSetting, NotificationUpdateResult, ToastNotification,
        ToastNotificationManager,
    };
    use windows::core::{HSTRING, Interface as _};

    use super::{AUMID, GROUP, tag};

    /// Seconds between 1601-01-01 (Windows epoch) and 1970-01-01.
    const EPOCH_DIFF: i64 = 11_644_473_600;

    /// The toast's bound values (its progress bar), as Windows keeps them for updates.
    fn notification_data(data: &[(&str, String)]) -> Result<NotificationData> {
        let d = NotificationData::new()?;
        let values = d.Values()?;
        for (k, v) in data {
            values.Insert(&HSTRING::from(*k), &HSTRING::from(v.as_str()))?;
        }
        // 0: always apply, whatever came before
        d.SetSequenceNumber(0)?;
        Ok(d)
    }

    pub fn show(xml: &str, id: &str, expires_at_unix: u64, data: &[(&str, String)]) -> Result<()> {
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
        if !data.is_empty() {
            toast.SetData(&notification_data(data)?)?;
        }
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID))?
            .Show(&toast)
            .context("ToastNotifier.Show")?;
        Ok(())
    }

    /// Change a shown toast's bound values in place: no new banner, no sound. `Ok(false)`: it's no
    /// longer in the notification center.
    pub fn update(id: &str, data: &[(&str, String)]) -> Result<bool> {
        let result = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID))?
            .UpdateWithTagAndGroup(
                &notification_data(data)?,
                &HSTRING::from(tag(id)),
                &HSTRING::from(GROUP),
            )
            .context("ToastNotifier.Update")?;
        Ok(result == NotificationUpdateResult::Succeeded)
    }

    /// Is the toast still on screen or in the notification center? One the user dismissed isn't,
    /// and neither is one that was clicked (Windows removes it before the click is handled).
    pub fn in_center(id: &str) -> Result<bool> {
        let history =
            ToastNotificationManager::History()?.GetHistoryWithId(&HSTRING::from(AUMID))?;
        let (want, group) = (HSTRING::from(tag(id)), HSTRING::from(GROUP));
        for i in 0..history.Size()? {
            let t = history.GetAt(i)?;
            if t.Tag()? == want && t.Group()? == group {
                return Ok(true);
            }
        }
        Ok(false)
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
    fn simple_toast_reports_its_click() {
        let s = Spec::parse(r#"{"title": "Backup <done>", "body": "42 GB & more"}"#).unwrap();
        let x = build_xml(&s, "id1", "tok", &Images::default());
        assert_eq!(
            x,
            r#"<toast launch="toastback://act?id=id1&amp;a=__body&amp;t=tok" activationType="protocol"><visual><binding template="ToastGeneric"><text>Backup &lt;done&gt;</text><text>42 GB &amp; more</text></binding></visual></toast>"#
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
    fn progress_bar_and_long_duration() {
        let s = Spec::parse(
            r#"{"title": "t", "duration": "long",
                "progress": {"value": 0.25, "status": "Copying", "label": "1 of 4"}}"#,
        )
        .unwrap();
        let x = build_xml(&s, "id1", "tok", &Images::default());
        assert!(x.contains(r#" duration="long""#), "{x}");
        // bound to the data, so it can be updated; no title binding, none was given
        assert!(x.contains(
            r#"<progress value="{progressValue}" status="{progressStatus}" valueStringOverride="{progressValueString}"/>"#
        ), "{x}");
        assert_eq!(
            progress_data(s.progress.as_ref().unwrap()),
            [
                ("progressValue", "0.25".to_string()),
                ("progressStatus", "Copying".to_string()),
                ("progressValueString", "1 of 4".to_string()),
            ]
        );
        // a short toast says nothing about its duration
        let plain = build_xml(
            &Spec::parse(r#"{"title": "t"}"#).unwrap(),
            "id1",
            "tok",
            &Images::default(),
        );
        assert!(!plain.contains("duration") && !plain.contains("<progress"));
        let s = Spec::parse(r#"{"title": "t", "progress": {"value": "indeterminate"}}"#).unwrap();
        assert_eq!(
            progress_data(s.progress.as_ref().unwrap()),
            [
                ("progressValue", "indeterminate".to_string()),
                ("progressStatus", String::new()),
            ]
        );
    }

    #[test]
    fn button_without_any_run_still_reports_its_click() {
        let s = Spec::parse(r#"{"title": "t", "buttons": [{"id": "ok", "label": "OK"}]}"#).unwrap();
        let x = build_xml(&s, "id1", "tok", &Images::default());
        assert!(x.contains(
            r#"<action content="OK" activationType="protocol" arguments="toastback://act?id=id1&amp;a=ok&amp;t=tok"/>"#
        ));
        assert!(!x.contains("system"));
    }
}
