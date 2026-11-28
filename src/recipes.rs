use crate::Route;
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

mod editor;
pub(super) use editor::{EditRecipe, NewRecipe};
mod shopping_picker;

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

#[derive(Deserialize, Serialize)]
enum SaveRecipe {
    Saved(i64),
    Invalid(String),
    Conflict,
}

type IngredientPosition = (usize, usize);

#[derive(Debug, Deserialize, PartialEq, Serialize)]
enum AddRecipeIngredients {
    Added { added: usize, skipped: usize },
    InvalidSelection,
    StaleRecipe,
}

#[post("/api/recipes/:id/shopping")]
async fn add_recipe_ingredients(
    id: i64,
    revision: i64,
    positions: Vec<IngredientPosition>,
) -> Result<AddRecipeIngredients, ServerFnError> {
    let actor = crate::auth::require_member()?.id;
    crate::auth::server::run_blocking(move || {
        storage::add_to_shopping(
            &mut crate::auth::server::connection()?,
            actor,
            id,
            revision,
            positions,
        )
        .map_err(crate::accounts::database_error)
    })
    .await
    .map_err(ServerFnError::new)
}

#[get("/api/recipes")]
async fn list_recipes() -> Result<Vec<RecipeCard>, ServerFnError> {
    crate::auth::require_member()?;
    crate::auth::server::run_blocking(|| {
        storage::list(&crate::auth::server::connection()?).map_err(crate::accounts::database_error)
    })
    .await
    .map_err(ServerFnError::new)
}

#[get("/api/recipes/:id")]
async fn get_recipe(id: i64) -> Result<Option<Recipe>, ServerFnError> {
    crate::auth::require_member()?;
    crate::auth::server::run_blocking(move || {
        storage::get(&mut crate::auth::server::connection()?, id)
            .map_err(crate::accounts::database_error)
    })
    .await
    .map_err(ServerFnError::new)
}

#[post("/api/recipes/create")]
async fn create_recipe(contents: RecipeContents) -> Result<SaveRecipe, ServerFnError> {
    crate::auth::require_member()?;
    save_recipe(contents, storage::create).await
}

#[post("/api/recipes/:id/edit")]
async fn update_recipe(
    id: i64,
    revision: i64,
    contents: RecipeContents,
) -> Result<SaveRecipe, ServerFnError> {
    crate::auth::require_member()?;
    save_recipe(contents, move |db, contents| {
        storage::update(db, id, revision, contents)
    })
    .await
}

#[post("/api/recipes/:id/delete")]
async fn delete_recipe(id: i64, revision: i64, title: String) -> Result<bool, ServerFnError> {
    crate::auth::require_member()?;
    crate::auth::server::run_blocking(move || {
        storage::delete(&crate::auth::server::connection()?, id, revision, &title)
            .map_err(crate::accounts::database_error)
    })
    .await
    .map_err(ServerFnError::new)
}

