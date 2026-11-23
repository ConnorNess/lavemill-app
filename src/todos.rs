use super::{CardHeading, Route, StatusMark};
use dioxus::prelude::*;

// A fixed day keeps the mock data identical during server rendering and hydration.
const TODAY: &str = "2026-11-23";
const MEMBERS: [&str; 4] = ["Connor", "Jade", "Ainsley", "Andrew"];

#[derive(Clone, PartialEq)]
struct Task {
    id: u64,
    title: String,
    due: String,
    assignee: String,
    completed: bool,
}

pub(super) struct TaskStore {
    tasks: Vec<Task>,
    next_id: u64,
}

impl TaskStore {
    pub(super) fn sample() -> Self {
        let tasks = [
            ("Muck out", "2026-11-22", "Connor", false),
            ("Take the bins out", TODAY, "Jade", false),
            ("Mop kitchen", TODAY, "Ainsley", false),
            ("Feed the chickens", TODAY, "Andrew", true),
            ("Clean the bathroom", "2026-11-24", "Andrew", false),
            ("Hoover upstairs", "2026-11-25", "", false),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (title, due, assignee, completed))| Task {
            id: index as u64,
            title: title.into(),
            due: due.into(),
            assignee: assignee.into(),
            completed,
        })
        .collect();
        Self { tasks, next_id: 6 }
    }

    fn add(&mut self, title: &str, due: &str, assignee: &str) -> Result<(), &'static str> {
        if title.trim().is_empty() {
            return Err("Enter a task title.");
        }
        if !valid_date(due) {
            return Err("Choose a valid due date.");
        }
        if !assignee.is_empty() && !MEMBERS.contains(&assignee) {
            return Err("Choose a household member or leave the task unassigned.");
        }
        self.tasks.push(Task {
            id: self.next_id,
            title: title.trim().into(),
            due: due.into(),
            assignee: assignee.into(),
            completed: false,
        });
        self.next_id += 1;
        Ok(())
    }

    fn set_completed(&mut self, id: u64, completed: bool) {
        if let Some(task) = self.tasks.iter_mut().find(|task| task.id == id) {
            task.completed = completed;
        }
    }

    fn assign(&mut self, id: u64, assignee: String) {
        if assignee.is_empty() || MEMBERS.contains(&assignee.as_str()) {
            if let Some(task) = self
                .tasks
                .iter_mut()
                .find(|task| task.id == id && !task.completed)
            {
                task.assignee = assignee;
            }
        }
    }
}

fn valid_date(date: &str) -> bool {
    let bytes = date.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    if !bytes
        .iter()
        .enumerate()
        .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
    {
        return false;
    }
    let year = date[..4].parse::<u32>().unwrap_or(0);
    let month = date[5..7].parse::<u32>().unwrap_or(0);
    let day = date[8..].parse::<u32>().unwrap_or(0);
    let days = match month {
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => 0,
    };
    year > 0 && day > 0 && day <= days
}

#[derive(Clone, Copy, PartialEq)]
enum Group {
    Overdue,
    Today,
    Upcoming,
    Completed,
}

impl Group {
    fn title(self) -> &'static str {
        match self {
            Self::Overdue => "Overdue",
            Self::Today => "Today",
            Self::Upcoming => "Upcoming",
            Self::Completed => "Completed",
        }
    }

    fn contains(self, task: &Task) -> bool {
        match self {
            Self::Completed => task.completed,
            Self::Overdue => !task.completed && task.due.as_str() < TODAY,
            Self::Today => !task.completed && task.due == TODAY,
            Self::Upcoming => !task.completed && task.due.as_str() > TODAY,
        }
    }
}

#[component]
pub(super) fn TasksCard() -> Element {
    let store = use_context::<Signal<TaskStore>>();
    let mut tasks: Vec<Task> = store
        .read()
        .tasks
        .iter()
        .filter(|task| task.due.as_str() <= TODAY)
        .cloned()
        .collect();
    tasks.sort_by(|a, b| {
        a.completed
            .cmp(&b.completed)
            .then(a.due.cmp(&b.due))
            .then(a.id.cmp(&b.id))
    });
    let open = tasks.iter().filter(|task| !task.completed).count();
    rsx! {
        section { class: "dashboard-card", aria_labelledby: "tasks-title",
            CardHeading { icon: "tasks", title: "Today’s To-dos", id: "tasks-title", count: format!("{open} open") }
            div { class: "card-body",
                ul { class: "item-list task-list",
                    for task in tasks.iter().take(4) {
                        li { key: "{task.id}", class: if task.completed { "list-row is-done" } else { "list-row" },
                            StatusMark { done: task.completed }
                            div { class: "row-copy",
                                span { class: "row-title", "{task.title}" }
                                span { class: if !task.completed && task.due.as_str() < TODAY { "row-note overdue" } else { "row-note" },
                                    if task.completed { "Completed" }
                                    else if task.due.as_str() < TODAY { "Overdue · {task.due}" }
                                    else { "Today" }
                                }
                            }
                            span { class: "assignee", if task.assignee.is_empty() { "Unassigned" } else { "{task.assignee}" } }
                        }
                    }
                }
            }
            Link { class: "card-page-link", to: Route::Todos {}, "View all to-dos" }
        }
    }
}

#[component]
pub(super) fn Todos() -> Element {
    rsx! {
        document::Title { "Lavemill · To-dos" }
        main { id: "main", class: "dashboard todos-page", tabindex: "-1",
            div { class: "todos-heading",
                h1 { "To-dos" }
                p { class: "mock-note", "Mock data · Today: 23 November 2026 · Resets on refresh" }
            }
            div { class: "todos-layout",
                div { class: "task-groups",
                    for group in [Group::Overdue, Group::Today, Group::Upcoming, Group::Completed] {
                        TaskGroup { key: "{group.title()}", group }
                    }
                }
                AddTaskForm {}
            }
        }
    }
}

