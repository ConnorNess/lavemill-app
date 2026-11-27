use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
struct Shortcut {
    service: Service,
    url: String,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
enum Service {
    Seerr,
    Plex,
    Komga,
    Foundry,
    BookStack,
}

impl Service {
    fn name(self) -> &'static str {
        match self {
            Self::Seerr => "Seerr",
            Self::Plex => "Plex",
            Self::Komga => "Komga",
            Self::Foundry => "Foundry",
            Self::BookStack => "BookStack",
        }
    }
}

#[get("/api/homelab/shortcuts")]
async fn shortcuts() -> Result<Vec<Shortcut>, ServerFnError> {
    crate::auth::require_member()?;

    Ok([
        (Service::Seerr, "LAVEMILL_SEERR_URL"),
        (Service::Plex, "LAVEMILL_PLEX_URL"),
        (Service::Komga, "LAVEMILL_KOMGA_URL"),
        (Service::Foundry, "LAVEMILL_FOUNDRY_URL"),
        (Service::BookStack, "LAVEMILL_BOOKSTACK_URL"),
    ]
    .into_iter()
    .filter_map(|(service, variable)| {
        let value = std::env::var(variable).ok()?;
        let url = shortcut_url(&value)?;
        Some(Shortcut { service, url })
    })
    .collect())
}

#[cfg(feature = "server")]
fn shortcut_url(value: &str) -> Option<String> {
    let url = url::Url::parse(value.trim()).ok()?;
    if matches!(url.scheme(), "http" | "https") && url.host().is_some() {
        Some(url.into())
    } else {
        None
    }
}

#[component]
pub fn Shortcuts() -> Element {
    let shortcuts = use_server_future(shortcuts)?;
    let shortcuts = shortcuts.read();
    let Some(Ok(links)) = shortcuts.as_ref() else {
        return rsx! {};
    };
    if links.is_empty() {
        return rsx! {};
    }

    rsx! {
        nav { class: "homelab-shortcuts", aria_label: "Homelab",
            div { class: "homelab-shortcut-rail",
                for link in links {
                    a {
                        key: "{link.service.name()}",
                        class: "homelab-shortcut",
                        href: "{link.url}",
                        target: "_blank",
                        rel: "noopener noreferrer",
                        span { class: "card-icon", crate::Icon { kind: link.service.name() } }
                        span { "{link.service.name()}" }
                        span { class: "sr-only", " (opens in a new tab)" }
                    }
                }
            }
        }
    }
}

#[cfg(all(test, feature = "server"))]
mod tests {
    use super::shortcut_url;

    #[test]
    fn shortcuts_accept_web_urls_and_skip_unusable_destinations() {
        assert_eq!(
            shortcut_url(" https://plex.example.com/web "),
            Some("https://plex.example.com/web".into())
        );
        assert_eq!(
            shortcut_url("http://192.168.1.20:5055"),
            Some("http://192.168.1.20:5055/".into())
        );
        for invalid in [
            "",
            "plex.example.com",
            "/web",
            "https://",
            "javascript:alert(1)",
        ] {
            assert_eq!(shortcut_url(invalid), None);
        }
    }
}
