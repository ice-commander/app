use adw::prelude::*;
use std::time::Duration;

const KINDS: [(&str, &str); 4] = [
    ("Suggest a feature", "feedback.type_feature"),
    ("Report a bug", "feedback.type_bug"),
    ("Usage question", "feedback.type_question"),
    ("Other", "feedback.type_other"),
];

pub fn client() -> String {
    format!(
        "Ice Commander {} {}/{}",
        common::version::APP_VERSION,
        common::version::APP_TYPE,
        common::version::BUILD_TYPE
    )
}

#[derive(Debug, PartialEq, Eq)]
pub enum Invalid {
    Missing,
    Email,
}

pub fn validate(name: &str, email: &str, message: &str) -> Result<(), Invalid> {
    if name.trim().is_empty() || email.trim().is_empty() || message.trim().is_empty() {
        return Err(Invalid::Missing);
    }
    if !looks_like_email(email.trim()) {
        return Err(Invalid::Email);
    }
    Ok(())
}

fn looks_like_email(s: &str) -> bool {
    let Some((local, domain)) = s.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !s.contains(char::is_whitespace)
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
}

pub fn payload(
    name: &str,
    email: &str,
    kind: usize,
    message: &str,
    platform: &str,
) -> serde_json::Value {
    serde_json::json!({
        "name": name.trim(),
        "email": email.trim(),
        "type": KINDS.get(kind).unwrap_or(&KINDS[0]).0,
        "message": message.trim(),
        "client": client(),
        "platform": platform,
    })
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Sent,
    TooSoon,
    Failed(String),
}

pub fn outcome(status: u16, body: &str) -> Outcome {
    if (200..300).contains(&status) {
        return Outcome::Sent;
    }
    if status == 429 {
        return Outcome::TooSoon;
    }
    let said = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(str::to_string))
        .filter(|m| !m.is_empty());
    Outcome::Failed(said.unwrap_or_else(|| format!("HTTP {status}")))
}

async fn post(server: String, body: serde_json::Value) -> Result<(u16, String), String> {
    let resp = reqwest::Client::new()
        .post(format!("{server}/api/feedback"))
        .timeout(Duration::from_secs(20))
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    Ok((status, resp.text().await.unwrap_or_default()))
}

fn tr(key: &str) -> String {
    crate::i18n::tr(key).to_string()
}

