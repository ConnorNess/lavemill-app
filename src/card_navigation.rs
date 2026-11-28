use crate::{
    theme::{use_header_theme_preview, use_theme_preview, PreviewSource},
    Route,
};
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
                        if begin_transition(page, false, None).await {
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
                            if begin_transition("users", false, None).await {
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
pub(crate) fn RecipesLink() -> Element {
    let navigator = use_navigator();
    let opening_recipes = use_route::<Route>() != (Route::Recipes {});
    rsx! {
        ThemePreviewTile { to: Route::Recipes {},
            Link {
                class: "header-tile recipes-link",
                to: Route::Recipes {},
                "data-home-return-anchor": "recipes",
                onclick_only: opening_recipes,
                onclick: move |_| {
                    if opening_recipes {
                        spawn(async move {
                            if begin_transition("recipes", false, None).await {
                                navigator.push(Route::Recipes {});
                            }
                        });
                    }
                },
                crate::Icon { kind: "recipes" }
                span { "Recipes" }
            }
        }
    }
}

#[component]
pub(crate) fn RecipeCardLink(id: i64, title: String) -> Element {
    let navigator = use_navigator();
    rsx! {
        Link {
            class: "recipe-card surface-panel",
            to: Route::RecipeDetail { id },
            "data-recipe-card-id": "{id}",
            onclick_only: true,
            onclick: move |_| {
                spawn(async move {
                    if begin_transition("recipes", false, Some(id)).await {
                        navigator.push(Route::RecipeDetail { id });
                    }
                });
            },
            "{title}"
        }
    }
}

#[component]
pub(crate) fn EditRecipeLink(id: i64) -> Element {
    let navigator = use_navigator();
    rsx! {
        Link {
            class: "recipe-action recipe-control secondary-button",
            to: Route::EditRecipe { id },
            "data-recipe-edit-link-id": "{id}",
            onclick_only: true,
            onclick: move |_| {
                spawn(async move {
                    if begin_transition("recipe-edit", false, Some(id)).await {
                        navigator.push(Route::EditRecipe { id });
                    }
                });
            },
            crate::Icon { kind: "pencil" }
            "Edit recipe"
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
                        if begin_transition(page, true, None).await {
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

async fn begin_transition(page: &str, returning_home: bool, recipe_id: Option<i64>) -> bool {
    let mut transition = document::eval(include_str!("../assets/card-navigation.js"));
    let recipe_id = recipe_id.map(|id| id.to_string());
    if transition.send((page, returning_home, recipe_id)).is_err() {
        return true;
    }
    transition.recv::<bool>().await.unwrap_or(true)
}
