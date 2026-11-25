use crate::Route;
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

#[cfg(feature = "server")]
mod storage;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct Ingredient {
    name: String,
    quantity: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct Step {
    instructions: String,
    note: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct Section<T> {
    heading: Option<String>,
    entries: Vec<T>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct RecipeContents {
    title: String,
    ingredients: Vec<Section<Ingredient>>,
    steps: Vec<Section<Step>>,
}

#[derive(Deserialize, Serialize)]
struct RecipeCard {
    id: i64,
    title: String,
}

#[derive(Deserialize, Serialize)]
struct Recipe {
    revision: i64,
    contents: RecipeContents,
}

#[cfg(feature = "server")]
fn require_member() -> Result<(), ServerFnError> {
    dioxus::fullstack::FullstackContext::current()
        .and_then(|context| context.extension::<crate::auth::Viewer>())
        .map(|_| ())
        .ok_or_else(|| ServerFnError::new("Sign in to continue."))
}

#[get("/api/recipes")]
async fn list_recipes() -> Result<Vec<RecipeCard>, ServerFnError> {
    require_member()?;
    crate::auth::server::run_blocking(|| {
        storage::list(&crate::auth::server::connection()?).map_err(crate::accounts::database_error)
    })
    .await
    .map_err(ServerFnError::new)
}

#[get("/api/recipes/:id")]
async fn get_recipe(id: i64) -> Result<Option<Recipe>, ServerFnError> {
    require_member()?;
    crate::auth::server::run_blocking(move || {
        storage::get(&mut crate::auth::server::connection()?, id)
            .map_err(crate::accounts::database_error)
    })
    .await
    .map_err(ServerFnError::new)
}

#[component]
pub(super) fn Recipes() -> Element {
    let recipes = use_server_future(list_recipes)?;
    rsx! {
        document::Title { "Lavemill · Recipes" }
        main { id: "main", class: "dashboard recipes-page", tabindex: "-1",
            h1 { "Recipes" }
            match recipes.read().as_ref() {
                Some(Ok(cards)) if cards.is_empty() => rsx! { p { "No recipes yet." } },
                Some(Ok(cards)) => rsx! {
                    ul { class: "recipe-cards",
                        for card in cards {
                            li { key: "{card.id}",
                                Link { class: "recipe-card", to: Route::RecipeDetail { id: card.id }, "{card.title}" }
                            }
                        }
                    }
                },
                Some(Err(_)) => rsx! { p { role: "alert", "Could not load recipes. Reload to try again." } },
                None => rsx! { p { "Loading recipes…" } },
            }
        }
    }
}

#[component]
pub(super) fn RecipeDetail(id: i64) -> Element {
    rsx! { RecipeDetailContent { key: "{id}", id } }
}

#[component]
fn RecipeDetailContent(id: i64) -> Element {
    let recipe = use_server_future(move || get_recipe(id))?;
    rsx! {
        main { id: "main", class: "dashboard recipes-page", tabindex: "-1",
            Link { to: Route::Recipes {}, "All recipes" }
            match recipe.read().as_ref() {
                Some(Ok(Some(recipe))) => rsx! {
                    document::Title { "Lavemill · {recipe.contents.title}" }
                    h1 { "{recipe.contents.title}" }
                    RecipeMethod { contents: recipe.contents.clone() }
                },
                Some(Ok(None)) => rsx! { h1 { "Recipe not found" } },
                Some(Err(_)) => rsx! { p { role: "alert", "Could not load this recipe. Reload to try again." } },
                None => rsx! { p { "Loading recipe…" } },
            }
        }
    }
}

#[component]
fn RecipeMethod(contents: RecipeContents) -> Element {
    let step_numbers = contents.steps.iter().scan(1, |start, section| {
        let current = *start;
        *start += section.entries.len();
        Some((current, section))
    });
    rsx! {
        div { class: "recipe-method",
            section {
                h2 { "Ingredients" }
                for section in &contents.ingredients {
                    if let Some(heading) = &section.heading { h3 { "{heading}" } }
                    ul {
                        for ingredient in &section.entries {
                            li {
                                "{ingredient.name}"
                                if let Some(quantity) = &ingredient.quantity {
                                    span { class: "row-note", " · {quantity}" }
                                }
                            }
                        }
                    }
                }
            }
            section {
                h2 { "Steps" }
                for (start, section) in step_numbers {
                    if let Some(heading) = &section.heading { h3 { "{heading}" } }
                    ol { start: start as i64,
                        for step in &section.entries {
                            li {
                                div { class: "recipe-text", "{step.instructions}" }
                                if let Some(note) = &step.note {
                                    div { class: "row-note recipe-text", "{note}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
