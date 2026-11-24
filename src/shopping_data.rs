use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, PartialEq, Serialize)]
pub(crate) struct ShoppingItem {
    pub id: i64,
    pub name: String,
    pub quantity: Option<String>,
    pub checked: bool,
}

#[derive(Deserialize, Serialize)]
pub(crate) enum ShoppingChange {
    Add {
        name: String,
        quantity: String,
    },
    Edit {
        id: i64,
        name: String,
        quantity: String,
    },
    SetChecked {
        id: i64,
        checked: bool,
    },
    Remove {
        id: i64,
    },
    Restore {
        id: i64,
    },
}

#[get("/api/shopping")]
pub(crate) async fn list_shopping() -> Result<Vec<ShoppingItem>, ServerFnError> {
    require_member()?;
    crate::auth::server::run_blocking(|| {
        crate::shopping_storage::list(&mut crate::auth::server::connection()?)
    })
    .await
    .map_err(ServerFnError::new)
}

#[post("/api/shopping/change")]
pub(crate) async fn change_shopping(
    change: ShoppingChange,
) -> Result<Option<String>, ServerFnError> {
    let actor = require_member()?;
    crate::auth::server::run_blocking(move || {
        crate::shopping_storage::apply(&mut crate::auth::server::connection()?, actor, change)
    })
    .await
    .map(|message| message.map(str::to_owned))
    .map_err(ServerFnError::new)
}

#[cfg(feature = "server")]
fn require_member() -> Result<i64, ServerFnError> {
    dioxus::fullstack::FullstackContext::current()
        .and_then(|context| context.extension::<crate::auth::Viewer>())
        .map(|viewer| viewer.id)
        .ok_or_else(|| ServerFnError::new("Sign in to continue."))
}
