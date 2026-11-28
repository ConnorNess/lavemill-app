use dioxus::prelude::*;

#[component]
pub(crate) fn SelectField(
    id: String,
    label: String,
    value: String,
    options: Vec<(String, String)>,
    #[props(default)] placeholder: Option<&'static str>,
    disabled: bool,
    on_change: EventHandler<String>,
) -> Element {
    let mut active = use_signal(|| None::<usize>);
    let mut trigger = use_signal(|| None::<std::rc::Rc<MountedData>>);
    let disabled = disabled || options.is_empty();
    let selected = options.iter().position(|option| option.0 == value);
    let selected_label = selected
        .map(|index| options[index].1.as_str())
        .or(placeholder)
        .or_else(|| options.first().map(|option| option.1.as_str()))
        .unwrap_or_default();
    let initial = selected.unwrap_or(0);
    let menu = format!("{id}-options");
    let active_option = active()
        .filter(|&index| !disabled && index < options.len())
        .map(|index| format!("{menu}-{index}"));
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
                const panel = menu?.closest('.todos-layout .task-groups, .todos-layout .add-task-panel');
                if (panel) menu.style.setProperty('--select-menu-room', `${{Math.max(40, panel.clientHeight - 16)}}px`);
                menu?.scrollIntoView({{block:'nearest',inline:'nearest'}});
                menu?.children[{index}]?.scrollIntoView({{block:'nearest',inline:'nearest'}});
                "#
            ));
        }
    });
    let keyboard_options = options.clone();
    rsx! {
        div { class: "select-field",
            button { id: "{id}", class: "select-trigger", r#type: "button",
                role: "combobox", aria_label: "{label}", aria_expanded: "{expanded}", aria_controls: "{menu}",
                aria_activedescendant: active_option, disabled,
                onmounted: move |event: MountedEvent| trigger.set(Some(event.data())),
                onblur: move |_| active.set(None),
                onclick: move |_| {
                    if active().is_some() {
                        active.set(None);
                    } else {
                        active.set(Some(initial));
                        if let Some(element) = trigger() {
                            spawn(async move { let _ = element.set_focus(true).await; });
                        }
                    }
                },
                onkeydown: move |event| {
                    if disabled || event.is_composing() { return; }
                    let last = keyboard_options.len() - 1;
                    let current = active().filter(|&index| index <= last);
                    let next = match event.key() {
                        Key::ArrowDown => current.map(|index| (index + 1).min(last)).unwrap_or(initial),
                        Key::ArrowUp => current.map(|index| index.saturating_sub(1)).unwrap_or(initial),
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
                span { class: "select-label", "{selected_label}" }
                span { class: "select-chevron", crate::Icon { kind: "chevron-down" } }
            }
            ul { id: "{menu}", class: "select-options", role: "listbox", aria_label: "{label}", hidden: !expanded,
                onmousedown: move |event| event.prevent_default(),
                for (index, (option_value, option_label)) in options.into_iter().enumerate() {
                    li { key: "{option_value}", id: "{menu}-{index}", role: "option",
                        aria_selected: (Some(index) == selected).to_string(),
                        class: if active() == Some(index) { "is-active" } else { "" },
                        onclick: move |_| choose.call(option_value.clone()), "{option_label}"
                    }
                }
            }
        }
    }
}
