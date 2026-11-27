use crate::{theme::use_theme_preview, Route};
use dioxus::prelude::*;

#[component]
pub(crate) fn HomeCard(
    to: Route,
    label: &'static str,
    page: &'static str,
    heading: &'static str,
    #[props(default)] scrollable: bool,
    children: Element,
) -> Element {
    let navigator = use_navigator();
    let destination = to.clone();
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
            Link {
                class: "card-hit-area",
                to,
                aria_label: label,
                "data-card-page": page,
                onclick_only: true,
                onclick: move |_| {
                    let destination = destination.clone();
                    spawn(async move {
                        if begin_transition(page, false).await {
                            navigator.push(destination);
                        }
                    });
                },
            }
            {children}
        }
    }
}

#[component]
pub(crate) fn ThemePreviewTile(to: Route, children: Element) -> Element {
    let preview = use_theme_preview(&to);
    rsx! {
        span {
            class: "theme-preview-tile",
            onpointerenter: move |event| {
                if event.pointer_type() != "touch" {
                    preview();
                }
            },
            onfocusin: move |_| preview(),
            {children}
        }
    }
}

#[component]
pub(crate) fn UsersLink() -> Element {
    let navigator = use_navigator();
    let opening_users = use_route::<Route>() != (Route::Users {});
    rsx! {
        ThemePreviewTile { to: Route::Users {},
            Link {
                class: "account-icon",
                to: Route::Users {},
                aria_label: "Users",
                title: "Users",
                "data-home-return-anchor": "users",
                onclick_only: opening_users,
                onclick: move |_| {
                    if opening_users {
                        spawn(async move {
                            if begin_transition("users", false).await {
                                navigator.push(Route::Users {});
                            }
                        });
                    }
                },
                crate::Icon { kind: "users" }
            }
        }
    }
}

#[component]
pub(crate) fn HomeLink() -> Element {
    let navigator = use_navigator();
    let page = match use_route::<Route>() {
        Route::Todos {} => Some("todos"),
        Route::Shopping {} => Some("shopping"),
        Route::Calendar { .. } => Some("calendar"),
        Route::Recipes {}
        | Route::NewRecipe {}
        | Route::RecipeDetail { .. }
        | Route::EditRecipe { .. } => Some("recipes"),
        Route::Users {} => Some("users"),
        Route::Home {} | Route::Login { .. } => None,
    };
    rsx! {
        Link {
            class: "header-tile brand",
            to: Route::Home {},
            onclick_only: page.is_some(),
            onclick: move |_| {
                if let Some(page) = page {
                    spawn(async move {
                        if begin_transition(page, true).await {
                            navigator.push(Route::Home {});
                        }
                    });
                }
            },
            crate::Icon { kind: "home" }
            span { "lavemill" }
        }
    }
}

async fn begin_transition(page: &str, returning_home: bool) -> bool {
    let mut transition = document::eval(include_str!("../assets/card-navigation.js"));
    if transition.send((page, returning_home)).is_err() {
        return true;
    }
    transition.recv::<bool>().await.unwrap_or(true)
}
