use super::{CardHeading, Route, StatusMark};
use dioxus::prelude::*;

#[derive(Clone, PartialEq)]
struct ShoppingItem {
    id: u64,
    name: String,
    quantity: String,
    checked: bool,
}

pub(super) struct ShoppingStore {
    items: Vec<ShoppingItem>,
    next_id: u64,
}

impl ShoppingStore {
    pub(super) fn sample() -> Self {
        Self {
            next_id: 5,
            items: [
                ("Milk", "2 litres", false),
                ("Bread", "1 loaf", false),
                ("Toilet paper", "1 pack", false),
                ("Pads", "1 pack", false),
                ("Eggs", "6", true),
            ]
            .into_iter()
            .enumerate()
            .map(|(index, (name, quantity, checked))| ShoppingItem {
                id: index as u64,
                name: name.into(),
                quantity: quantity.into(),
                checked,
            })
            .collect(),
        }
    }

    fn add(&mut self, name: &str, quantity: &str) -> Result<(), &'static str> {
        if name.trim().is_empty() {
            return Err("Enter an item name.");
        }
        self.items.push(ShoppingItem {
            id: self.next_id,
            name: name.trim().into(),
            quantity: quantity.trim().into(),
            checked: false,
        });
        self.next_id += 1;
        Ok(())
    }

    fn set_checked(&mut self, id: u64, checked: bool) {
        if let Some(item) = self.items.iter_mut().find(|item| item.id == id) {
            item.checked = checked;
        }
    }
}

#[component]
pub(super) fn ShoppingCard() -> Element {
    let store = use_context::<Signal<ShoppingStore>>();
    let state = store.read();
    let open = state.items.iter().filter(|item| !item.checked).count();
    rsx! {
        section { class: "dashboard-card", aria_labelledby: "shopping-title",
            CardHeading { icon: "shopping", title: "Shopping List", id: "shopping-title", count: format!("{open} to get") }
            div { class: "card-body",
                if open == 0 { p { class: "shopping-empty", "Nothing to pick up." } }
                ul { class: "item-list shopping-list",
                    for item in state.items.iter().filter(|item| !item.checked).take(4) {
                        PreviewRow { key: "{item.id}", item: item.clone() }
                    }
                }
                if let Some(item) = state.items.iter().find(|item| item.checked) {
                    h3 { class: "list-divider", "Already picked up" }
                    ul { class: "item-list shopping-list",
                        PreviewRow { item: item.clone() }
                    }
                }
            }
            Link { class: "card-page-link", to: Route::Shopping {}, "View shopping list" }
        }
    }
}

#[component]
fn PreviewRow(item: ShoppingItem) -> Element {
    rsx! {
        li { class: if item.checked { "list-row is-done" } else { "list-row" },
            StatusMark { done: item.checked }
            div { class: "row-copy",
                span { class: "row-title", "{item.name}" }
                if !item.quantity.is_empty() { span { class: "row-note", "{item.quantity}" } }
            }
        }
    }
}

#[component]
pub(super) fn Shopping() -> Element {
    rsx! {
        document::Title { "Lavemill · Shopping List" }
        main { id: "main", class: "dashboard shopping-page", tabindex: "-1",
            div { class: "shopping-heading",
                h1 { "Shopping List" }
                p { class: "mock-note", "Mock data · Resets on refresh" }
            }
            QuickAdd {}
            div { class: "shopping-groups",
                ShoppingGroup { checked: false }
                ShoppingGroup { checked: true }
            }
        }
    }
}

#[component]
fn ShoppingGroup(checked: bool) -> Element {
    let store = use_context::<Signal<ShoppingStore>>();
    let items: Vec<_> = store
        .read()
        .items
        .iter()
        .filter(|item| item.checked == checked)
        .cloned()
        .collect();
    let title = if checked { "Picked up" } else { "To get" };
    rsx! {
        section { class: "shopping-group", aria_label: title,
            div { class: "shopping-group-heading",
                h2 { "{title}" }
                span { class: "count-badge", "{items.len()}" }
            }
            if items.is_empty() {
                p { class: "shopping-empty", if checked { "No items picked up." } else { "Nothing to pick up." } }
            } else {
                ul { class: "item-list shopping-page-list",
                    for item in items { ShoppingItemRow { key: "{item.id}", item } }
                }
            }
        }
    }
}

