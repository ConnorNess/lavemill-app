use crate::{
    theme::{use_header_theme_preview, use_theme_preview, PreviewSource},
    Route,
};
use dioxus::prelude::*;
use dioxus::router::Navigator;

#[component]
pub(crate) fn HomeCard(
    to: Route,
    label: &'static str,
    heading: &'static str,
    #[props(default)] scrollable: bool,
    children: Element,
) -> Element {
    let preview = use_theme_preview(&to);
    rsx! {
        section {
            class: if scrollable { "dashboard-card dashboard-list-card" } else { "dashboard-card" },
            tabindex: scrollable.then_some("0"),
            aria_labelledby: heading,
            onpointerenter: move |event| {
                if event.pointer_type() != "touch" {
                    preview();
                }
            },
            onfocusin: move |_| preview(),
            div { class: "home-card-content",
                NavigationLink {
                    class: "card-hit-area",
                    to,
                    aria_label: label,
                    anchor: true,
                }
                {children}
            }
        }
    }
}

#[component]
pub(crate) fn ThemePreviewTile(to: Route, children: Element) -> Element {
    let preview = use_header_theme_preview(&to);
    rsx! {
        span {
            class: "theme-preview-tile",
            onpointerenter: move |event| {
                if event.pointer_type() != "touch" {
                    preview.call((PreviewSource::Pointer, true));
                }
            },
            onpointerleave: move |event| {
                if event.pointer_type() != "touch" {
                    preview.call((PreviewSource::Pointer, false));
                }
            },
            onfocusin: move |_| preview.call((PreviewSource::Focus, true)),
            onfocusout: move |_| preview.call((PreviewSource::Focus, false)),
            {children}
        }
    }
}

#[component]
pub(crate) fn UsersLink() -> Element {
    rsx! {
        ThemePreviewTile { to: Route::Users {},
            NavigationLink {
                class: "account-icon",
                to: Route::Users {},
                aria_label: "Users",
                title: "Users",
                anchor: true,
                crate::Icon { kind: "users" }
            }
        }
    }
}

#[component]
pub(crate) fn RecipesLink() -> Element {
    rsx! {
        ThemePreviewTile { to: Route::Recipes {},
            NavigationLink {
                class: "header-tile recipes-link",
                to: Route::Recipes {},
                anchor: true,
                crate::Icon { kind: "recipes" }
                span { "Recipes" }
            }
        }
    }
}

#[component]
pub(crate) fn HomeLink() -> Element {
    rsx! {
        NavigationLink {
            class: "header-tile brand",
            to: Route::Home {},
            crate::Icon { kind: "home" }
            span { "lavemill" }
        }
    }
}

#[component]
pub(crate) fn NavigationLink(
    to: Route,
    class: Option<&'static str>,
    aria_label: Option<&'static str>,
    title: Option<&'static str>,
    #[props(default)] anchor: bool,
    children: Element,
) -> Element {
    let navigator = use_navigator();
    let current = use_route::<Route>() == to;
    let destination = to.clone();
    let path = to.to_string();
    let anchor_path = anchor.then(|| path.split_once('?').map_or(path.as_str(), |(path, _)| path));
    rsx! {
        Link {
            class: class.map(str::to_owned),
            aria_label,
            title,
            "data-navigation-anchor": anchor_path,
            to,
            onclick_only: true,
            onclick: move |_| {
                if !current {
                    let destination = destination.clone();
                    spawn(async move {
                        if begin_transition(&destination, false).await {
                            navigator.push(destination);
                        }
                    });
                }
            },
            {children}
        }
    }
}

pub(crate) async fn replace_page(navigator: Navigator, to: Route) {
    if begin_transition(&to, true).await {
        navigator.replace(to);
    }
}

async fn begin_transition(destination: &Route, replacing: bool) -> bool {
    let mut transition = document::eval(
        r#"
        const [path, replacing] = await dioxus.recv();
        if (window.lavemillNavigate) {
            window.lavemillNavigate(path, () => dioxus.send(true), { replacing, blocked: () => dioxus.send(false) });
        } else {
            dioxus.send(true);
        }
        "#,
    );
    if transition
        .send((destination.to_string(), replacing))
        .is_err()
    {
        return true;
    }
    transition.recv::<bool>().await.unwrap_or(true)
}
