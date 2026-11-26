use super::{
    create_recipe, get_recipe, update_recipe, Ingredient, RecipeContents, SaveRecipe, Section, Step,
};
use crate::Route;
use dioxus::prelude::*;

#[derive(Clone, Default, PartialEq)]
struct EntryFields {
    text: String,
    detail: String,
}

enum EntryChange {
    Text(String),
    Detail(String),
}

impl EntryFields {
    fn update(&mut self, change: EntryChange) {
        match change {
            EntryChange::Text(value) => self.text = value,
            EntryChange::Detail(value) => self.detail = value,
        }
    }
}

struct SectionFields {
    heading: String,
    entries: Ordered<EntryFields>,
}

enum EditorItem {
    Entry(EntryFields),
    Subsection(SectionFields),
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

impl<T> FromIterator<T> for Ordered<T> {
    fn from_iter<I: IntoIterator<Item = T>>(values: I) -> Self {
        let items: Vec<_> = values.into_iter().enumerate().collect();
        Self {
            next_key: items.len(),
            items,
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
        Self {
            heading: String::new(),
            entries: [EntryFields::default()].into_iter().collect(),
        }
    }
}

fn blank_items() -> Ordered<EditorItem> {
    [EditorItem::Entry(EntryFields::default())]
        .into_iter()
        .collect()
}

fn edit_items<T>(
    sections: Vec<Section<T>>,
    entry: impl Fn(T) -> EntryFields,
) -> Ordered<EditorItem> {
    let mut items = Ordered::default();
    for section in sections {
        let entries = section.entries.into_iter().map(&entry);
        if let Some(heading) = section.heading {
            items.push(EditorItem::Subsection(SectionFields {
                heading,
                entries: entries.collect(),
            }));
        } else {
            for fields in entries {
                items.push(EditorItem::Entry(fields));
            }
        }
    }
    items
}

fn contents<T>(items: &Ordered<EditorItem>, entry: impl Fn(&EntryFields) -> T) -> Vec<Section<T>> {
    let mut sections: Vec<Section<T>> = Vec::new();
    for (_, item) in &items.items {
        match item {
            EditorItem::Entry(fields) => {
                let value = entry(fields);
                match sections.last_mut() {
                    Some(section) if section.heading.is_none() => section.entries.push(value),
                    _ => sections.push(Section {
                        heading: None,
                        entries: vec![value],
                    }),
                }
            }
            EditorItem::Subsection(section) => sections.push(Section {
                heading: Some(section.heading.clone()),
                entries: section
                    .entries
                    .items
                    .iter()
                    .map(|(_, fields)| entry(fields))
                    .collect(),
            }),
        }
    }
    sections
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
pub(crate) fn EditRecipe(id: i64) -> Element {
    rsx! { EditRecipeContent { key: "{id}", id } }
}

#[component]
fn EditRecipeContent(id: i64) -> Element {
    let recipe = use_server_future(move || get_recipe(id))?;
    rsx! {
        document::Title { "Lavemill · Edit recipe" }
        main { id: "main", class: "dashboard recipes-page", tabindex: "-1",
            Link { to: Route::RecipeDetail { id }, "Back to recipe" }
            h1 { "Edit recipe" }
            match recipe.read().as_ref() {
                Some(Ok(Some(recipe))) => rsx! {
                    RecipeEditor { target: EditTarget { id, revision: recipe.revision }, initial: recipe.contents.clone() }
                },
                Some(Ok(None)) => rsx! { p { "Recipe not found." } },
                Some(Err(_)) => rsx! { p { role: "alert", "Could not load this recipe. Reload to try again." } },
                None => rsx! { p { "Loading recipe…" } },
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
struct EditTarget {
    id: i64,
    revision: i64,
}

#[component]
fn RecipeEditor(target: Option<EditTarget>, initial: Option<RecipeContents>) -> Element {
    let (mut title, ingredients, steps) = use_hook(|| {
        let (title, ingredients, steps) = match initial {
            Some(recipe) => (
                recipe.title,
                edit_items(recipe.ingredients, |ingredient| EntryFields {
                    text: ingredient.name,
                    detail: ingredient.quantity.unwrap_or_default(),
                }),
                edit_items(recipe.steps, |step| EntryFields {
                    text: step.instructions,
                    detail: step.note.unwrap_or_default(),
                }),
            ),
            None => (String::new(), blank_items(), blank_items()),
        };
        (
            Signal::new(title),
            Signal::new(ingredients),
            Signal::new(steps),
        )
    });
    let mut pending = use_signal(|| false);
    let mut error = use_signal(String::new);
    let mut conflict = use_signal(|| false);
    let navigator = use_navigator();
    rsx! {
        form { class: "recipe-editor", onsubmit: move |event| {
            event.prevent_default();
            if pending() {
                return;
            }
            pending.set(true);
            error.set(String::new());
            conflict.set(false);
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
                let result = match target {
                    Some(target) => update_recipe(target.id, target.revision, draft).await,
                    None => create_recipe(draft).await,
                };
                match result {
                    Ok(SaveRecipe::Saved(id)) => { navigator.replace(Route::RecipeDetail { id }); }
                    Ok(SaveRecipe::Invalid(message)) => error.set(message),
                    Ok(SaveRecipe::Conflict) => {
                        conflict.set(true);
                        error.set("Someone saved a newer version. Your draft is still here.".into());
                    }
                    Err(_) => error.set("Could not save the recipe. Try again.".into()),
                }
                pending.set(false);
            });
        },
            fieldset { disabled: pending(), class: "recipe-editor-fields",
                label { "Title", input { required: true, value: title(), oninput: move |event| title.set(event.value()) } }
                div { class: "recipe-editor-columns",
                    EditItems { items: ingredients, ingredients: true }
                    EditItems { items: steps, ingredients: false }
                }
                if !error().is_empty() { p { class: "form-error", role: "alert", {error()} } }
                if conflict() {
                    if let Some(target) = target {
                        Link { to: Route::RecipeDetail { id: target.id }, new_tab: true, "View latest recipe in a new tab" }
                    }
                }
                button { class: "primary-button", r#type: "submit", "Save recipe" }
            }
        }
    }
}

#[component]
fn EditItems(mut items: Signal<Ordered<EditorItem>>, ingredients: bool) -> Element {
    let draft = items.read();
    let (heading, item_name) = if ingredients {
        ("Ingredients", "ingredient")
    } else {
        ("Steps", "step")
    };
    rsx! {
        section { class: "recipe-editor-section",
            h2 { "{heading}" }
            for (index, (key, item)) in draft.items.iter().enumerate() {
                match item {
                    EditorItem::Entry(entry) => rsx! {
                        EntryRow {
                            key: "item-{key}",
                            ingredients,
                            entry: entry.clone(),
                            label: format!("{item_name} at position {}", index + 1),
                            index,
                            count: draft.items.len(),
                            update: move |change| {
                                if let EditorItem::Entry(entry) = &mut items.write().items[index].1 {
                                    entry.update(change);
                                }
                            },
                            order: move |action| items.write().change(index, action),
                        }
                    },
                    EditorItem::Subsection(section) => rsx! {
                        fieldset { class: "recipe-editor-subsection", key: "item-{key}", aria_label: "{heading} subsection at position {index + 1}",
                            div { class: "recipe-editor-row",
                                input {
                                    required: true,
                                    aria_label: "Subsection name",
                                    placeholder: "Subsection name",
                                    value: section.heading.clone(),
                                    oninput: move |event| {
                                        if let EditorItem::Subsection(section) = &mut items.write().items[index].1 {
                                            section.heading = event.value();
                                        }
                                    },
                                }
                                OrderButtons {
                                    label: format!("{heading} subsection at position {}", index + 1),
                                    index,
                                    count: draft.items.len(),
                                    change: move |action| items.write().change(index, action),
                                }
                            }
                            for (entry_index, (entry_key, entry)) in section.entries.items.iter().enumerate() {
                                EntryRow {
                                    key: "{entry_key}",
                                    ingredients,
                                    entry: entry.clone(),
                                    label: format!("{item_name} {} in subsection at position {}", entry_index + 1, index + 1),
                                    index: entry_index,
                                    count: section.entries.items.len(),
                                    update: move |change| {
                                        if let EditorItem::Subsection(section) = &mut items.write().items[index].1 {
                                            section.entries.items[entry_index].1.update(change);
                                        }
                                    },
                                    order: move |action| {
                                        if let EditorItem::Subsection(section) = &mut items.write().items[index].1 {
                                            section.entries.change(entry_index, action);
                                        }
                                    },
                                }
                            }
                            button { class: "secondary-button", r#type: "button",
                                onclick: move |_| {
                                    if let EditorItem::Subsection(section) = &mut items.write().items[index].1 {
                                        section.entries.push(EntryFields::default());
                                    }
                                },
                                "Add {item_name} to subsection"
                            }
                        }
                    },
                }
            }
            div { class: "recipe-editor-actions",
                button { class: "secondary-button", r#type: "button",
                    onclick: move |_| items.write().push(EditorItem::Entry(EntryFields::default())),
                    "Add {item_name}"
                }
                button { class: "secondary-button", r#type: "button",
                    onclick: move |_| items.write().push(EditorItem::Subsection(SectionFields::blank())),
                    "Add subsection"
                }
            }
        }
    }
}

#[component]
fn EntryRow(
    ingredients: bool,
    entry: EntryFields,
    label: String,
    index: usize,
    count: usize,
    update: EventHandler<EntryChange>,
    order: EventHandler<OrderAction>,
) -> Element {
    rsx! {
        div { class: "recipe-editor-row",
            if ingredients {
                div { class: "recipe-editor-ingredient",
                    input { required: true, aria_label: "Ingredient name", placeholder: "Ingredient name",
                        value: entry.text, oninput: move |event| update.call(EntryChange::Text(event.value())) }
                    input { aria_label: "Quantity (optional)", placeholder: "Quantity",
                        value: entry.detail, oninput: move |event| update.call(EntryChange::Detail(event.value())) }
                }
            } else {
                div { class: "recipe-editor-step",
                    textarea { required: true, rows: 2, aria_label: "Instructions", placeholder: "Instructions",
                        value: entry.text, oninput: move |event| update.call(EntryChange::Text(event.value())) }
                    textarea { rows: 1, aria_label: "Note (optional)", placeholder: "Note (optional)",
                        value: entry.detail, oninput: move |event| update.call(EntryChange::Detail(event.value())) }
                }
            }
            OrderButtons { label, index, count, change: order }
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
    let up = format!("Move {label} up");
    let down = format!("Move {label} down");
    let remove = format!("Remove {label}");
    rsx! {
        div { class: "recipe-order-buttons",
            button { r#type: "button", aria_label: up.clone(), title: up, disabled: index == 0,
                onclick: move |_| change.call(OrderAction::Up), "↑" }
            button { r#type: "button", aria_label: down.clone(), title: down, disabled: index + 1 == count,
                onclick: move |_| change.call(OrderAction::Down), "↓" }
            button { r#type: "button", aria_label: remove.clone(), title: remove,
                onclick: move |_| change.call(OrderAction::Remove), "×" }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_a_subsection_keeps_its_entries_together() {
        let main = |entries: Vec<&str>| Section {
            heading: None,
            entries: entries.into_iter().map(String::from).collect(),
        };
        let marinade = Section {
            heading: Some("Marinade".into()),
            entries: vec!["Soy sauce".into(), "Garlic puree".into()],
        };
        let saved = vec![
            main(vec!["Pasta", "Chicken breast"]),
            marinade.clone(),
            main(vec!["Salt", "Pepper"]),
        ];
        let mut draft = edit_items(saved.clone(), |name| EntryFields {
            text: name,
            detail: String::new(),
        });
        let ingredient_name = |fields: &EntryFields| fields.text.clone();

        assert_eq!(contents(&draft, ingredient_name), saved);
        draft.change(2, OrderAction::Down);
        assert_eq!(
            contents(&draft, ingredient_name),
            vec![
                main(vec!["Pasta", "Chicken breast", "Salt"]),
                marinade,
                main(vec!["Pepper"]),
            ]
        );
    }
}
