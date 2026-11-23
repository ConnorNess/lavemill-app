use dioxus::prelude::*;
#[cfg(feature = "server")]
mod database;

mod todos;
use todos::{TasksCard, Todos};
mod shopping;
use shopping::{Shopping, ShoppingCard};

#[derive(Debug, Clone, Routable, PartialEq)]
enum Route {
    #[layout(HouseholdLayout)]
    #[route("/")]
    Home {},
    #[route("/todos")]
    Todos {},
    #[route("/shopping")]
    Shopping {},
}

const MAIN_CSS: Asset = asset!("/assets/main.css");

fn main() {
    #[cfg(feature = "server")]
    {
        let path = database::configured_path();
        if let Err(error) = database::initialize(&path) {
            eprintln!(
                "Failed to initialize database at {}: {error}",
                path.display()
            );
            std::process::exit(1);
        }
    }
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    rsx! {
        document::Link { rel: "stylesheet", href: MAIN_CSS }
        Router::<Route> {}
    }
}

#[component]
fn HouseholdLayout() -> Element {
    use_context_provider(|| Signal::new(todos::TaskStore::sample()));
    use_context_provider(|| Signal::new(shopping::ShoppingStore::sample()));
    let route = use_route::<Route>();
    rsx! {
        a { class: "skip-link", href: "#main", "Skip to content" }
        header { class: "site-header",
            div { class: "header-inner",
                Link { class: "brand", to: Route::Home {},
                    span { class: "brand-mark", Icon { kind: "home" } }
                    span { "lavemill" }
                }
                nav { class: "main-nav", aria_label: "Main navigation",
                    Link { to: Route::Home {}, class: if route == (Route::Home {}) { "nav-link active" } else { "nav-link" }, "Home" }
                    Link { to: Route::Todos {}, class: if route == (Route::Todos {}) { "nav-link active" } else { "nav-link" }, "To-dos" }
                    Link { to: Route::Shopping {}, class: if route == (Route::Shopping {}) { "nav-link active" } else { "nav-link" }, "Shopping" }
                }
            }
        }
        Outlet::<Route> {}
    }
}

#[component]
fn Home() -> Element {
    rsx! {
        main { id: "main", class: "dashboard", tabindex: "-1",
            document::Title { "Lavemill · Home" }
            h1 { class: "sr-only", "Home" }
            div { class: "dashboard-grid",
                TasksCard {}
                ShoppingCard {}
                CalendarCard {}
            }
        }
    }
}

#[component]
fn CardHeading(
    icon: &'static str,
    title: &'static str,
    id: &'static str,
    count: String,
) -> Element {
    rsx! {
        div { class: "card-heading",
            span { class: "card-icon", Icon { kind: icon } }
            div { class: "card-heading-copy",
                h2 { id, "{title}" }
            }
            span { class: "count-badge", "{count}" }
        }
    }
}

// Read-only status indicators rather than interactive checkboxes.
#[component]
fn StatusMark(done: bool) -> Element {
    let class = if done {
        "status-mark is-checked"
    } else {
        "status-mark"
    };
    rsx! {
        span { class, aria_hidden: "true",
            if done { Icon { kind: "check" } }
        }
        span { class: "sr-only", if done { "Completed: " } else { "Open: " } }
    }
}

#[component]
fn CalendarCard() -> Element {
    rsx! {
        section { class: "dashboard-card", aria_labelledby: "calendar-title",
            CardHeading { icon: "calendar", title: "Shared Calendar", id: "calendar-title", count: "This week" }
            div { class: "card-body agenda",
                section { class: "agenda-group", aria_labelledby: "agenda-today",
                    h3 { id: "agenda-today", class: "agenda-date", "Today", span { "23 Nov" } }
                    ul { class: "item-list",
                        AgendaRow { title: "Family dinner" }
                    }
                }
                section { class: "agenda-group", aria_labelledby: "agenda-saturday",
                    h3 { id: "agenda-saturday", class: "agenda-date", "Saturday", span { "28 Nov" } }
                    ul { class: "item-list",
                        AgendaRow { title: "Gramps visiting" }
                    }
                }
            }
        }
    }
}

#[component]
fn AgendaRow(title: &'static str) -> Element {
    rsx! {
        li { class: "agenda-row",
            span { class: "agenda-marker", aria_hidden: "true" }
            div { class: "row-copy",
                span { class: "row-title", "{title}" }
            }
        }
    }
}

#[component]
fn Icon(kind: &'static str) -> Element {
    let path = match kind {
        "home" => "M3 10 12 3l9 7M5 9v11h14V9M9 20v-7h6v7",
        "tasks" => "m3 6 2 2 3-4M11 6h10M3 13l2 2 3-4M11 13h10M4 20h3M11 20h10",
        "shopping" => "M3 3h2l3 12h11l2-8H6M9 20h.01M18 20h.01",
        "calendar" => {
            "M8 2v4M16 2v4M3 10h18M5 4h14a2 2 0 0 1 2 2v14H3V6a2 2 0 0 1 2-2M7 14h2M13 14h2M7 18h2"
        }
        "check" => "m5 12 4 4L19 6",
        _ => "",
    };
    rsx! {
        svg { width: "24", height: "24", view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "1.7", stroke_linecap: "round", stroke_linejoin: "round", "aria-hidden": "true", "focusable": "false",
            path { d: path }
        }
    }
}
