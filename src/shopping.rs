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
}

impl ShoppingStore {
    pub(super) fn sample() -> Self {
        Self {
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
                ul { class: "item-list shopping-list shopping-page-list",
                    for item in items { PreviewRow { key: "{item.id}", item } }
                }
            }
        }
    }
}
