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

#[derive(Clone, Copy)]
struct HeaderPreview {
    pointer: Signal<Option<(Route, Theme)>>,
    focus: Signal<Option<(Route, Theme)>>,
}

#[derive(Clone, Copy)]
pub(crate) enum PreviewSource {
    Pointer,
    Focus,
}

impl ActiveTheme {
    fn select(mut self, theme: Theme) {
        if *self.0.peek() != theme {
            self.0.set(theme);
        }
    }
}

pub(crate) fn use_household_theme() -> Theme {
    let route = use_route::<Route>();
    let route_theme = Theme::for_route(&route);
    let active = use_context_provider(|| ActiveTheme(Signal::new(Theme::Mint)));
    let header = use_context_provider(|| HeaderPreview {
        pointer: Signal::new(None),
        focus: Signal::new(None),
    });

    use_effect(use_reactive!(|route| {
        if let Some(theme) = Theme::for_route(&route) {
            active.select(theme);
        }
        for mut preview in [header.pointer, header.focus] {
            let stale = preview
                .peek()
                .as_ref()
                .is_some_and(|(page, _)| page != &route);
            if stale {
                preview.set(None);
            }
        }
    }));

    for preview in [header.pointer, header.focus] {
        if let Some((_, theme)) = preview.read().as_ref().filter(|(page, _)| page == &route) {
            return *theme;
        }
    }
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

pub(crate) fn use_header_theme_preview(destination: &Route) -> Callback<(PreviewSource, bool)> {
    let route = use_route::<Route>();
    let theme = Theme::for_route(destination);
    let active: ActiveTheme = use_context();
    let header: HeaderPreview = use_context();

    Callback::new(move |(source, entering)| {
        let Some(theme) = theme else { return };
        if matches!(route, Route::Home {}) {
            if entering {
                active.select(theme);
            }
            return;
        }
        let mut preview = match source {
            PreviewSource::Pointer => header.pointer,
            PreviewSource::Focus => header.focus,
        };
        if entering {
            preview.set(Some((route.clone(), theme)));
        } else if preview
            .peek()
            .as_ref()
            .is_some_and(|(page, color)| page == &route && *color == theme)
        {
            preview.set(None);
        }
    })
}
