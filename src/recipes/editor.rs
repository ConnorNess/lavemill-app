use super::{create_recipe, Ingredient, RecipeContents, SaveRecipe, Section, Step};
use crate::Route;
use dioxus::prelude::*;

#[derive(Default)]
struct EntryFields {
    text: String,
    detail: String,
}

#[derive(Default)]
struct SectionFields {
    heading: String,
    entries: Ordered<EntryFields>,
}

struct Ordered<T> {
    items: Vec<(usize, T)>,
    next_key: usize,
}

impl<T> Default for Ordered<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            next_key: 0,
        }
    }
}

impl<T> Ordered<T> {
    fn push(&mut self, value: T) {
        self.items.push((self.next_key, value));
        self.next_key += 1;
    }

    fn change(&mut self, index: usize, action: OrderAction) {
        match action {
            OrderAction::Up => self.items.swap(index, index - 1),
            OrderAction::Down => self.items.swap(index, index + 1),
            OrderAction::Remove => {
                self.items.remove(index);
            }
        }
    }
}

#[derive(Clone, Copy)]
enum OrderAction {
    Up,
    Down,
    Remove,
}

impl SectionFields {
    fn blank() -> Self {
        let mut section = Self::default();
        section.entries.push(EntryFields::default());
        section
    }
}

fn blank_sections() -> Ordered<SectionFields> {
    let mut sections = Ordered::default();
    sections.push(SectionFields::blank());
    sections
}

fn contents<T>(
    sections: &Ordered<SectionFields>,
    entry: impl Fn(&EntryFields) -> T,
) -> Vec<Section<T>> {
    sections
        .items
        .iter()
        .map(|(_, section)| Section {
            heading: Some(section.heading.clone()),
            entries: section
                .entries
                .items
                .iter()
                .map(|(_, fields)| entry(fields))
                .collect(),
        })
        .collect()
}

#[component]
pub(crate) fn NewRecipe() -> Element {
    rsx! {
        document::Title { "Lavemill · Add recipe" }
        main { id: "main", class: "dashboard recipes-page", tabindex: "-1",
            Link { to: Route::Recipes {}, "All recipes" }
            h1 { "Add recipe" }
            RecipeEditor {}
        }
    }
}

#[component]
fn RecipeEditor() -> Element {
    let mut title = use_signal(String::new);
    let ingredients = use_signal(blank_sections);
    let steps = use_signal(blank_sections);
    let mut pending = use_signal(|| false);
    let mut error = use_signal(String::new);
    let navigator = use_navigator();
    rsx! {
        form { class: "recipe-editor", onsubmit: move |event| {
            event.prevent_default();
            if pending() {
                return;
            }
            pending.set(true);
            error.set(String::new());
            let draft = RecipeContents {
                title: title(),
                ingredients: contents(&ingredients.read(), |fields| Ingredient {
                    name: fields.text.clone(),
                    quantity: Some(fields.detail.clone()),
                }),
                steps: contents(&steps.read(), |fields| Step {
                    instructions: fields.text.clone(),
                    note: Some(fields.detail.clone()),
                }),
            };
            spawn(async move {
                match create_recipe(draft).await {
                    Ok(SaveRecipe::Saved(id)) => { navigator.replace(Route::RecipeDetail { id }); }
                    Ok(SaveRecipe::Invalid(message)) => error.set(message),
                    Err(_) => error.set("Could not save the recipe. Try again.".into()),
                }
                pending.set(false);
            });
        },
            fieldset { disabled: pending(), class: "recipe-editor-fields",
                label { "Title", input { required: true, value: title(), oninput: move |event| title.set(event.value()) } }
                EditSections { sections: ingredients, ingredients: true }
                EditSections { sections: steps, ingredients: false }
                if !error().is_empty() { p { class: "form-error", role: "alert", {error()} } }
                button { class: "primary-button", r#type: "submit", "Save recipe" }
            }
        }
    }
}

#[component]
fn EditSections(mut sections: Signal<Ordered<SectionFields>>, ingredients: bool) -> Element {
    let sections_read = sections.read();
    let (heading, item_name, text_label, detail_label) = if ingredients {
        (
            "Ingredients",
            "ingredient",
            "Ingredient name",
            "Quantity (optional)",
        )
    } else {
        ("Steps", "step", "Instructions", "Note (optional)")
    };
    rsx! {
        section { class: "recipe-editor-section",
            h2 { "{heading}" }
            for (section_index, (section_key, section)) in sections_read.items.iter().enumerate() {
                fieldset { key: "{section_key}",
                    legend { "{heading} section {section_index + 1}" }
                    label { "Section heading (optional)",
                        input { value: section.heading.clone(), oninput: move |event| {
                            sections.write().items[section_index].1.heading = event.value();
                        } }
                    }
                    OrderButtons {
                        label: format!("{heading} section {}", section_index + 1),
                        index: section_index,
                        count: sections_read.items.len(),
                        change: move |action| sections.write().change(section_index, action),
                    }
                    for (entry_index, (entry_key, entry)) in section.entries.items.iter().enumerate() {
                        div { class: "recipe-editor-entry", key: "{entry_key}",
                            label { "{text_label}",
                                if ingredients {
                                    input { required: true, value: entry.text.clone(), oninput: move |event| {
                                        sections.write().items[section_index].1.entries.items[entry_index].1.text = event.value();
                                    } }
                                } else {
                                    textarea { required: true, value: entry.text.clone(), oninput: move |event| {
                                        sections.write().items[section_index].1.entries.items[entry_index].1.text = event.value();
                                    } }
                                }
                            }
                            label { "{detail_label}",
                                if ingredients {
                                    input { value: entry.detail.clone(), oninput: move |event| {
                                        sections.write().items[section_index].1.entries.items[entry_index].1.detail = event.value();
                                    } }
                                } else {
                                    textarea { value: entry.detail.clone(), oninput: move |event| {
                                        sections.write().items[section_index].1.entries.items[entry_index].1.detail = event.value();
                                    } }
                                }
                            }
                            OrderButtons {
                                label: format!("{item_name} {} in section {}", entry_index + 1, section_index + 1),
                                index: entry_index,
                                count: section.entries.items.len(),
                                change: move |action| sections.write().items[section_index].1.entries.change(entry_index, action),
                            }
                        }
                    }
                    button { class: "secondary-button", r#type: "button",
                        onclick: move |_| sections.write().items[section_index].1.entries.push(EntryFields::default()),
                        "Add {item_name}"
                    }
                }
            }
            button { class: "secondary-button", r#type: "button", onclick: move |_| sections.write().push(SectionFields::blank()),
                "Add {item_name} section"
            }
        }
    }
}

#[component]
fn OrderButtons(
    label: String,
    index: usize,
    count: usize,
    change: EventHandler<OrderAction>,
) -> Element {
    rsx! {
        div { class: "recipe-order-buttons",
            button { class: "secondary-button", r#type: "button", aria_label: "Move {label} up", disabled: index == 0,
                onclick: move |_| change.call(OrderAction::Up), "Move up" }
            button { class: "secondary-button", r#type: "button", aria_label: "Move {label} down", disabled: index + 1 == count,
                onclick: move |_| change.call(OrderAction::Down), "Move down" }
            button { class: "secondary-button", r#type: "button", aria_label: "Remove {label}",
                onclick: move |_| change.call(OrderAction::Remove), "Remove" }
        }
    }
}
