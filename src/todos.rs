use super::{CardHeading, Route, StatusMark};
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, PartialEq, Serialize)]
pub(crate) struct Task {
    pub id: i64,
    pub title: String,
    pub description: Option<String>,
    pub due: String,
    pub reporter: String,
    pub assignee_id: Option<i64>,
    pub assignee: Option<String>,
    pub status: String,
    pub can_reopen: bool,
}

#[derive(Clone, Deserialize, PartialEq, Serialize)]
pub(crate) struct Member {
    pub id: i64,
    pub name: String,
}

#[derive(Clone, Deserialize, PartialEq, Serialize)]
pub(crate) struct Tasks {
    pub today: String,
    pub tasks: Vec<Task>,
    pub members: Vec<Member>,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
pub(crate) enum Repeat {
    Once,
    Daily,
    Weekly,
    Monthly,
}

#[derive(Deserialize, Serialize)]
pub(crate) enum Change {
    Create {
        title: String,
        description: String,
        due: String,
        assignee: Option<i64>,
        repeat: Repeat,
    },
    Describe {
        id: i64,
        description: String,
    },
    Assign {
        id: i64,
        assignee: Option<i64>,
    },
    Complete {
        id: i64,
    },
    Reopen {
        id: i64,
    },
}

#[get("/api/todos")]
async fn list_tasks() -> Result<Tasks, ServerFnError> {
    crate::auth::require_member()?;
    crate::auth::server::run_blocking(|| {
        let today = crate::todo_storage::household_date()?;
        crate::todo_storage::list(&mut crate::auth::server::connection()?, today)
    })
    .await
    .map_err(ServerFnError::new)
}

#[post("/api/todos/change")]
async fn change_task(change: Change) -> Result<Option<String>, ServerFnError> {
    let actor = crate::auth::require_member()?.id;
    let result = crate::auth::server::run_blocking(move || {
        let today = crate::todo_storage::household_date()?;
        crate::todo_storage::apply(
            &mut crate::auth::server::connection()?,
            today,
            actor,
            change,
        )
    })
    .await;
    Ok(result.err().map(str::to_owned))
}

async fn save(change: Change) -> Result<(), String> {
    match change_task(change).await {
        Ok(None) => Ok(()),
        Ok(Some(message)) => Err(message),
        Err(_) => Err("Could not save the task. Try again.".into()),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Group {
    Overdue,
    Today,
    Upcoming,
    Completed,
    Failed,
}

impl Group {
    fn title(self) -> &'static str {
        match self {
            Self::Overdue => "Overdue",
            Self::Today => "Today",
            Self::Upcoming => "Upcoming",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
        }
    }

    fn contains(self, task: &Task, today: &str) -> bool {
        match self {
            Self::Completed => task.status == "completed",
            Self::Failed => task.status == "failed",
            Self::Overdue => task.status == "open" && task.due.as_str() < today,
            Self::Today => task.status == "open" && task.due == today,
            Self::Upcoming => task.status == "open" && task.due.as_str() > today,
        }
    }
}

#[component]
pub(super) fn TasksCard() -> Element {
    let data = use_server_future(list_tasks)?;
    let result = data.read();
    let mut tasks = Vec::new();
    if let Some(Ok(data)) = result.as_ref() {
        tasks.extend(
            data.tasks
                .iter()
                .filter(|task| task.due <= data.today && task.status != "failed"),
        );
        tasks.sort_by_key(|task| task.status == "completed");
    }
    let open = tasks.iter().filter(|task| task.status == "open").count();
    rsx! {
        section { class: "dashboard-card", aria_labelledby: "tasks-title",
            CardHeading { icon: "tasks", title: "Today’s To-dos", id: "tasks-title", count: format!("{open} open") }
            div { class: "card-body",
                match result.as_ref() {
                    Some(Ok(data)) => rsx! {
                        if tasks.is_empty() { p { class: "empty-tasks", "No tasks due today." } }
                        ul { class: "item-list task-list",
                            for task in tasks.into_iter().take(4) {
                                li { key: "{task.id}", class: if task.status == "completed" { "list-row is-done" } else { "list-row" },
                                    StatusMark { done: task.status == "completed" }
                                    div { class: "row-copy",
                                        span { class: "row-title", "{task.title}" }
                                        span { class: "row-note",
                                            if task.status == "completed" { "Completed" }
                                            else if task.due < data.today { "Overdue · {task.due}" }
                                            else { "Today" }
                                        }
                                    }
                                    span { class: "assignee", {task.assignee.as_deref().unwrap_or("Unassigned")} }
                                }
                            }
                        }
                    },
                    Some(Err(_)) => rsx! { p { role: "alert", "Could not load tasks. Reload to try again." } },
                    None => rsx! { p { "Loading tasks…" } },
                }
            }
            Link { class: "card-page-link", to: Route::Todos {}, "View all to-dos" }
        }
    }
}

#[component]
pub(super) fn Todos() -> Element {
    let data = use_server_future(list_tasks)?;
    use_context_provider(|| data);
    let result = data.read();
    rsx! {
        document::Title { "Lavemill · To-dos" }
        main { id: "main", class: "dashboard todos-page", tabindex: "-1",
            div { class: "todos-heading", h1 { "To-dos" } }
            match result.as_ref() {
                Some(Ok(data)) => rsx! {
                    div { class: "todos-layout",
                        div { class: "task-groups",
                            for group in [Group::Overdue, Group::Today, Group::Upcoming, Group::Completed, Group::Failed] {
                                TaskGroup {
                                    key: "{group.title()}",
                                    group,
                                    tasks: data.tasks.iter().filter(|task| group.contains(task, &data.today)).cloned().collect::<Vec<_>>(),
                                    members: data.members.clone(),
                                }
                            }
                        }
                        AddTaskForm { today: data.today.clone(), members: data.members.clone() }
                    }
                },
                Some(Err(_)) => rsx! { p { role: "alert", "Could not load tasks. Reload to try again." } },
                None => rsx! { p { "Loading tasks…" } },
            }
        }
    }
}

#[component]
fn TaskGroup(group: Group, tasks: Vec<Task>, members: Vec<Member>) -> Element {
    rsx! {
        section { class: "task-group", aria_label: group.title(),
            div { class: "task-group-heading",
                h2 { class: if group == Group::Overdue { "overdue" } else { "" }, "{group.title()}" }
                span { class: "count-badge", "{tasks.len()}" }
            }
            if tasks.is_empty() {
                p { class: "empty-tasks", "No {group.title().to_lowercase()} tasks." }
            } else {
                ul { class: "item-list",
                    for task in tasks { EditableTaskRow { key: "{task.id}", task, members: members.clone() } }
                }
            }
        }
    }
}

#[component]
fn EditableTaskRow(task: Task, members: Vec<Member>) -> Element {
    let mut data = use_context::<Resource<Result<Tasks, ServerFnError>>>();
    let mut error = use_signal(String::new);
    let mut pending = use_signal(|| false);
    let mut draft = use_signal(|| None::<String>);
    let id = task.id;
    let completed = task.status == "completed";
    let open = task.status == "open";
    let submit = Callback::new(move |change: Change| {
        if pending() {
            return;
        }
        pending.set(true);
        spawn(async move {
            error.set(String::new());
            let editing = matches!(&change, Change::Describe { .. });
            match save(change).await {
                Ok(()) => {
                    if editing {
                        draft.set(None);
                    }
                    data.restart();
                }
                Err(message) => error.set(message),
            }
            pending.set(false);
        });
    });
    let assignee = task
        .assignee_id
        .map(|id| id.to_string())
        .unwrap_or_default();
    rsx! {
        li { class: if completed { "editable-task is-done" } else { "editable-task" },
            if open || completed {
                label { class: "task-check-target",
                    input { r#type: "checkbox", checked: completed, disabled: pending() || (completed && !task.can_reopen),
                    aria_label: if completed { format!("Reopen {}", task.title) } else { format!("Complete {}", task.title) },
                    onchange: move |event| {
                        submit.call(if event.checked() { Change::Complete { id } } else { Change::Reopen { id } });
                    }
                    }
                }
            }
            div { class: "row-copy",
                span { class: "row-title", "{task.title}" }
                time { class: "row-note", datetime: "{task.due}", "Due {task.due}" }
                span { class: "row-note", "Reported by {task.reporter}" }
                if let Some(description) = task.description.as_ref() { p { "{description}" } }
                if let Some(description) = draft() {
                    form { onsubmit: move |event| {
                        event.prevent_default();
                        if let Some(description) = draft() {
                            submit.call(Change::Describe { id, description });
                        }
                    },
                        label {
                            "Description"
                            textarea { value: "{description}", disabled: pending(), oninput: move |event| draft.set(Some(event.value())) }
                        }
                        button { r#type: "submit", disabled: pending(), "Save description" }
                    }
                } else if open {
                    button { r#type: "button", class: "undo-button", disabled: pending(),
                        onclick: move |_| draft.set(Some(task.description.clone().unwrap_or_default())),
                        "Edit description"
                    }
                }
                if !error().is_empty() { p { class: "form-error", role: "alert", "{error}" } }
            }
            if !open {
                span { class: "completed-assignee", {task.assignee.as_deref().unwrap_or("Unassigned")} }
                if task.can_reopen {
                    button { class: "undo-button", r#type: "button", disabled: pending(), aria_label: "Undo completion of {task.title}",
                        onclick: move |_| submit.call(Change::Reopen { id }), "Undo" }
                }
            } else {
                label { class: "task-assignee-field",
                    span { class: "sr-only", "Assign {task.title} to" }
                    MemberSelect { value: assignee, members, disabled: pending(),
                        on_change: move |assignee| submit.call(Change::Assign { id, assignee }) }
                }
            }
        }
    }
}

#[component]
fn MemberSelect(
    value: String,
    members: Vec<Member>,
    disabled: bool,
    on_change: EventHandler<Option<i64>>,
) -> Element {
    rsx! {
        select { value: "{value}", disabled, onchange: move |event| on_change.call(event.value().parse().ok()),
            option { value: "", "Unassigned" }
            for member in members { option { key: "{member.id}", value: "{member.id}", "{member.name}" } }
        }
    }
}

#[component]
fn AddTaskForm(today: String, members: Vec<Member>) -> Element {
    let mut data = use_context::<Resource<Result<Tasks, ServerFnError>>>();
    let mut title = use_signal(String::new);
    let mut due = use_signal(|| today);
    let mut description = use_signal(String::new);
    let mut assignee = use_signal(|| None::<i64>);
    let mut repeat = use_signal(|| Repeat::Once);
    let mut error = use_signal(String::new);
    let mut pending = use_signal(|| false);
    let mut feedback = use_signal(String::new);
    let selected_assignee = assignee().map(|id| id.to_string()).unwrap_or_default();
    rsx! {
        section { class: "add-task-panel", aria_labelledby: "add-task-heading",
            h2 { id: "add-task-heading", "Add a task" }
            form { onsubmit: move |event| {
                event.prevent_default();
                if pending() {
                    return;
                }
                pending.set(true);
                spawn(async move {
                error.set(String::new());
                feedback.set(String::new());
                let task_title = title();
                match save(Change::Create { title: task_title.clone(), description: description(), due: due(), assignee: assignee(), repeat: repeat() }).await {
                    Ok(()) => {
                        feedback.set(format!("Added: {}", task_title.trim()));
                        title.set(String::new());
                        description.set(String::new());
                        data.restart();
                    }
                    Err(message) => error.set(message),
                }
                pending.set(false);
                });
            },
                label { r#for: "new-task-title", "Task" }
                input { id: "new-task-title", name: "title", required: true, value: "{title}", disabled: pending(),
                    oninput: move |event| title.set(event.value()) }
                label { r#for: "new-task-description", "Description (optional)" }
                textarea { id: "new-task-description", value: "{description}", disabled: pending(), oninput: move |event| description.set(event.value()) }
                label { r#for: "new-task-date", "Due date" }
                input { id: "new-task-date", name: "due", r#type: "date", required: true, min: "0001-01-01", max: "9999-12-31", value: "{due}", disabled: pending(), oninput: move |event| due.set(event.value()) }
                label { r#for: "new-task-repeat", "Repeat" }
                select { id: "new-task-repeat", disabled: pending(), onchange: move |event| repeat.set(match event.value().as_str() {
                    "daily" => Repeat::Daily,
                    "weekly" => Repeat::Weekly,
                    "monthly" => Repeat::Monthly,
                    _ => Repeat::Once,
                }),
                    option { value: "once", "Once" }
                    option { value: "daily", "Daily" }
                    option { value: "weekly", "Weekly" }
                    option { value: "monthly", "Monthly" }
                }
                label {
                    "Assigned to"
                    MemberSelect { value: selected_assignee, members, disabled: pending(), on_change: move |value| assignee.set(value) }
                }
                if !error().is_empty() { p { class: "form-error", role: "alert", "{error}" } }
                button { class: "primary-button", r#type: "submit", disabled: pending(), "Add task" }
                p { class: "form-feedback", role: "status", "{feedback}" }
            }
        }
    }
}
