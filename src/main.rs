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
mod card_navigation;
mod homelab;
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
use recipes::{EditRecipe, NewRecipe, RecipeDetail, Recipes};
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
    #[route("/recipes/new")]
    NewRecipe {},
    #[route("/recipes/:id/edit")]
    EditRecipe { id: i64 },
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
const FONT_ASSETS: Asset = asset!("/assets/fonts", AssetOptions::folder());

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
        document::Link { rel: "stylesheet", href: "{FONT_ASSETS}/fonts.css" }
        document::Link { rel: "stylesheet", href: MAIN_CSS }
        Router::<Route> {}
    }
}

#[component]
fn HouseholdLayout() -> Element {
    use_context_provider(|| Signal::new(None::<shopping::RemovedItem>));
    let mut account_open = use_signal(|| false);
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
                card_navigation::HomeLink {}
                Link { class: "header-tile recipes-link", to: Route::Recipes {}, "data-home-return-anchor": "recipes",
                    Icon { kind: "recipes" }
                    span { "Recipes" }
                }
                div { class: if account_open() { "account-menu is-open" } else { "account-menu" },
                    onkeydown: move |event| {
                        if event.key() == Key::Escape {
                            account_open.set(false);
                        }
                    },
                    if is_admin {
                        button { class: "account-disclosure", r#type: "button",
                            aria_label: "Account options", aria_expanded: account_open(),
                            onclick: move |_| account_open.toggle(),
                        }
                        Link { class: "account-icon", to: Route::Users {}, aria_label: "Users", title: "Users", "data-home-return-anchor": "users",
                            Icon { kind: "users" }
                        }
                    } else {
                        button { class: "account-icon", r#type: "button",
                            aria_label: "Account options", aria_expanded: account_open(),
                            onclick: move |_| account_open.toggle(),
                            Icon { kind: "users" }
                        }
                    }
                    form { class: "account-signout", action: "/auth/logout", method: "post",
                        button { r#type: "submit", "Sign out" }
                    }
                }
            }
        }
        Outlet::<Route> {}
    }
}

#[component]
fn Home() -> Element {
    rsx! {
        main { id: "main", class: "dashboard home-dashboard", tabindex: "-1",
            document::Title { "Lavemill · Home" }
            h1 { class: "sr-only", "Home" }
            div { class: "home-dashboard-layout",
                div { class: "home-dashboard-content",
                    activity::ActivitySection {}
                    div { class: "dashboard-grid",
                        TasksCard {}
                        ShoppingCard {}
                        CalendarCard {}
                    }
                }
                homelab::Shortcuts {}
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

#[component]
fn Icon(kind: &'static str) -> Element {
    let path = match kind {
        "home" => "M3 10 12 3l9 7M5 9v11h14V9M9 20v-7h6v7",
        "tasks" => "m3 6 2 2 3-4M11 6h10M3 13l2 2 3-4M11 13h10M4 20h3M11 20h10",
        "shopping" => "M3 3h2l3 12h11l2-8H6M9 20h.01M18 20h.01",
        "calendar" => {
            "M8 2v4M16 2v4M3 10h18M5 4h14a2 2 0 0 1 2 2v14H3V6a2 2 0 0 1 2-2M7 14h2M13 14h2M7 18h2"
        }
        "users" => "M20 21a8 8 0 0 0-16 0M12 13a4 4 0 1 0 0-8 4 4 0 0 0 0 8Z",
        "recipes" => "M12 7c-3-2-6-2-9-1v13c3-1 6-1 9 1 3-2 6-2 9-1V6c-3-1-6-1-9 1Zm0 0v13",
        "Seerr" => {
            "M21 12s-3.5 6-9 6-9-6-9-6 3.5-6 9-6 9 6 9 6ZM15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z"
        }
        "Plex" => "m9 5 6 7-6 7",
        "Komga" => "M12 6c-3-2-6-2-9-1v13c3-1 6-1 9 1 3-2 6-2 9-1V5c-3-1-6-1-9 1Zm0 0v13",
        "Foundry" => "m12 3 8 5v8l-8 5-8-5V8l8-5Zm0 0 5 13H7l5-13ZM4 8l3 8 5 5 5-5 3-8",
        "BookStack" => "M4 5h14v4H4zM6 9h14v5H6zM4 14h14v5H4z",
        _ => "",
    };
    rsx! {
        svg { width: "24", height: "24", view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "1.7", stroke_linecap: "round", stroke_linejoin: "round", "aria-hidden": "true", "focusable": "false",
            path { d: path }
        }
    }
}
