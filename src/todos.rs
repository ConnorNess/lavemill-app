use super::{CardHeading, Route};
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
    pub assignee_first_name: Option<String>,
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

impl Repeat {
    fn value(self) -> &'static str {
        match self {
            Self::Once => "once",
            Self::Daily => "daily",
            Self::Weekly => "weekly",
            Self::Monthly => "monthly",
        }
    }
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
    Rename {
        id: i64,
        title: String,
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
    let mut refreshed = use_signal(|| None::<Tasks>);
    let mut pending = use_signal(|| false);
    let mut refresh_failed = use_signal(|| false);
    let mut error = use_signal(String::new);
    let mut failed_checkbox = use_signal(|| (None::<i64>, 0));
    let result = data.read();
    let refreshed_data = refreshed.read();
    let current = refreshed_data
        .as_ref()
        .or_else(|| result.as_ref()?.as_ref().ok());
    let mut tasks = Vec::new();
    if let Some(data) = current {
        tasks.extend(data.tasks.iter());
        tasks.sort_by_key(|task| match task.status.as_str() {
            "open" => 0,
            "completed" => 1,
            _ => 2,
        });
    }
    let open = tasks.iter().filter(|task| task.status == "open").count();
    let toggle = Callback::new(move |(id, checked): (i64, bool)| {
        if pending() || refresh_failed() {
            return;
        }
        pending.set(true);
        error.set(String::new());
        spawn(async move {
            let change = if checked {
                Change::Complete { id }
            } else {
                Change::Reopen { id }
            };
            let reset_checkbox = match save(change).await {
                Ok(()) => match list_tasks().await {
                    Ok(tasks) => {
                        refreshed.set(Some(tasks));
                        false
                    }
                    Err(_) => {
                        refresh_failed.set(true);
                        true
                    }
                },
                Err(message) => {
                    error.set(message);
                    true
                }
            };
            if reset_checkbox {
                let (_, revision) = failed_checkbox();
                failed_checkbox.set((Some(id), revision + 1));
            }
            pending.set(false);
        });
    });
    rsx! {
        crate::card_navigation::HomeCard {
            to: Route::Todos {}, label: "Open to-dos", page: "todos", heading: "tasks-title", scrollable: true,
            CardHeading { icon: "tasks", title: "To-dos", id: "tasks-title", count: format!("{open} open") }
            div { class: "card-body",
                if refresh_failed() {
                    p { class: "form-error", role: "alert", "Your change was saved, but the to-dos could not refresh. Reload the page to try again." }
                } else if !error().is_empty() {
                    p { class: "form-error", role: "alert", "{error}" }
                }
                if let Some(data) = current {
                    if tasks.is_empty() { p { class: "empty-tasks", "No to-dos yet." } }
                    ul { class: "item-list task-list",
                        for task in tasks {
                            TaskPreviewRow { key: "{task.id}", task: task.clone(),
                                today: data.today.clone(), disabled: pending() || refresh_failed(),
                                failed_checkbox: failed_checkbox(), on_toggle: toggle,
                            }
                        }
                    }
                } else if matches!(result.as_ref(), Some(Err(_))) {
                    p { role: "alert", "Could not load tasks. Reload to try again." }
                } else {
                    p { "Loading tasks…" }
                }
            }
        }
    }
}