#[cfg(feature = "server")]
async fn save_recipe(
    contents: RecipeContents,
    write: impl FnOnce(&mut rusqlite::Connection, &RecipeContents) -> rusqlite::Result<SaveRecipe>
        + Send
        + 'static,
) -> Result<SaveRecipe, ServerFnError> {
    let contents = match storage::validate(contents) {
        Ok(contents) => contents,
        Err(message) => return Ok(SaveRecipe::Invalid(message.into())),
    };
    crate::auth::server::run_blocking(move || {
        write(&mut crate::auth::server::connection()?, &contents)
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
        main { id: "main", class: "dashboard interior-page recipes-page", tabindex: "-1", "data-recipes-index": "",
            header { class: "recipe-heading",
                h1 { "Recipes" }
                Link { class: "recipe-action recipe-control primary-button", to: Route::NewRecipe {}, "Add recipe" }
            }
            match recipes.read().as_ref() {
                Some(Ok(cards)) if cards.is_empty() => rsx! { p { "No recipes yet." } },
                Some(Ok(cards)) => rsx! {
                    ul { class: "recipe-cards",
                        for card in cards {
                            li { key: "{card.id}",
                                crate::card_navigation::RecipeCardLink { id: card.id, title: card.title.clone() }
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
        main { id: "main", class: "dashboard interior-page recipes-page", tabindex: "-1", "data-recipe-detail-id": "{id}",
            Link { class: "recipe-back-link", to: Route::Recipes {}, "All recipes" }
            match recipe.read().as_ref() {
                Some(Ok(Some(recipe))) => rsx! {
                    document::Title { "Lavemill · {recipe.contents.title}" }
                    header { class: "recipe-heading",
                        h1 { "{recipe.contents.title}" }
                        div { class: "recipe-detail-actions",
                            Link { class: "recipe-action recipe-control secondary-button", to: Route::EditRecipe { id },
                                crate::Icon { kind: "pencil" }
                                "Edit recipe"
                            }
                            DeleteRecipe { id, revision: recipe.revision, title: recipe.contents.title.clone() }
                        }
                    }
                    RecipeMethod { id, revision: recipe.revision, contents: recipe.contents.clone() }
                },
                Some(Ok(None)) => rsx! { h1 { "Recipe not found" } },
                Some(Err(_)) => rsx! { p { role: "alert", "Could not load this recipe. Reload to try again." } },
                None => rsx! { p { "Loading recipe…" } },
            }
        }
    }
}

#[component]
fn DeleteRecipe(id: i64, revision: i64, title: String) -> Element {
    let navigator = use_navigator();
    let mut confirmation = use_signal(String::new);
    let mut pending = use_signal(|| false);
    let mut stale = use_signal(|| false);
    let mut error = use_signal(|| "");
    let matches_title = *confirmation.read() == title;

    rsx! {
        button {
            r#type: "button",
            class: "recipe-action recipe-control secondary-button",
            onclick: move |_| {
                confirmation.set(String::new());
                if !stale() {
                    error.set("");
                }
                let _ = document::eval(r#"
                    const dialog = document.getElementById("recipe-delete-dialog");
                    dialog.oncancel = (event) => {
                        if (dialog.querySelector("fieldset").disabled) event.preventDefault();
                    };
                    dialog.showModal();
                "#);
            },
            "Delete recipe"
        }
        dialog { id: "recipe-delete-dialog", class: "recipe-delete-dialog surface-panel",
            aria_labelledby: "recipe-delete-title",
            h2 { id: "recipe-delete-title", "Delete recipe?" }
            p { "This permanently deletes the recipe, including its ingredients and steps." }
            form {
                onsubmit: move |event| {
                    event.prevent_default();
                    if !matches_title || pending() || stale() {
                        return;
                    }
                    let typed_title = confirmation();
                    pending.set(true);
                    error.set("");
                    spawn(async move {
                        match delete_recipe(id, revision, typed_title).await {
                            Ok(true) => { navigator.replace(Route::Recipes {}); }
                            Ok(false) => {
                                stale.set(true);
                                error.set("This recipe changed or is unavailable. Reload the page to continue.");
                            }
                            Err(_) => error.set("Could not delete the recipe. Try again."),
                        }
                        pending.set(false);
                    });
                },
                fieldset { disabled: pending(),
                    label {
                        "Type “{title}” to confirm"
                        input {
                            r#type: "text", autocomplete: "off", spellcheck: false,
                            value: "{confirmation}", disabled: stale(),
                            oninput: move |event| confirmation.set(event.value()),
                        }
                    }
                    if !error().is_empty() {
                        p { class: "form-error", role: "alert", "{error}" }
                    }
                    div { class: "recipe-detail-actions recipe-delete-actions",
                        button {
                            r#type: "button", class: "secondary-button",
                            onclick: move |_| { let _ = document::eval(r#"document.getElementById("recipe-delete-dialog").close();"#); },
                            "Cancel"
                        }
                        if matches_title && !stale() {
                            button { r#type: "submit", class: "primary-button",
                                if pending() { "Deleting…" } else { "Delete recipe" }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn RecipeMethod(id: i64, revision: i64, contents: RecipeContents) -> Element {
    let step_numbers = contents.steps.iter().scan(1, |start, section| {
        let current = *start;
        *start += section.entries.len();
        Some((current, section))
    });
    rsx! {
        div { class: "recipe-method",
            section { class: "surface-panel",
                h2 { "Ingredients" }
                if contents.ingredients.iter().any(|section| !section.entries.is_empty()) {
                    shopping_picker::ShoppingPicker {
                        id,
                        revision,
                        sections: contents.ingredients.clone(),
                    }
                }
                for section in &contents.ingredients {
                    div { class: if section.heading.is_some() { "recipe-subsection" },
                        if let Some(heading) = &section.heading { h3 { "{heading}" } }
                        ul { class: "recipe-ingredient-list",
                            for ingredient in &section.entries {
                                li {
                                    span { class: "recipe-ingredient-name", "{ingredient.name}" }
                                    if let Some(quantity) = &ingredient.quantity {
                                        span { class: "recipe-ingredient-quantity", "{quantity}" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            section { class: "surface-panel",
                h2 { "Steps" }
                for (start, section) in step_numbers {
                    div { class: if section.heading.is_some() { "recipe-subsection" },
                        if let Some(heading) = &section.heading { h3 { "{heading}" } }
                        ol { class: "recipe-step-list", start: start as i64,
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
}
