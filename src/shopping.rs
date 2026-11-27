use super::{CardHeading, Route};
use crate::shopping_data::{change_shopping, list_shopping, ShoppingChange, ShoppingItem};
use dioxus::prelude::*;

#[derive(Clone, PartialEq)]
pub(super) struct RemovedItem {
    id: i64,
    name: String,
}

#[derive(Clone, Copy)]
struct ShoppingActions {
    items: Signal<Option<Vec<ShoppingItem>>>,
    pending: Signal<bool>,
    refresh_failed: Signal<bool>,
}

impl ShoppingActions {
    fn disabled(self) -> bool {
        (self.pending)() || (self.refresh_failed)()
    }

    async fn save(mut self, change: ShoppingChange) -> Result<(), String> {
        if self.disabled() {
            return Err("Wait for the list to refresh before saving.".into());
        }
        self.pending.set(true);
        let result = match change_shopping(change).await {
            Ok(None) => {
                self.refresh().await;
                Ok(())
            }
            Ok(Some(message)) => Err(message),
            Err(_) => Err("Could not save the change. Reload the list before trying again.".into()),
        };
        self.pending.set(false);
        result
    }

    async fn refresh(mut self) {
        match list_shopping().await {
            Ok(items) => {
                self.items.set(Some(items));
                self.refresh_failed.set(false);
            }
            Err(_) => self.refresh_failed.set(true),
        }
    }
}

fn use_shopping_actions() -> ShoppingActions {
    ShoppingActions {
        items: use_signal(|| None),
        pending: use_signal(|| false),
        refresh_failed: use_signal(|| false),
    }
}

#[component]
pub(super) fn ShoppingCard() -> Element {
    let initial = use_server_future(list_shopping)?;
    let actions = use_shopping_actions();
    use_context_provider(|| actions);
    let initial = initial.read();
    let refreshed = actions.items.read();
    let items = refreshed
        .as_ref()
        .or_else(|| initial.as_ref()?.as_ref().ok());
    rsx! {
        crate::card_navigation::HomeCard {
            to: Route::Shopping {}, label: "Open shopping list", page: "shopping", heading: "shopping-title", scrollable: true,
            CardHeading { icon: "shopping", title: "Shopping List", id: "shopping-title",
                count: match items {
                    Some(items) => format!("{} to get", items.iter().filter(|item| !item.checked).count()),
                    _ => String::new(),
                }
            }
            div { class: "card-body",
                if (actions.refresh_failed)() {
                    p { class: "form-error", role: "alert", "Your change was saved, but the shopping list could not refresh. Reload the page to try again." }
                }
                if let Some(items) = items {
                    if !items.iter().any(|item| !item.checked) {
                        p { class: "shopping-empty", "Nothing to pick up." }
                    }
                    ul { class: "item-list shopping-list",
                        for item in items.iter().filter(|item| !item.checked) {
                            PreviewRow { key: "{item.id}", item: item.clone() }
                        }
                    }
                    if items.iter().any(|item| item.checked) {
                        h3 { class: "list-divider", "Already picked up" }
                        ul { class: "item-list shopping-list",
                            for item in items.iter().filter(|item| item.checked) {
                                PreviewRow { key: "{item.id}", item: item.clone() }
                            }
                        }
                    }
                } else if matches!(initial.as_ref(), Some(Err(_))) {
                    p { class: "form-error", role: "alert", "Could not load the shopping list." }
                } else {
                    p { class: "shopping-empty", role: "status", "Loading shopping list…" }
                }
            }
        }
    }
}

#[component]
fn PreviewRow(item: ShoppingItem) -> Element {
    let error = use_signal(String::new);
    rsx! {
        li { class: if item.checked { "list-row is-done" } else { "list-row" },
            ShoppingCheckbox { item: item.clone(), error }
            div { class: "row-copy",
                span { class: "row-title", "{item.name}" }
                if let Some(quantity) = item.quantity { span { class: "row-note", "{quantity}" } }
                if !error().is_empty() { p { class: "form-error", role: "alert", "{error}" } }
            }
        }
    }
}