#[component]
fn TaskGroup(group: Group) -> Element {
    let store = use_context::<Signal<TaskStore>>();
    let mut tasks: Vec<Task> = store
        .read()
        .tasks
        .iter()
        .filter(|task| group.contains(task))
        .cloned()
        .collect();
    tasks.sort_by(|a, b| a.due.cmp(&b.due).then(a.id.cmp(&b.id)));
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
                    for task in tasks { EditableTaskRow { key: "{task.id}", task } }
                }
            }
        }
    }
}

#[component]
fn EditableTaskRow(task: Task) -> Element {
    let mut store = use_context::<Signal<TaskStore>>();
    let id = task.id;
    rsx! {
        li { class: if task.completed { "editable-task is-done" } else { "editable-task" },
            label { class: "task-check-target",
                input { r#type: "checkbox", checked: task.completed,
                    aria_label: if task.completed { format!("Reopen {}", task.title) } else { format!("Complete {}", task.title) },
                    onchange: move |event| store.write().set_completed(id, event.checked())
                }
            }
            div { class: "row-copy",
                span { class: "row-title", "{task.title}" }
                time { class: "row-note", datetime: "{task.due}", "Due {task.due}" }
            }
            if task.completed {
                span { class: "completed-assignee", if task.assignee.is_empty() { "Unassigned" } else { "{task.assignee}" } }
                button { class: "undo-button", r#type: "button", aria_label: "Undo completion of {task.title}",
                    onclick: move |_| store.write().set_completed(id, false), "Undo" }
            } else {
                label { class: "task-assignee-field",
                    span { class: "sr-only", "Assign {task.title} to" }
                    select { value: "{task.assignee}", onchange: move |event| store.write().assign(id, event.value()),
                        option { value: "", "Unassigned" }
                        for member in MEMBERS { option { value: member, "{member}" } }
                    }
                }
            }
        }
    }
}

#[component]
fn AddTaskForm() -> Element {
    let mut store = use_context::<Signal<TaskStore>>();
    let mut title = use_signal(String::new);
    let mut due = use_signal(|| TODAY.to_string());
    let mut assignee = use_signal(String::new);
    let mut error = use_signal(String::new);
    let mut feedback = use_signal(String::new);
    rsx! {
        section { class: "add-task-panel", aria_labelledby: "add-task-heading",
            h2 { id: "add-task-heading", "Add a task" }
            form { onsubmit: move |_| {
                let result = store.write().add(&title(), &due(), &assignee());
                match result {
                    Ok(()) => {
                        feedback.set(format!("Added: {}", title().trim()));
                        error.set(String::new());
                        title.set(String::new());
                    }
                    Err(message) => { error.set(message.into()); feedback.set(String::new()); }
                }
            },
                label { r#for: "new-task-title", "Task" }
                input { id: "new-task-title", name: "title", required: true, value: "{title}",
                    oninput: move |event| { title.set(event.value()); feedback.set(String::new()); error.set(String::new()); }
                }
                label { r#for: "new-task-date", "Due date" }
                input { id: "new-task-date", name: "due", r#type: "date", required: true, min: "0001-01-01", max: "9999-12-31", value: "{due}", oninput: move |event| due.set(event.value()) }
                label { r#for: "new-task-assignee", "Assigned to" }
                select { id: "new-task-assignee", name: "assignee", value: "{assignee}", onchange: move |event| assignee.set(event.value()),
                    option { value: "", "Unassigned" }
                    for member in MEMBERS { option { value: member, "{member}" } }
                }
                if !error().is_empty() { p { class: "form-error", role: "alert", "{error}" } }
                button { class: "primary-button", r#type: "submit", "Add task" }
                p { class: "form-feedback", role: "status", "{feedback}" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_moves_between_groups_without_losing_date_or_assignment() {
        let mut store = TaskStore::sample();
        store.add("  Mop hallway  ", "2026-11-22", "Jade").unwrap();
        let id = store.tasks.last().unwrap().id;
        store.assign(id, "Ainsley".into());
        assert!(Group::Overdue.contains(store.tasks.last().unwrap()));
        store.set_completed(id, true);
        assert!(Group::Completed.contains(store.tasks.last().unwrap()));
        assert!(!Group::Overdue.contains(store.tasks.last().unwrap()));
        store.set_completed(id, false);
        let task = store.tasks.last().unwrap();
        assert!(Group::Overdue.contains(task));
        assert_eq!(
            (&*task.title, &*task.due, &*task.assignee),
            ("Mop hallway", "2026-11-22", "Ainsley")
        );
        store.add("Tomorrow", "2026-11-24", "").unwrap();
        assert!(Group::Upcoming.contains(store.tasks.last().unwrap()));
    }

    #[test]
    fn invalid_input_does_not_create_tasks() {
        let mut store = TaskStore::sample();
        let count = store.tasks.len();
        assert!(store.add("  ", TODAY, "Jade").is_err());
        assert!(store.add("Task", "2026-02-29", "Jade").is_err());
        assert!(store.add("Task", "2026-11-31", "Jade").is_err());
        assert!(store.add("Task", TODAY, "Unknown").is_err());
        assert_eq!(store.tasks.len(), count);
        assert!(valid_date("2028-02-29"));
        assert!(!valid_date("2026-1-01"));
    }
}