pub fn show(parent: &impl IsA<gtk::Widget>) {
    let platform = ic_plugin_host::updates::platform();

    let intro = gtk::Label::builder()
        .label(tr("feedback.intro"))
        .wrap(true)
        .xalign(0.0)
        .build();
    intro.add_css_class("dim-label");

    let name = adw::EntryRow::builder().title(tr("feedback.name")).build();
    let email = adw::EntryRow::builder()
        .title(tr("feedback.email"))
        .input_purpose(gtk::InputPurpose::Email)
        .build();
    let labels: Vec<String> = KINDS.iter().map(|(_, key)| tr(key)).collect();
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let kind = adw::ComboRow::builder()
        .title(tr("feedback.type"))
        .model(&gtk::StringList::new(&refs))
        .build();

    let fields = adw::PreferencesGroup::new();
    fields.add(&name);
    fields.add(&email);
    fields.add(&kind);

    let message_title = gtk::Label::builder()
        .label(tr("feedback.message"))
        .xalign(0.0)
        .build();
    message_title.add_css_class("heading");
    let message = gtk::TextView::builder()
        .wrap_mode(gtk::WrapMode::WordChar)
        .top_margin(10)
        .bottom_margin(10)
        .left_margin(12)
        .right_margin(12)
        .accepts_tab(false)
        .build();
    let message_frame = gtk::ScrolledWindow::builder()
        .child(&message)
        .min_content_height(150)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();
    message_frame.add_css_class("card");

    let me = client();
    let sent_along = gtk::Label::builder()
        .label(crate::i18n::trf(
            "feedback.sent_along",
            &[("client", me.as_str()), ("platform", platform.as_str())],
        ))
        .wrap(true)
        .xalign(0.0)
        .build();
    sent_along.add_css_class("dim-label");
    sent_along.add_css_class("caption");

    let error = gtk::Label::builder()
        .wrap(true)
        .xalign(0.0)
        .visible(false)
        .build();
    error.add_css_class("error");

    let cancel = gtk::Button::with_label(&tr("common.cancel"));
    let send = gtk::Button::with_label(&tr("feedback.send"));
    send.add_css_class("suggested-action");
    let buttons = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::End)
        .margin_top(6)
        .build();
    buttons.append(&cancel);
    buttons.append(&send);

    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(6)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();
    body.append(&intro);
    body.append(&fields);
    body.append(&message_title);
    body.append(&message_frame);
    body.append(&sent_along);
    body.append(&error);
    body.append(&buttons);

    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    view.set_content(Some(&body));

    let dialog = adw::Dialog::builder()
        .title(tr("feedback.title"))
        .content_width(480)
        .child(&view)
        .build();

    let d = dialog.clone();
    cancel.connect_clicked(move |_| {
        d.close();
    });

    let first = name.clone();
    let d = dialog.clone();
    let parent_widget: gtk::Widget = parent.clone().upcast();
    send.connect_clicked(move |btn| {
        let buf = message.buffer();
        let text = buf.text(&buf.start_iter(), &buf.end_iter(), false).to_string();
        let (n, e) = (name.text().to_string(), email.text().to_string());

        if let Err(why) = validate(&n, &e, &text) {
            error.set_text(&tr(match why {
                Invalid::Missing => "feedback.required",
                Invalid::Email => "feedback.bad_email",
            }));
            error.set_visible(true);
            return;
        }

        let body = payload(&n, &e, kind.selected() as usize, &text, &platform);
        error.set_visible(false);
        btn.set_sensitive(false);
        btn.set_label(&tr("feedback.sending"));
        fields.set_sensitive(false);
        message.set_sensitive(false);

        let rx = crate::updater::spawn_on_tokio(post(crate::updater::update_server(), body));
        let (btn, error, fields, message, d, parent_widget) = (
            btn.clone(),
            error.clone(),
            fields.clone(),
            message.clone(),
            d.clone(),
            parent_widget.clone(),
        );
        gtk::glib::spawn_future_local(async move {
            let result = match rx.await {
                Ok(Ok((status, text))) => outcome(status, &text),
                Ok(Err(net)) => Outcome::Failed(net),
                Err(_) => Outcome::Failed(tr("feedback.interrupted")),
            };
            match result {
                Outcome::Sent => {
                    d.close();
                    thank(&parent_widget);
                }
                failed => {
                    error.set_text(&match failed {
                        Outcome::TooSoon => tr("feedback.too_soon"),
                        Outcome::Failed(why) => {
                            crate::i18n::trf("feedback.failed", &[("error", why.as_str())])
                        }
                        Outcome::Sent => unreachable!(),
                    });
                    error.set_visible(true);
                    btn.set_label(&tr("feedback.send"));
                    btn.set_sensitive(true);
                    fields.set_sensitive(true);
                    message.set_sensitive(true);
                }
            }
        });
    });

    dialog.present(Some(parent));
    first.grab_focus();
}

