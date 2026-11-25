use dioxus::prelude::*;
#[cfg(feature = "server")]
mod accounts;
mod auth;
use auth::Login;
mod activity;
mod calendar;
use calendar::{Calendar, CalendarCard};
#[cfg(feature = "server")]
mod calendar_server;
#[cfg(feature = "server")]
mod household_time;
mod users;
use users::Users;
#[cfg(feature = "server")]
mod database;
#[cfg(feature = "server")]
mod steam;
#[cfg(feature = "server")]
mod tautulli;

#[cfg(feature = "server")]
mod todo_storage;
mod todos;
use todos::{TasksCard, Todos};
mod recipes;
mod shopping;
use recipes::{RecipeDetail, Recipes};
mod shopping_data;
#[cfg(feature = "server")]
mod shopping_storage;
use shopping::{Shopping, ShoppingCard};

#[derive(Debug, Clone, Routable, PartialEq)]
enum Route {
    #[route("/login?:error")]
    Login { error: Option<String> },
    #[layout(HouseholdLayout)]
    #[route("/")]
    Home {},
    #[route("/todos")]
    Todos {},
    #[route("/shopping")]
    Shopping {},
    #[route("/recipes")]
    Recipes {},
    #[route("/recipes/:id")]
    RecipeDetail { id: i64 },
    #[route("/calendar?:view&:date&:notice")]
    Calendar {
        view: Option<String>,
        date: Option<String>,
        notice: Option<String>,
    },
    #[route("/users")]
    Users {},
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
        let args: Vec<_> = std::env::args().skip(1).collect();
        if args.first().is_some_and(|arg| arg == "admin") {
            if let Err(error) = accounts::admin_command(&args[1..]) {
                eprintln!("{error}");
                std::process::exit(1);
            }
            return;
        }
        let origin = std::env::var("LAVEMILL_ORIGIN").unwrap_or_default();
        let router = auth::server::router(&origin).unwrap_or_else(|error| {
            eprintln!("{error}");
            std::process::exit(1);
        });
        dioxus::serve(move || {
            let router = router.clone();
            async move { Ok(router) }
        });
    }
    #[cfg(not(feature = "server"))]
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
    use_context_provider(|| Signal::new(None::<shopping::RemovedItem>));
    let route = use_route::<Route>();
    let viewer = use_server_future(auth::current_viewer)?;
    let viewer = viewer.read();
    let is_admin = match viewer.as_ref() {
        Some(Ok(Some(viewer))) => viewer.is_admin,
        Some(Ok(None)) => {
            return rsx! { main { class: "dashboard", a { href: "/login", "Sign in to continue" } } }
        }
        Some(Err(_)) => {
            return rsx! { main { class: "dashboard", p { role: "alert", "Could not check your session. Reload to try again." } } }
        }
        None => return rsx! { main { class: "dashboard", p { "Checking your session…" } } },
    };
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
                    Link { to: Route::Recipes {}, class: if matches!(route, Route::Recipes {} | Route::RecipeDetail { .. }) { "nav-link active" } else { "nav-link" }, "Recipes" }
                    Link { to: Route::Calendar { view: None, date: None, notice: None }, class: if matches!(route, Route::Calendar { .. }) { "nav-link active" } else { "nav-link" }, "Calendar" }
                    if is_admin {
                        Link { to: Route::Users {}, class: if route == (Route::Users {}) { "nav-link active" } else { "nav-link" }, "Users" }
                    }
                }
                form { action: "/auth/logout", method: "post",
                    button { class: "undo-button", r#type: "submit", "Sign out" }
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
                activity::ActivityCard {}
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
