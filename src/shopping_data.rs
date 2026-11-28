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
    let viewer = crate::auth::require_viewer()?;
    crate::auth::server::run_blocking(move || {
        let mut db = crate::auth::server::connection()?;
        match viewer {
            crate::auth::Viewer::Member(_) => crate::shopping_storage::list(&mut db),
            crate::auth::Viewer::Guest => crate::shopping_storage::snapshot(&db),
        }
    })
    .await
    .map_err(ServerFnError::new)
}

#[post("/api/shopping/change")]
pub(crate) async fn change_shopping(
    change: ShoppingChange,
) -> Result<Option<String>, ServerFnError> {
    let actor = crate::auth::require_member()?.id;
    crate::auth::server::run_blocking(move || {
        crate::shopping_storage::apply(&mut crate::auth::server::connection()?, actor, change)
    })
    .await
    .map(|message| message.map(str::to_owned))
    .map_err(ServerFnError::new)
}