#[component]
fn QuickAdd() -> Element {
    let mut store = use_context::<Signal<ShoppingStore>>();
    let mut name = use_signal(String::new);
    let mut quantity = use_signal(String::new);
    let mut error = use_signal(String::new);
    let mut feedback = use_signal(String::new);
    rsx! {
        form { class: "shopping-quick-add", aria_label: "Add shopping item",
            onsubmit: move |event| {
                event.prevent_default();
                let result = store.write().add(&name(), &quantity());
                match result {
                    Ok(()) => {
                        feedback.set(format!("Added: {}", name().trim()));
                        error.set(String::new());
                        name.set(String::new());
                        quantity.set(String::new());
                    }
                    Err(message) => { error.set(message.into()); feedback.set(String::new()); }
                }
            },
            div { class: "shopping-form-field",
                label { r#for: "shopping-name", "Item" }
                input { id: "shopping-name", name: "name", required: true, value: "{name}",
                    oninput: move |event| { name.set(event.value()); error.set(String::new()); feedback.set(String::new()); }
                }
            }
            div { class: "shopping-form-field",
                label { r#for: "shopping-quantity", "Quantity (optional)" }
                input { id: "shopping-quantity", name: "quantity", value: "{quantity}", oninput: move |event| quantity.set(event.value()) }
            }
            button { class: "primary-button", r#type: "submit", "Add item" }
            if !error().is_empty() { p { class: "form-error shopping-form-message", role: "alert", "{error}" } }
            p { class: "form-feedback shopping-form-message", role: "status", "{feedback}" }
        }
    }
}

#[component]
fn ShoppingItemRow(item: ShoppingItem) -> Element {
    let mut store = use_context::<Signal<ShoppingStore>>();
    let id = item.id;
    rsx! {
        li { class: if item.checked { "shopping-item-row is-done" } else { "shopping-item-row" },
            label { class: "shopping-check",
                input { r#type: "checkbox", checked: item.checked,
                    aria_label: if item.checked { format!("Mark {} as still to get", item.name) } else { format!("Mark {} as picked up", item.name) },
                    onchange: move |event| store.write().set_checked(id, event.checked())
                }
            }
            div { class: "row-copy",
                span { class: "row-title", "{item.name}" }
                if !item.quantity.is_empty() { span { class: "row-note", "{item.quantity}" } }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_add_preserves_free_text_quantities_and_duplicate_items() {
        let mut store = ShoppingStore::sample();
        let count = store.items.len();
        assert!(store.add("  ", "1 pack").is_err());
        assert_eq!(store.items.len(), count);
        store.add(" Milk ", " a handful ").unwrap();
        store.add("Milk", "").unwrap();
        let first = &store.items[count];
        let second = &store.items[count + 1];
        assert_eq!((&*first.name, &*first.quantity), ("Milk", "a handful"));
        assert_eq!(second.quantity, "");
        assert_ne!(first.id, second.id);
        assert_eq!(
            store
                .items
                .iter()
                .filter(|item| item.name == "Milk")
                .count(),
            3
        );
    }

    #[test]
    fn checking_and_unchecking_keeps_identity_content_and_order() {
        let mut store = ShoppingStore::sample();
        let original = store.items[1].clone();
        store.set_checked(original.id, true);
        assert!(store.items[1].checked);
        assert_eq!(store.items.iter().filter(|item| !item.checked).count(), 3);
        store.set_checked(original.id, false);
        assert!(store.items[1] == original);
        assert_eq!(store.items.iter().filter(|item| !item.checked).count(), 4);
    }
}
