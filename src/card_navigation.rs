use crate::Route;
use dioxus::prelude::*;

#[component]
pub(crate) fn HomeCardLink(to: Route, label: &'static str, page: &'static str) -> Element {
    let navigator = use_navigator();
    let destination = to.clone();
    rsx! {
        Link {
            class: "card-hit-area",
            to,
            aria_label: label,
            "data-card-page": page,
            onclick_only: true,
            onclick: move |_| {
                let destination = destination.clone();
                spawn(async move {
                    if begin_transition(page).await {
                        navigator.push(destination);
                    }
                });
            },
        }
    }
}

async fn begin_transition(page: &str) -> bool {
    let mut transition = document::eval(include_str!("../assets/card-navigation.js"));
    if transition.send(page).is_err() {
        return true;
    }
    transition.recv::<bool>().await.unwrap_or(true)
}