#[component]
pub(super) fn Shopping() -> Element {
    let initial = use_server_future(list_shopping)?;
    let editing = use_signal(|| None::<i64>);
    let actions = use_shopping_actions();
    use_context_provider(|| actions);
    let initial = initial.read();
    let refreshed = actions.items.read();
    let items = refreshed
        .as_ref()
        .or_else(|| initial.as_ref()?.as_ref().ok());
    let refresh_failed = (actions.refresh_failed)();
    let load_failed = items.is_none() && matches!(initial.as_ref(), Some(Err(_)));
    rsx! {
        document::Title { "Lavemill · Shopping List" }
        main { id: "main", class: "dashboard interior-page shopping-page", tabindex: "-1",
            div { class: "shopping-heading",
                h1 { "Shopping List" }
            }
            if refresh_failed || load_failed {
                p { class: "form-error", role: "alert",
                    if items.is_some() { "Your change was saved, but the list could not refresh." }
                    else { "Could not load the shopping list." }
                }
                button { class: "secondary-button", disabled: (actions.pending)(),
                    onclick: move |_| async move {
                        let mut actions = actions;
                        actions.pending.set(true);
                        actions.refresh().await;
                        actions.pending.set(false);
                    }, "Reload list"
                }
            }
            if let Some(items) = items {
                QuickAdd {}
                RemovedNotice {}
                div { class: "shopping-groups",
                    ShoppingGroup { checked: false, items: items.iter().filter(|item| !item.checked).cloned().collect(), editing }
                    ShoppingGroup { checked: true, items: items.iter().filter(|item| item.checked).cloned().collect(), editing }
                }
            } else if !load_failed {
                p { class: "shopping-empty", role: "status", "Loading shopping list…" }
            }
        }
    }
}

