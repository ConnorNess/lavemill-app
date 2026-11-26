use super::{
    add_recipe_ingredients, AddRecipeIngredients, Ingredient, IngredientPosition, Section,
};
use crate::Route;
use dioxus::prelude::*;
use std::collections::BTreeSet;

const OPEN_PICKER: &str = r#"
    const dialog = document.getElementById("recipe-shopping-picker");
    dialog.oncancel = (event) => {
        if (dialog.querySelector("fieldset").disabled) event.preventDefault();
    };
    dialog.showModal();
"#;
const CLOSE_PICKER: &str = r#"document.getElementById("recipe-shopping-picker").close();"#;

#[component]
pub(super) fn ShoppingPicker(
    id: i64,
    revision: i64,
    sections: Vec<Section<Ingredient>>,
) -> Element {
    let mut selection = use_signal(BTreeSet::<IngredientPosition>::new);
    let mut pending = use_signal(|| false);
    let mut stale = use_signal(|| false);
    let mut error = use_signal(|| "");
    let mut feedback = use_signal(|| None::<(usize, usize)>);
    let sections_for_open = sections.clone();

    rsx! {
        button {
            r#type: "button",
            class: "recipe-action",
            onclick: move |_| {
                if pending() {
                    return;
                }
                selection.set(
                    sections_for_open.iter().enumerate().flat_map(|(section, ingredients)| {
                        (0..ingredients.entries.len()).map(move |entry| (section, entry))
                    }).collect()
                );
                if !stale() {
                    error.set("");
                }
                let _ = document::eval(OPEN_PICKER);
            },
            "Add to shopping list"
        }
        div { class: "recipe-shopping-feedback", role: "status",
            if let Some((added, skipped)) = feedback() {
                p { "Added {added}. Skipped {skipped} already on the shopping list." }
            }
        }
        if feedback().is_some() {
            Link { to: Route::Shopping {}, "View shopping list" }
        }
        dialog { id: "recipe-shopping-picker", class: "recipe-shopping-picker",
            aria_labelledby: "recipe-shopping-picker-title",
            h2 { id: "recipe-shopping-picker-title", "Add to shopping list" }
            form {
                onsubmit: move |event| {
                    event.prevent_default();
                    if pending() || stale() || selection.read().is_empty() {
                        return;
                    }
                    let positions = selection.read().iter().copied().collect();
                    pending.set(true);
                    error.set("");
                    spawn(async move {
                        match add_recipe_ingredients(id, revision, positions).await {
                            Ok(AddRecipeIngredients::Added { added, skipped }) => {
                                feedback.set(Some((added, skipped)));
                                let _ = document::eval(CLOSE_PICKER);
                            }
                            Ok(AddRecipeIngredients::InvalidSelection) => {
                                error.set("Choose ingredients and try again.");
                            }
                            Ok(AddRecipeIngredients::StaleRecipe) => {
                                stale.set(true);
                                error.set("This recipe changed or is unavailable. Reload it to continue.");
                            }
                            Err(_) => error.set("Could not add ingredients. Try again."),
                        }
                        pending.set(false);
                    });
                },
                fieldset { disabled: pending(),
                    legend { class: "sr-only", "Ingredients to add" }
                    div { class: "recipe-shopping-ingredients",
                        for (section_index, section) in sections.iter().enumerate() {
                            div { key: "{section_index}", class: "recipe-shopping-section",
                                if let Some(heading) = &section.heading {
                                    h3 { "{heading}" }
                                }
                                for (entry_index, ingredient) in section.entries.iter().enumerate() {
                                    {
                                        let position = (section_index, entry_index);
                                        rsx! {
                                            label { key: "{entry_index}",
                                                input {
                                                    r#type: "checkbox",
                                                    checked: selection.read().contains(&position),
                                                    onchange: move |event| {
                                                        if event.checked() {
                                                            selection.write().insert(position);
                                                        } else {
                                                            selection.write().remove(&position);
                                                        }
                                                    },
                                                }
                                                span {
                                                    "{ingredient.name}"
                                                    if let Some(quantity) = &ingredient.quantity {
                                                        span { class: "row-note", " · {quantity}" }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if !error().is_empty() {
                        p { class: "form-error", role: "alert", "{error}" }
                    }
                    div { class: "recipe-shopping-actions",
                        button {
                            r#type: "button",
                            onclick: move |_| { let _ = document::eval(CLOSE_PICKER); },
                            "Cancel"
                        }
                        if stale() {
                            button {
                                r#type: "button",
                                class: "primary-button",
                                onclick: move |_| { let _ = document::eval("window.location.reload();"); },
                                "Reload recipe"
                            }
                        } else {
                            button {
                                r#type: "submit",
                                class: "primary-button",
                                disabled: selection.read().is_empty(),
                                if pending() { "Adding…" } else { "Add selected ingredients" }
                            }
                        }
                    }
                }
            }
        }
    }
}