fn thank(parent: &gtk::Widget) {
    let done = adw::AlertDialog::builder()
        .heading(tr("feedback.thanks_title"))
        .body(tr("feedback.thanks_body"))
        .build();
    done.add_response("ok", &tr("common.ok"));
    done.set_default_response(Some("ok"));
    done.present(Some(parent));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_payload_carries_exactly_what_the_site_form_sends_plus_where_it_came_from() {
        let p = payload("Gordon", "g@example.com", 1, "Protocol list is blank", "windows-x86_64");
        let keys: std::collections::BTreeSet<&str> =
            p.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            ["client", "email", "message", "name", "platform", "type"].into_iter().collect()
        );
        assert_eq!(p["platform"], "windows-x86_64");
        assert!(p["client"].as_str().unwrap().starts_with("Ice Commander "));
    }

    #[test]
    fn the_payload_fits_the_type_the_server_parses_it_into() {
        let p = payload("Gordon", "g@example.com", 1, "Protocol list is blank", "windows-x86_64");
        let parsed: Result<std::collections::HashMap<String, String>, _> = serde_json::from_value(p);
        assert!(
            parsed.is_ok(),
            "the site reads the body as HashMap<String, String>; a non-string value is a 400"
        );
    }

    #[test]
    fn the_type_goes_out_in_the_sites_english_wording_whatever_the_ui_language() {
        let sent: Vec<String> = (0..4)
            .map(|i| payload("a", "a@b.c", i, "m", "p")["type"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(sent, ["Suggest a feature", "Report a bug", "Usage question", "Other"]);
    }

    #[test]
    fn an_out_of_range_choice_falls_back_to_the_first_type_instead_of_panicking() {
        assert_eq!(payload("a", "a@b.c", 99, "m", "p")["type"], "Suggest a feature");
    }

    #[test]
    fn no_field_is_named_source_so_the_servers_own_one_is_not_doubled() {
        assert!(payload("a", "a@b.c", 0, "m", "p").get("source").is_none());
        assert!(payload("a", "a@b.c", 0, "m", "p").get("ip").is_none());
    }

    #[test]
    fn surrounding_whitespace_is_trimmed_before_sending() {
        let p = payload("  Gordon \n", " g@example.com ", 0, "\n  hello  \n", "p");
        assert_eq!(p["name"], "Gordon");
        assert_eq!(p["email"], "g@example.com");
        assert_eq!(p["message"], "hello");
    }

    #[test]
    fn name_email_and_message_are_all_required_like_on_the_site() {
        assert_eq!(validate("", "a@b.c", "m"), Err(Invalid::Missing));
        assert_eq!(validate("a", "", "m"), Err(Invalid::Missing));
        assert_eq!(validate("a", "a@b.c", "   \n "), Err(Invalid::Missing));
        assert_eq!(validate("a", "a@b.c", "m"), Ok(()));
    }

    #[test]
    fn an_obviously_broken_email_is_caught_before_it_is_sent() {
        for bad in ["gordon", "gordon@", "@example.com", "g@example", "g@.com", "g @x.com"] {
            assert_eq!(validate("a", bad, "m"), Err(Invalid::Email), "{bad} passed");
        }
        assert_eq!(validate("a", "g.k+tag@mail.example.co.uk", "m"), Ok(()));
    }

    #[test]
    fn a_2xx_answer_means_the_message_went_through() {
        assert_eq!(outcome(200, r#"{"status":"success"}"#), Outcome::Sent);
        assert_eq!(outcome(204, ""), Outcome::Sent);
    }

    #[test]
    fn the_servers_five_minute_limit_is_told_apart_from_a_real_failure() {
        assert_eq!(
            outcome(429, r#"{"status":"error","message":"Cannot be sent, you have just sent a message"}"#),
            Outcome::TooSoon
        );
    }

    #[test]
    fn a_failure_shows_the_servers_own_reason_when_it_gives_one() {
        assert_eq!(
            outcome(500, r#"{"status":"error","message":"Telegram bot error: down"}"#),
            Outcome::Failed("Telegram bot error: down".into())
        );
    }

    #[test]
    fn a_failure_without_a_readable_reason_falls_back_to_the_status() {
        assert_eq!(outcome(502, "<html>Bad Gateway</html>"), Outcome::Failed("HTTP 502".into()));
        assert_eq!(outcome(500, r#"{"message":""}"#), Outcome::Failed("HTTP 500".into()));
    }

    fn serve_once(status: u16, reply: &'static str) -> (String, std::sync::mpsc::Receiver<String>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut got = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = sock.read(&mut chunk).unwrap();
                if n == 0 {
                    break;
                }
                got.extend_from_slice(&chunk[..n]);
                let text = String::from_utf8_lossy(&got).to_string();
                if let Some((head, body)) = text.split_once("\r\n\r\n") {
                    let len = head
                        .lines()
                        .find_map(|l| {
                            let (k, v) = l.split_once(':')?;
                            k.eq_ignore_ascii_case("content-length").then(|| v.trim().parse().ok())?
                        })
                        .unwrap_or(0usize);
                    if body.len() >= len {
                        break;
                    }
                }
            }
            tx.send(String::from_utf8_lossy(&got).into_owned()).unwrap();
            let answer = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                reply.len()
            );
            sock.write_all(answer.as_bytes()).unwrap();
        });
        (format!("http://{addr}"), rx)
    }

    #[tokio::test]
    async fn the_message_reaches_the_sites_feedback_route_as_json() {
        let (server, seen) = serve_once(200, r#"{"status":"success"}"#);
        let body = payload("Gordon", "g@example.com", 1, "Protocol list is blank", "windows-x86_64");
        let (status, text) = post(server, body).await.unwrap();
        assert_eq!(outcome(status, &text), Outcome::Sent);

        let request = seen.recv().unwrap();
        let (head, json) = request.split_once("\r\n\r\n").unwrap();
        assert!(head.starts_with("POST /api/feedback HTTP/1.1"), "went to: {}", head.lines().next().unwrap());
        assert!(
            head.lines().any(|l| l.to_ascii_lowercase() == "content-type: application/json"),
            "the site parses JSON, so the request must say so:\n{head}"
        );
        let sent: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(sent["name"], "Gordon");
        assert_eq!(sent["type"], "Report a bug");
        assert_eq!(sent["platform"], "windows-x86_64");
        assert!(sent["client"].as_str().unwrap().starts_with("Ice Commander "));
    }

    #[tokio::test]
    async fn the_servers_rate_limit_comes_back_as_too_soon() {
        let (server, _seen) = serve_once(
            429,
            r#"{"status":"error","message":"Cannot be sent, you have just sent a message"}"#,
        );
        let (status, text) = post(server, payload("a", "a@b.c", 0, "m", "p")).await.unwrap();
        assert_eq!(outcome(status, &text), Outcome::TooSoon);
    }

    #[tokio::test]
    async fn a_server_that_is_not_there_is_a_failure_and_not_a_hang() {
        let closed = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap()
        };
        let started = std::time::Instant::now();
        let got = post(format!("http://{closed}"), payload("a", "a@b.c", 0, "m", "p")).await;
        assert!(got.is_err());
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    fn used_keys() -> std::collections::BTreeSet<String> {
        let marker = ['"', 'f', 'e', 'e', 'd', 'b', 'a', 'c', 'k', '.'].iter().collect::<String>();
        let production = include_str!("feedback.rs").split("#[cfg(test)]").next().unwrap();
        production
            .split(marker.as_str())
            .skip(1)
            .filter_map(|rest| {
                let key: String =
                    rest.chars().take_while(|c| c.is_ascii_lowercase() || *c == '_').collect();
                (!key.is_empty() && rest[key.len()..].starts_with('"')).then(|| format!("feedback.{key}"))
            })
            .collect()
    }

    fn placeholders(s: &str) -> std::collections::BTreeSet<String> {
        s.split("%{").skip(1).filter_map(|r| r.split_once('}').map(|(p, _)| p.to_string())).collect()
    }

    #[test]
    fn every_string_the_dialog_uses_exists_in_all_fifteen_languages() {
        let locales: [(&str, &str); 15] = [
            ("en", include_str!("../locales/en.json")),
            ("ru", include_str!("../locales/ru.json")),
            ("pl", include_str!("../locales/pl.json")),
            ("cs", include_str!("../locales/cs.json")),
            ("sk", include_str!("../locales/sk.json")),
            ("de", include_str!("../locales/de.json")),
            ("es", include_str!("../locales/es.json")),
            ("uk", include_str!("../locales/uk.json")),
            ("it", include_str!("../locales/it.json")),
            ("fr", include_str!("../locales/fr.json")),
            ("ro", include_str!("../locales/ro.json")),
            ("hu", include_str!("../locales/hu.json")),
            ("be", include_str!("../locales/be.json")),
            ("bg", include_str!("../locales/bg.json")),
            ("sr", include_str!("../locales/sr.json")),
        ];
        let keys = used_keys();
        assert!(keys.len() >= 15, "the scan found only {keys:?}");
        let en: std::collections::HashMap<String, String> = serde_json::from_str(locales[0].1).unwrap();
        for (lang, raw) in locales {
            let map: std::collections::HashMap<String, String> = serde_json::from_str(raw).unwrap();
            for key in &keys {
                let text = map.get(key).unwrap_or_else(|| panic!("{lang}.json is missing {key}"));
                assert!(!text.trim().is_empty(), "{lang}.json has an empty {key}");
                assert_eq!(
                    placeholders(text),
                    placeholders(&en[key]),
                    "{lang}.json {key} does not carry the same %{{…}} slots as English"
                );
            }
        }
    }
}