#[component]
fn ShoppingGroup(checked: bool, items: Vec<ShoppingItem>, editing: Signal<Option<i64>>) -> Element {
    let title = if checked { "Picked up" } else { "To get" };
    rsx! {
        section { class: "shopping-group surface-panel", aria_label: title,
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
    let actions = use_context::<ShoppingActions>();
    let mut name = use_signal(String::new);
    let mut quantity = use_signal(String::new);
    let mut error = use_signal(String::new);
    let mut feedback = use_signal(String::new);
    rsx! {
        form { class: "shopping-quick-add surface-panel", aria_label: "Add shopping item",
            onsubmit: move |event| {
                event.prevent_default();
                let submitted_name = name();
                let change = ShoppingChange::Add { name: submitted_name.clone(), quantity: quantity() };
                async move { match actions.save(change).await {
                    Ok(()) => {
                        feedback.set(format!("Added: {}", submitted_name.trim()));
                        error.set(String::new());
                        name.set(String::new());
                        quantity.set(String::new());
                    }
                    Err(message) => { error.set(message); feedback.set(String::new()); }
                } }
            },
            div { class: "shopping-form-field",
                label { r#for: "shopping-name", "Item" }
                input { id: "shopping-name", name: "name", required: true, value: "{name}", readonly: actions.disabled(),
                    oninput: move |event| { name.set(event.value()); error.set(String::new()); feedback.set(String::new()); }
                }
            }
            div { class: "shopping-form-field",
                label { r#for: "shopping-quantity", "Quantity (optional)" }
                input { id: "shopping-quantity", name: "quantity", value: "{quantity}", readonly: actions.disabled(), oninput: move |event| quantity.set(event.value()) }
            }
            button { class: "primary-button", r#type: "submit", disabled: actions.disabled(), "Add item" }
            if !error().is_empty() { p { class: "form-error shopping-form-message", role: "alert", "{error}" } }
            p { class: "form-feedback shopping-form-message", role: "status", "{feedback}" }
        }
    }
}

#[component]
fn ShoppingItemRow(item: ShoppingItem, mut editing: Signal<Option<i64>>) -> Element {
    let actions = use_context::<ShoppingActions>();
    let mut removed = use_context::<Signal<Option<RemovedItem>>>();
    let mut error = use_signal(String::new);
    let id = item.id;
    let active_editor = editing();
    let removed_item = RemovedItem {
        id,
        name: item.name.clone(),
    };
    let remove = move |_| {
        let removed_item = removed_item.clone();
        async move {
            error.set(String::new());
            match actions.save(ShoppingChange::Remove { id }).await {
                Ok(()) => {
                    if *editing.peek() == Some(id) {
                        editing.set(None);
                    }
                    removed.set(Some(removed_item));
                }
                Err(message) => error.set(message),
            }
        }
    };
    rsx! {
        li { class: if item.checked { "shopping-item-row is-done" } else { "shopping-item-row" },
            if active_editor == Some(id) {
                ShoppingEditForm { item: item.clone(), editing, on_remove: remove.clone() }
            } else {
                ShoppingCheckbox { item: item.clone(), error }
                div { class: "row-copy",
                    span { class: "row-title", "{item.name}" }
                    if let Some(quantity) = item.quantity { span { class: "row-note", "{quantity}" } }
                }
                button { class: "edit-item-button", r#type: "button", disabled: active_editor.is_some() || actions.disabled(), aria_label: "Edit {item.name}",
                    onclick: move |_| editing.set(Some(id)), "Edit" }
                button { class: "remove-item-button", r#type: "button", aria_label: "Remove {item.name}",
                    disabled: actions.disabled(), onclick: remove, "×" }
            }
            if !error().is_empty() { p { class: "form-error shopping-form-message", role: "alert", "{error}" } }
        }
    }
}

#[component]
fn ShoppingCheckbox(item: ShoppingItem, mut error: Signal<String>) -> Element {
    let actions = use_context::<ShoppingActions>();
    let mut checkbox_revision = use_signal(|| 0);
    let id = item.id;
    rsx! {
        label { class: "shopping-check",
            for revision in [checkbox_revision()] {
                input { key: "{revision}", r#type: "checkbox", checked: item.checked, disabled: actions.disabled(),
                    aria_label: if item.checked { format!("Mark {} as still to get", item.name) } else { format!("Mark {} as picked up", item.name) },
                    onchange: move |event| {
                        let checked = event.checked();
                        async move {
                            error.set(String::new());
                            if let Err(message) = actions.save(ShoppingChange::SetChecked { id, checked }).await {
                                checkbox_revision += 1;
                                error.set(message);
                            } else if (actions.refresh_failed)() {
                                checkbox_revision += 1;
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn ShoppingEditForm(
    item: ShoppingItem,
    mut editing: Signal<Option<i64>>,
    on_remove: EventHandler<MouseEvent>,
) -> Element {
    let actions = use_context::<ShoppingActions>();
    let id = item.id;
    let mut name = use_signal(|| item.name.clone());
    let mut quantity = use_signal(|| item.quantity.unwrap_or_default());
    let mut error = use_signal(String::new);
    rsx! {
        form { class: "shopping-edit-form surface-panel", aria_label: "Edit {item.name}",
            onsubmit: move |event| {
                event.prevent_default();
                async move {
                    if *editing.peek() != Some(id) || actions.disabled() { return; }
                    let change = ShoppingChange::Edit { id, name: name(), quantity: quantity() };
                    match actions.save(change).await {
                        Ok(()) => editing.set(None),
                        Err(message) => error.set(message),
                    }
                }
            },
            div { class: "shopping-form-field",
                label { r#for: "edit-name-{id}", "Item" }
                input { id: "edit-name-{id}", name: "name", required: true, value: "{name}", readonly: actions.disabled(),
                    oninput: move |event| {
                        name.set(event.value()); error.set(String::new());
                    }
                }
            }
            div { class: "shopping-form-field",
                label { r#for: "edit-quantity-{id}", "Quantity (optional)" }
                input { id: "edit-quantity-{id}", name: "quantity", value: "{quantity}", readonly: actions.disabled(),
                    oninput: move |event| { quantity.set(event.value()) }
                }
            }
            if !error().is_empty() { p { class: "form-error shopping-form-message", role: "alert", {error()} } }
            div { class: "shopping-edit-actions",
                button { class: "primary-button", r#type: "submit", disabled: actions.disabled(), "Save" }
                button { class: "secondary-button", r#type: "button", disabled: (actions.pending)(), onclick: move |_| editing.set(None), "Cancel" }
                button { class: "remove-item-button", r#type: "button", aria_label: "Remove {item.name}",
                    disabled: actions.disabled(), onclick: move |event| on_remove.call(event), "×" }
            }
        }
    }
}

#[component]
fn RemovedNotice() -> Element {
    let actions = use_context::<ShoppingActions>();
    let mut removed = use_context::<Signal<Option<RemovedItem>>>();
    let mut error = use_signal(|| None::<(i64, String)>);
    rsx! {
        div { class: "shopping-removal-status surface-panel",
            if let Some(RemovedItem { id, name }) = removed() {
                p { role: "status", "Removed {name}." }
                button { class: "secondary-button", r#type: "button", aria_label: "Undo removal of {name}",
                    disabled: actions.disabled(),
                    onclick: move |_| async move {
                        error.set(None);
                        match actions.save(ShoppingChange::Restore { id }).await {
                            Ok(()) => removed.set(None),
                            Err(message) => error.set(Some((id, message))),
                        }
                    }, "Undo" }
                if let Some((_, message)) = error().filter(|(failed_id, _)| *failed_id == id) {
                    p { class: "form-error", role: "alert", "{message}" }
                }
            }
        }
    }
}
