use crate::Route;
use dioxus::prelude::*;

#[component]
pub(crate) fn HomeCardLink(to: Route, label: &'static str, page: &'static str) -> Element {
    let navigator = use_navigator();
    let destination = to.clone();
    rsx! {
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
