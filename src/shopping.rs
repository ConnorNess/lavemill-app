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
    removed: Option<RemovedItem>,
}

struct RemovedItem {
    item: ShoppingItem,
    index: usize,
}

#[derive(Clone, PartialEq)]
struct EditDraft {
    id: u64,
    name: String,
    quantity: String,
    error: String,
}

impl EditDraft {
    fn from_item(item: &ShoppingItem) -> Self {
        Self {
            id: item.id,
            name: item.name.clone(),
            quantity: item.quantity.clone(),
            error: String::new(),
        }
    }
}

impl ShoppingStore {
    pub(super) fn sample() -> Self {
        Self {
            next_id: 5,
            removed: None,
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

    fn save_edit(&mut self, draft: &EditDraft) -> Result<(), &'static str> {
        if draft.name.trim().is_empty() {
            return Err("Enter an item name.");
        }
        let item = self
            .items
            .iter_mut()
            .find(|item| item.id == draft.id)
            .ok_or("This item is no longer on the list.")?;
        item.name = draft.name.trim().into();
        item.quantity = draft.quantity.trim().into();
        Ok(())
    }

    fn remove(&mut self, id: u64) {
        if let Some(index) = self.items.iter().position(|item| item.id == id) {
            self.removed = Some(RemovedItem {
                item: self.items.remove(index),
                index,
            });
        }
    }

    fn undo_remove(&mut self) {
        if let Some(removed) = self.removed.take() {
            self.items
                .insert(removed.index.min(self.items.len()), removed.item);
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
    let editing = use_signal(|| None::<EditDraft>);
    rsx! {
        document::Title { "Lavemill · Shopping List" }
        main { id: "main", class: "dashboard shopping-page", tabindex: "-1",
            div { class: "shopping-heading",
                h1 { "Shopping List" }
                p { class: "mock-note", "Mock data · Resets on refresh" }
            }
            QuickAdd {}
            RemovedNotice {}
            div { class: "shopping-groups",
                ShoppingGroup { checked: false, editing }
                ShoppingGroup { checked: true, editing }
            }
        }
    }
}

#[component]
fn ShoppingGroup(checked: bool, editing: Signal<Option<EditDraft>>) -> Element {
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
                    for item in items { ShoppingItemRow { key: "{item.id}", item, editing } }
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
fn ShoppingItemRow(item: ShoppingItem, mut editing: Signal<Option<EditDraft>>) -> Element {
    let mut store = use_context::<Signal<ShoppingStore>>();
    let id = item.id;
    let draft = editing
        .read()
        .as_ref()
        .filter(|draft| draft.id == id)
        .cloned();
    let editing_other = editing.read().is_some();
    let original_draft = EditDraft::from_item(&item);
    rsx! {
        li { class: if item.checked { "shopping-item-row is-done" } else { "shopping-item-row" },
            if let Some(draft) = draft {
                form { class: "shopping-edit-form", aria_label: "Edit {item.name}",
                    onsubmit: move |event| {
                        event.prevent_default();
                        let draft = editing.peek().clone();
                        if let Some(draft) = draft {
                            match store.write().save_edit(&draft) {
                                Ok(()) => editing.set(None),
                                Err(message) => {
                                    if let Some(draft) = editing.write().as_mut() { draft.error = message.into(); }
                                }
                            }
                        }
                    },
                    div { class: "shopping-form-field",
                        label { r#for: "edit-name-{id}", "Item" }
                        input { id: "edit-name-{id}", name: "name", required: true, value: "{draft.name}",
                            oninput: move |event| {
                                if let Some(draft) = editing.write().as_mut() { draft.name = event.value(); draft.error.clear(); }
                            }
                        }
                    }
                    div { class: "shopping-form-field",
                        label { r#for: "edit-quantity-{id}", "Quantity (optional)" }
                        input { id: "edit-quantity-{id}", name: "quantity", value: "{draft.quantity}",
                            oninput: move |event| { if let Some(draft) = editing.write().as_mut() { draft.quantity = event.value(); } }
                        }
                    }
                    if !draft.error.is_empty() { p { class: "form-error shopping-form-message", role: "alert", "{draft.error}" } }
                    div { class: "shopping-edit-actions",
                        button { class: "primary-button", r#type: "submit", "Save" }
                        button { class: "secondary-button", r#type: "button", onclick: move |_| editing.set(None), "Cancel" }
                        button { class: "remove-item-button", r#type: "button", aria_label: "Remove {item.name}",
                            onclick: move |_| { editing.set(None); store.write().remove(id); }, "×" }
                    }
                }
            } else {
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
                button { class: "edit-item-button", r#type: "button", disabled: editing_other, aria_label: "Edit {item.name}",
                    onclick: move |_| editing.set(Some(original_draft.clone())), "Edit" }
                button { class: "remove-item-button", r#type: "button", aria_label: "Remove {item.name}",
                    onclick: move |_| store.write().remove(id), "×" }
            }
        }
    }
}

#[component]
fn RemovedNotice() -> Element {
    let mut store = use_context::<Signal<ShoppingStore>>();
    let name = store
        .read()
        .removed
        .as_ref()
        .map(|removed| removed.item.name.clone());
    rsx! {
        div { class: "shopping-removal-status",
            if let Some(name) = name {
                p { role: "status", "Removed {name}." }
                button { class: "secondary-button", r#type: "button", aria_label: "Undo removal of {name}",
                    onclick: move |_| store.write().undo_remove(), "Undo" }
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

    #[test]
    fn editing_uses_a_draft_and_preserves_identity_checked_state_and_order() {
        let mut store = ShoppingStore::sample();
        let original = store.items[4].clone();
        let mut draft = EditDraft::from_item(&original);
        draft.name = "  ".into();
        draft.quantity = "2 boxes".into();
        assert!(store.save_edit(&draft).is_err());
        assert!(store.items[4] == original);
        assert_eq!(draft.quantity, "2 boxes");
        draft.name = " Large eggs ".into();
        store.save_edit(&draft).unwrap();
        let saved = &store.items[4];
        assert_eq!((saved.id, saved.checked), (original.id, true));
        assert_eq!((&*saved.name, &*saved.quantity), ("Large eggs", "2 boxes"));
        let mut cancelled = EditDraft::from_item(saved);
        cancelled.name = "Discard this change".into();
        drop(cancelled);
        assert_eq!(store.items[4].name, "Large eggs");
    }

    #[test]
    fn removal_undo_restores_checked_and_open_items_in_their_original_positions() {
        let mut store = ShoppingStore::sample();
        let original = store.items.clone();
        for index in [0, 2, 4] {
            store.remove(original[index].id);
            assert_eq!(store.items.len(), original.len() - 1);
            assert!(!store.items.iter().any(|item| item.id == original[index].id));
            store.undo_remove();
            assert!(store.items == original);
            assert!(store.removed.is_none());
            store.undo_remove();
            assert!(store.items == original);
        }
    }

    #[test]
    fn undo_only_restores_latest_removal_and_new_items_keep_unique_ids() {
        let mut store = ShoppingStore::sample();
        store.remove(1);
        store.remove(4);
        store.add("Eggs", "12").unwrap();
        store.undo_remove();
        assert!(!store.items.iter().any(|item| item.id == 1));
        assert_eq!(
            store.items.iter().map(|item| item.id).collect::<Vec<_>>(),
            [0, 2, 3, 4, 5]
        );
        assert!(store.items[3].checked);
        assert!(!store.items[4].checked);
    }
}