#[component]
fn TaskPreviewRow(
    task: Task,
    today: String,
    disabled: bool,
    failed_checkbox: (Option<i64>, usize),
    on_toggle: EventHandler<(i64, bool)>,
) -> Element {
    let id = task.id;
    let completed = task.status == "completed";
    let (failed_id, revision) = failed_checkbox;
    let revision = if failed_id == Some(id) { revision } else { 0 };
    rsx! {
        li { class: if completed { "list-row is-done" } else { "list-row" },
            "data-task-transition-id": "{id}",
            if task.status == "open" || completed {
                label { class: "task-check-target",
                    // A keyed child lets a rejected change replace the browser's toggled input.
                    for revision in [revision] {
                        input { key: "{revision}", r#type: "checkbox", checked: completed,
                            disabled: disabled || (completed && !task.can_reopen),
                            aria_label: if completed { format!("Reopen {}", task.title) } else { format!("Complete {}", task.title) },
                            onchange: move |event| on_toggle.call((id, event.checked())),
                        }
                    }
                }
            }
            div { class: "row-copy",
                span { class: "row-title", "{task.title}" }
                span { class: "row-note",
                    if completed { "Completed" }
                    else if task.status == "failed" { "Failed · {task.due}" }
                    else if task.due < today { "Overdue · {task.due}" }
                    else if task.due == today { "Today" }
                    else { "Due {task.due}" }
                }
            }
            span { class: "task-preview-assignee", {task.assignee_first_name.as_deref().unwrap_or("Unassigned")} }
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
        main { id: "main", class: "dashboard interior-page todos-page", tabindex: "-1",
            div { class: "todos-heading", h1 { "To-dos" } }
            match result.as_ref() {
                Some(Ok(data)) => rsx! {
                    div { class: "todos-layout",
                        div { class: "task-groups", role: "region", aria_label: "Task list", tabindex: "0",
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
        section { class: "task-group surface-panel", aria_label: group.title(),
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

#[derive(Clone, Copy, PartialEq)]
enum TaskField {
    Title,
    Description,
}

#[derive(Clone)]
struct TaskDraft {
    field: TaskField,
    value: String,
}

#[component]
fn EditableTaskRow(task: Task, members: Vec<Member>) -> Element {
    let mut data = use_context::<Resource<Result<Tasks, ServerFnError>>>();
    let mut error = use_signal(String::new);
    let mut pending = use_signal(|| false);
    let mut draft = use_signal(|| None::<TaskDraft>);
    let mut focus_after_edit = use_signal(|| None::<TaskField>);
    let id = task.id;
    let initial_title = task.title.clone();
    let initial_description = task.description.clone().unwrap_or_default();
    let completed = task.status == "completed";
    let open = task.status == "open";
    let active_draft = draft();
    let finish_edit = Callback::new(move |()| {
        focus_after_edit.set(draft.peek().as_ref().map(|edit| edit.field));
        draft.set(None);
        error.set(String::new());
    });
    let submit = Callback::new(move |change: Change| {
        if pending() {
            return;
        }
        pending.set(true);
        spawn(async move {
            error.set(String::new());
            let editing = matches!(&change, Change::Rename { .. } | Change::Describe { .. });
            match save(change).await {
                Ok(()) => {
                    if editing {
                        finish_edit.call(());
                    }
                    data.restart();
                }
                Err(message) => error.set(message),
            }
            pending.set(false);
        });
    });
    let save_edit = Callback::new(move |()| {
        if pending() {
            return;
        }
        let Some(TaskDraft { field, value }) = draft() else {
            return;
        };
        let change = match field {
            TaskField::Title if value.trim().is_empty() => {
                error.set("Enter a task title.".into());
                return;
            }
            TaskField::Title => Change::Rename { id, title: value },
            TaskField::Description => Change::Describe {
                id,
                description: value,
            },
        };
        submit.call(change);
    });
    let cancel_edit = Callback::new(move |()| {
        if !pending() {
            finish_edit.call(());
        }
    });
    let begin_edit = Callback::new(move |edit: TaskDraft| {
        focus_after_edit.set(None);
        error.set(String::new());
        draft.set(Some(edit));
    });
    let assignee = task
        .assignee_id
        .map(|id| id.to_string())
        .unwrap_or_default();
    rsx! {
        li { class: if completed { "editable-task is-done" } else { "editable-task" },
            "data-task-transition-id": "{id}",
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
                if let Some(TaskDraft { field: TaskField::Title, value }) = active_draft.as_ref() {
                    TaskInlineEditor { field: TaskField::Title, value: value.clone(), pending: pending(),
                        on_change: move |value| draft.set(Some(TaskDraft { field: TaskField::Title, value })),
                        on_save: save_edit, on_cancel: cancel_edit,
                    }
                } else {
                    div { class: "task-edit-line",
                        if open {
                            TaskPencil { label: format!("Edit title of {}", task.title),
                                disabled: pending() || active_draft.is_some(),
                                restore_focus: focus_after_edit() == Some(TaskField::Title),
                                on_edit: move |_| begin_edit.call(TaskDraft { field: TaskField::Title, value: initial_title.clone() }),
                            }
                        }
                        span { class: "row-title", "{task.title}" }
                    }
                }
                if let Some(TaskDraft { field: TaskField::Description, value }) = active_draft.as_ref() {
                    TaskInlineEditor { field: TaskField::Description, value: value.clone(), pending: pending(),
                        on_change: move |value| draft.set(Some(TaskDraft { field: TaskField::Description, value })),
                        on_save: save_edit, on_cancel: cancel_edit,
                    }
                } else if open || task.description.is_some() {
                    div { class: "task-edit-line task-description-line",
                        if open {
                            TaskPencil { label: format!("Edit description of {}", task.title),
                                disabled: pending() || active_draft.is_some(),
                                restore_focus: focus_after_edit() == Some(TaskField::Description),
                                on_edit: move |_| begin_edit.call(TaskDraft { field: TaskField::Description, value: initial_description.clone() }),
                            }
                        }
                        if let Some(description) = task.description.as_ref() {
                            p { class: "task-description", "{description}" }
                        } else {
                            span { class: "row-note", "No description" }
                        }
                    }
                }
                time { class: "row-note", datetime: "{task.due}", "Due {task.due}" }
                span { class: "row-note", "Reported by {task.reporter}" }
                if !error().is_empty() { p { class: "form-error", role: "alert", "{error}" } }
            }
            if !open {
                span { class: "completed-assignee", {task.assignee.as_deref().unwrap_or("Unassigned")} }
                if task.can_reopen {
                    button { class: "undo-button", r#type: "button", disabled: pending(), aria_label: "Undo completion of {task.title}",
                        onclick: move |_| submit.call(Change::Reopen { id }), "Undo" }
                }
            } else {
                div { class: "task-assignee-field",
                    MemberSelect { id: format!("task-assignee-{id}"), label: format!("Assign {} to", task.title),
                        value: assignee, members, disabled: pending(),
                        on_change: move |assignee| submit.call(Change::Assign { id, assignee }) }
                }
            }
        }
    }
}

#[component]
fn TaskPencil(
    label: String,
    disabled: bool,
    restore_focus: bool,
    on_edit: EventHandler<MouseEvent>,
) -> Element {
    rsx! {
        button { class: "task-icon-button", r#type: "button", aria_label: "{label}", disabled,
            onclick: on_edit,
            onmounted: move |event: MountedEvent| async move {
                if restore_focus {
                    focus_task_control(event).await;
                }
            },
            crate::Icon { kind: "pencil" }
        }
    }
}

async fn focus_task_control(event: MountedEvent) {
    let _ = event.set_focus(true).await;
}

#[component]
fn TaskInlineEditor(
    field: TaskField,
    value: String,
    pending: bool,
    on_change: EventHandler<String>,
    on_save: EventHandler<()>,
    on_cancel: EventHandler<()>,
) -> Element {
    use dioxus::html::input_data::keyboard_types::Modifiers;

    let (icon, label, save_label, cancel_label) = match field {
        TaskField::Title => ("tasks", "Task title", "Save title", "Cancel title edit"),
        TaskField::Description => (
            "description",
            "Task description",
            "Save description",
            "Cancel description edit",
        ),
    };
    rsx! {
        form { class: "task-edit-form",
            onsubmit: move |event| {
                event.prevent_default();
                on_save.call(());
            },
            onkeydown: move |event| {
                if event.key() == Key::Escape && !event.is_composing() {
                    event.prevent_default();
                    event.stop_propagation();
                    on_cancel.call(());
                }
            },
            TodoTextField { icon,
                if field == TaskField::Title {
                    input { r#type: "text", aria_label: label, aria_required: "true", value: "{value}", readonly: pending,
                        onmounted: focus_task_control,
                        oninput: move |event| on_change.call(event.value()),
                    }
                } else {
                    textarea { aria_label: label, value: "{value}", readonly: pending,
                        onmounted: focus_task_control,
                        oninput: move |event| on_change.call(event.value()),
                        onkeydown: move |event| {
                            if event.key() == Key::Enter && !event.is_composing()
                                && !event.modifiers().contains(Modifiers::SHIFT)
                            {
                                event.prevent_default();
                                on_save.call(());
                            }
                        },
                    }
                }
            }
            div { class: "task-edit-actions",
                button { class: "task-icon-button task-save-edit", r#type: "submit", aria_label: save_label, disabled: pending,
                    crate::Icon { kind: "check" }
                }
                button { class: "task-icon-button", r#type: "button", aria_label: cancel_label, disabled: pending,
                    onclick: move |_| on_cancel.call(()), crate::Icon { kind: "close" }
                }
            }
        }
    }
}

#[component]
fn MemberSelect(
    id: String,
    label: String,
    value: String,
    members: Vec<Member>,
    disabled: bool,
    on_change: EventHandler<Option<i64>>,
) -> Element {
    let options = std::iter::once((String::new(), "Unassigned".to_owned()))
        .chain(
            members
                .into_iter()
                .map(|member| (member.id.to_string(), member.name)),
        )
        .collect();
    rsx! {
        TodoSelectField { id, label, value, options, disabled,
            on_change: move |value: String| on_change.call(value.parse().ok()),
        }
    }
}

#[component]
fn TodoTextField(icon: &'static str, children: Element) -> Element {
    rsx! {
        span { class: "todo-field todo-text-field",
            span { class: "todo-field-icon", crate::Icon { kind: icon } }
            {children}
        }
    }
}

#[component]
fn TodoSelectField(
    id: String,
    label: String,
    value: String,
    options: Vec<(String, String)>,
    disabled: bool,
    on_change: EventHandler<String>,
) -> Element {
    let mut active = use_signal(|| None::<usize>);
    let mut trigger = use_signal(|| None::<std::rc::Rc<MountedData>>);
    let selected = options
        .iter()
        .position(|option| option.0 == value)
        .unwrap_or(0);
    let selected_label = &options[selected].1;
    let menu = format!("{id}-options");
    let active_option = active().map(|index| format!("{menu}-{index}"));
    let expanded = active_option.is_some();
    let choose = Callback::new(move |next: String| {
        active.set(None);
        if next != value {
            on_change.call(next);
        }
    });
    let scroll_menu = menu.clone();
    use_effect(move || {
        if let Some(index) = active() {
            let _ = document::eval(&format!(
                r#"
                const menu = document.getElementById('{scroll_menu}');
                const panel = menu?.closest('.task-groups, .add-task-panel');
                if (panel) menu.style.setProperty('--todo-menu-room', `${{Math.max(40, panel.clientHeight - 16)}}px`);
                menu?.scrollIntoView({{block:'nearest',inline:'nearest'}});
                menu?.children[{index}]?.scrollIntoView({{block:'nearest',inline:'nearest'}});
                "#
            ));
        }
    });
    let keyboard_options = options.clone();
    rsx! {
        div { class: "todo-select-field",
            button { id: "{id}", class: "todo-field todo-select-trigger", r#type: "button",
                role: "combobox", aria_label: "{label}", aria_expanded: "{expanded}", aria_controls: "{menu}",
                aria_activedescendant: active_option, disabled,
                onmounted: move |event: MountedEvent| trigger.set(Some(event.data())),
                onblur: move |_| active.set(None),
                onclick: move |_| {
                    if active().is_some() {
                        active.set(None);
                    } else {
                        active.set(Some(selected));
                        if let Some(element) = trigger() {
                            spawn(async move { let _ = element.set_focus(true).await; });
                        }
                    }
                },
                onkeydown: move |event| {
                    if event.is_composing() { return; }
                    let current = active();
                    let last = keyboard_options.len() - 1;
                    let next = match event.key() {
                        Key::ArrowDown => current.map(|index| (index + 1).min(last)).unwrap_or(selected),
                        Key::ArrowUp => current.map(|index| index.saturating_sub(1)).unwrap_or(selected),
                        Key::Home => 0,
                        Key::End => last,
                        Key::Escape if current.is_some() => {
                            event.prevent_default();
                            event.stop_propagation();
                            active.set(None);
                            return;
                        }
                        Key::Tab => {
                            active.set(None);
                            return;
                        }
                        key if current.is_some() && (key == Key::Enter || key == Key::Character(" ".into())) => {
                            event.prevent_default();
                            if let Some(index) = current {
                                choose.call(keyboard_options[index].0.clone());
                            }
                            return;
                        }
                        _ => return,
                    };
                    event.prevent_default();
                    active.set(Some(next));
                },
                span { class: "todo-select-label", "{selected_label}" }
                span { class: "todo-field-icon", crate::Icon { kind: "chevron-down" } }
            }
            ul { id: "{menu}", class: "todo-select-options", role: "listbox", aria_label: "{label}", hidden: !expanded,
                onmousedown: move |event| event.prevent_default(),
                for (index, (option_value, option_label)) in options.into_iter().enumerate() {
                    li { key: "{option_value}", id: "{menu}-{index}", role: "option",
                        aria_selected: (index == selected).to_string(),
                        class: if active() == Some(index) { "is-active" } else { "" },
                        onclick: move |_| choose.call(option_value.clone()), "{option_label}"
                    }
                }
            }
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
        section { class: "add-task-panel surface-panel", aria_labelledby: "add-task-heading", tabindex: "0",
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
                TodoTextField { icon: "tasks",
                    input { id: "new-task-title", name: "title", required: true, value: "{title}", disabled: pending(),
                        oninput: move |event| title.set(event.value()) }
                }
                label { r#for: "new-task-description", "Description (optional)" }
                TodoTextField { icon: "description",
                    textarea { id: "new-task-description", value: "{description}", disabled: pending(), oninput: move |event| description.set(event.value()) }
                }
                label { r#for: "new-task-date", "Due date" }
                span { class: "todo-field",
                    input { id: "new-task-date", name: "due", r#type: "date", required: true, min: "0001-01-01", max: "9999-12-31", value: "{due}", disabled: pending(), oninput: move |event| due.set(event.value()) }
                }
                label { r#for: "new-task-repeat", "Repeat" }
                TodoSelectField { id: "new-task-repeat", label: "Repeat", value: repeat().value(), disabled: pending(),
                    options: [("once", "Once"), ("daily", "Daily"), ("weekly", "Weekly"), ("monthly", "Monthly")]
                        .map(|(value, label)| (value.to_owned(), label.to_owned())).to_vec(),
                    on_change: move |value: String| repeat.set(match value.as_str() {
                        "daily" => Repeat::Daily,
                        "weekly" => Repeat::Weekly,
                        "monthly" => Repeat::Monthly,
                        _ => Repeat::Once,
                    }),
                }
                label { r#for: "new-task-assignee", "Assigned to" }
                MemberSelect { id: "new-task-assignee", label: "Assigned to", value: selected_assignee, members,
                    disabled: pending(), on_change: move |value| assignee.set(value) }
                if !error().is_empty() { p { class: "form-error", role: "alert", "{error}" } }
                button { class: "primary-button", r#type: "submit", disabled: pending(), "Add task" }
                p { class: "form-feedback", role: "status", "{feedback}" }
            }
        }
    }
}
