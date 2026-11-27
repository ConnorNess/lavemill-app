use crate::Route;
use dioxus::prelude::*;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Theme {
    Mint,
    Orange,
    Purple,
    Blue,
    Red,
}

impl Theme {
    fn for_route(route: &Route) -> Option<Self> {
        match route {
            Route::Home {} => None,
            Route::Shopping {} => Some(Self::Orange),
            Route::Calendar { .. } => Some(Self::Purple),
            Route::Users {} => Some(Self::Blue),
            Route::Recipes {}
            | Route::NewRecipe {}
            | Route::EditRecipe { .. }
            | Route::RecipeDetail { .. } => Some(Self::Red),
            Route::Login { .. } | Route::Todos {} => Some(Self::Mint),
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Mint => "mint",
            Self::Orange => "orange",
            Self::Purple => "purple",
            Self::Blue => "blue",
            Self::Red => "red",
        }
    }
}

#[derive(Clone, Copy)]
struct ActiveTheme(Signal<Theme>);

impl ActiveTheme {
    fn select(mut self, theme: Theme) {
        if *self.0.peek() != theme {
            self.0.set(theme);
        }
    }
}

pub(crate) fn use_household_theme() -> Theme {
    let route_theme = Theme::for_route(&use_route::<Route>());
    let active = use_context_provider(|| ActiveTheme(Signal::new(Theme::Mint)));

    use_effect(use_reactive!(|route_theme| {
        if let Some(theme) = route_theme {
            active.select(theme);
        }
    }));

    route_theme.unwrap_or_else(|| (active.0)())
}

pub(crate) fn use_theme_preview(destination: &Route) -> impl Fn() + Copy {
    let on_home = matches!(use_route::<Route>(), Route::Home {});
    let theme = Theme::for_route(destination).filter(|_| on_home);
    let active: ActiveTheme = use_context();

    move || {
        if let Some(theme) = theme {
            active.select(theme);
        }
    }
}
