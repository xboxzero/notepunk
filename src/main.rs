use leptos::ev;
use leptos::mount::mount_to_body;
use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use web_sys::{window, HtmlAnchorElement};

const STORAGE_NOTES: &str = "notepunk:notes";
const STORAGE_CURRENT: &str = "notepunk:current";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct Note {
    id: String,
    title: String,
    body: String,
    updated_at: f64,
}

impl Note {
    fn fresh() -> Self {
        let now = js_sys::Date::now();
        let rand = (js_sys::Math::random() * 1e9) as u32;
        Self {
            id: format!("{}-{}", now as u64, rand),
            title: String::new(),
            body: String::new(),
            updated_at: now,
        }
    }

    fn display_title(&self) -> String {
        if self.title.trim().is_empty() {
            "untitled".to_string()
        } else {
            self.title.clone()
        }
    }
}

fn local_storage() -> Option<web_sys::Storage> {
    window()?.local_storage().ok().flatten()
}

fn load_notes() -> Vec<Note> {
    local_storage()
        .and_then(|s| s.get_item(STORAGE_NOTES).ok().flatten())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn persist_notes(notes: &[Note]) {
    if let (Ok(json), Some(s)) = (serde_json::to_string(notes), local_storage()) {
        let _ = s.set_item(STORAGE_NOTES, &json);
    }
}

fn load_current() -> Option<String> {
    local_storage().and_then(|s| s.get_item(STORAGE_CURRENT).ok().flatten())
}

fn persist_current(id: &Option<String>) {
    if let Some(s) = local_storage() {
        match id {
            Some(v) => {
                let _ = s.set_item(STORAGE_CURRENT, v);
            }
            None => {
                let _ = s.remove_item(STORAGE_CURRENT);
            }
        }
    }
}

fn download_markdown(note: &Note) {
    let title = note.display_title();
    let safe: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let filename = format!("{}.md", safe);
    let content = if note.title.trim().is_empty() {
        note.body.clone()
    } else {
        format!("# {}\n\n{}", note.title, note.body)
    };
    let array = js_sys::Array::new();
    array.push(&wasm_bindgen::JsValue::from_str(&content));
    let blob = match web_sys::Blob::new_with_str_sequence(&array) {
        Ok(b) => b,
        Err(_) => return,
    };
    let url = match web_sys::Url::create_object_url_with_blob(&blob) {
        Ok(u) => u,
        Err(_) => return,
    };
    if let Some(doc) = window().and_then(|w| w.document()) {
        if let Ok(el) = doc.create_element("a") {
            if let Ok(a) = el.dyn_into::<HtmlAnchorElement>() {
                a.set_href(&url);
                a.set_download(&filename);
                a.click();
            }
        }
    }
    let _ = web_sys::Url::revoke_object_url(&url);
}

fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(App);
}

#[component]
fn App() -> impl IntoView {
    let initial_notes = load_notes();
    let initial_id =
        load_current().or_else(|| initial_notes.first().map(|n| n.id.clone()));

    let (notes, set_notes) = signal(initial_notes);
    let (current_id, set_current_id) = signal(initial_id);

    Effect::new(move |_| {
        let n = notes.get();
        persist_notes(&n);
    });

    Effect::new(move |_| {
        let id = current_id.get();
        persist_current(&id);
    });

    let current_note = move || {
        let id = current_id.get()?;
        notes.get().into_iter().find(|n| n.id == id)
    };

    let new_note = move |_| {
        let n = Note::fresh();
        let id = n.id.clone();
        set_notes.update(|v| v.insert(0, n));
        set_current_id.set(Some(id));
    };

    let delete_current = move |_| {
        let Some(id) = current_id.get() else { return };
        set_notes.update(|v| v.retain(|n| n.id != id));
        let next = notes.get_untracked().first().map(|n| n.id.clone());
        set_current_id.set(next);
    };

    let export_current = move |_| {
        if let Some(n) = current_note() {
            download_markdown(&n);
        }
    };

    let update_title = move |ev: ev::Event| {
        let value = event_target_value(&ev);
        let Some(id) = current_id.get_untracked() else { return };
        set_notes.update(|v| {
            if let Some(n) = v.iter_mut().find(|n| n.id == id) {
                n.title = value;
                n.updated_at = js_sys::Date::now();
            }
        });
    };

    let update_body = move |ev: ev::Event| {
        let value = event_target_value(&ev);
        let Some(id) = current_id.get_untracked() else { return };
        set_notes.update(|v| {
            if let Some(n) = v.iter_mut().find(|n| n.id == id) {
                n.body = value;
                n.updated_at = js_sys::Date::now();
            }
        });
    };

    view! {
        <main class="page">
            <header class="masthead">
                <h1 class="title">"NOTEPUNK"</h1>
                <p class="tagline">"// capture · remix · remember //"</p>
            </header>
            <div class="layout">
                <aside class="sidebar">
                    <button class="btn-new" on:click=new_note>"+ new note"</button>
                    <ul class="note-list">
                        <For
                            each=move || notes.get()
                            key=|n| n.id.clone()
                            children=move |n: Note| {
                                let id = n.id.clone();
                                let id_for_active = id.clone();
                                let title = n.display_title();
                                let preview: String =
                                    n.body.chars().take(60).collect();
                                let is_active = move || {
                                    current_id.get() == Some(id_for_active.clone())
                                };
                                view! {
                                    <li
                                        class:active=is_active
                                        on:click=move |_| {
                                            set_current_id.set(Some(id.clone()))
                                        }
                                    >
                                        <div class="note-title">{title}</div>
                                        <div class="note-preview">{preview}</div>
                                    </li>
                                }
                            }
                        />
                    </ul>
                </aside>
                <section class="editor">
                    {move || match current_note() {
                        Some(n) => view! {
                            <div class="editor-inner">
                                <input
                                    class="title-input"
                                    type="text"
                                    placeholder="title"
                                    prop:value=n.title.clone()
                                    on:input=update_title
                                />
                                <textarea
                                    class="body-input"
                                    placeholder="// start writing //"
                                    prop:value=n.body.clone()
                                    on:input=update_body
                                ></textarea>
                                <div class="toolbar">
                                    <button on:click=export_current>"export .md"</button>
                                    <button class="danger" on:click=delete_current>
                                        "delete"
                                    </button>
                                </div>
                            </div>
                        }
                        .into_any(),
                        None => view! {
                            <div class="empty-state">
                                <p>"no note open."</p>
                                <p class="dim">"hit + new note to start."</p>
                            </div>
                        }
                        .into_any(),
                    }}
                </section>
            </div>
        </main>
    }
}
