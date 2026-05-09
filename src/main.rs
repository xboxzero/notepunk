use leptos::ev;
use leptos::mount::mount_to_body;
use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
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

#[derive(Clone, Debug, PartialEq)]
struct ImageHit {
    title: String,
    thumb_url: String,
    full_url: String,
}

async fn search_commons(query: &str) -> Result<Vec<ImageHit>, String> {
    let encoded = js_sys::encode_uri_component(query)
        .as_string()
        .ok_or_else(|| "encoding failed".to_string())?;
    let url = format!(
        "https://commons.wikimedia.org/w/api.php?action=query&format=json&origin=*\
         &generator=search&gsrsearch={}&gsrnamespace=6&gsrlimit=24\
         &prop=imageinfo&iiprop=url%7Csize%7Cmime&iiurlwidth=240",
        encoded
    );
    let resp = gloo_net::http::Request::get(&url)
        .send()
        .await
        .map_err(|e| format!("network error: {}", e))?;
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("bad json: {}", e))?;

    let pages = json
        .get("query")
        .and_then(|q| q.get("pages"))
        .and_then(|p| p.as_object());
    let Some(pages) = pages else { return Ok(Vec::new()) };

    let mut items = Vec::new();
    for (_, page) in pages {
        let title = page
            .get("title")
            .and_then(|t| t.as_str())
            .and_then(|s| s.strip_prefix("File:"))
            .unwrap_or("")
            .to_string();
        let info = page.get("imageinfo").and_then(|a| a.get(0));
        let Some(info) = info else { continue };
        let mime = info.get("mime").and_then(|m| m.as_str()).unwrap_or("");
        if !mime.starts_with("image/") || mime == "image/svg+xml" {
            continue;
        }
        let thumb_url = info
            .get("thumburl")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
        let full_url = info
            .get("url")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
        if thumb_url.is_empty() || full_url.is_empty() || title.is_empty() {
            continue;
        }
        items.push(ImageHit {
            title,
            thumb_url,
            full_url,
        });
    }

    Ok(items)
}

fn extract_wikilinks(body: &str) -> Vec<String> {
    let mut links = Vec::new();
    let bytes = body.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            let start = i + 2;
            let mut j = start;
            let mut closed = false;
            while j + 1 < bytes.len() {
                if bytes[j] == b']' && bytes[j + 1] == b']' {
                    if let Ok(s) = std::str::from_utf8(&bytes[start..j]) {
                        let trimmed = s.trim();
                        if !trimmed.is_empty() {
                            links.push(trimmed.to_string());
                        }
                    }
                    i = j + 2;
                    closed = true;
                    break;
                }
                j += 1;
            }
            if !closed {
                break;
            }
        } else {
            i += 1;
        }
    }
    links
}

#[derive(Serialize)]
struct GraphNode<'a> {
    id: &'a str,
    label: String,
}

#[derive(Serialize)]
struct GraphEdge {
    id: String,
    source: String,
    target: String,
}

fn build_graph_payload(notes: &[Note]) -> (String, String) {
    let title_to_id: HashMap<String, &str> = notes
        .iter()
        .map(|n| (n.display_title().to_lowercase(), n.id.as_str()))
        .collect();

    let nodes: Vec<GraphNode> = notes
        .iter()
        .map(|n| GraphNode {
            id: &n.id,
            label: n.display_title(),
        })
        .collect();

    let mut edges = Vec::new();
    let mut idx: u64 = 0;
    for n in notes {
        for link in extract_wikilinks(&n.body) {
            if let Some(target_id) = title_to_id.get(&link.to_lowercase()) {
                if *target_id != n.id.as_str() {
                    edges.push(GraphEdge {
                        id: format!("e{}", idx),
                        source: n.id.clone(),
                        target: target_id.to_string(),
                    });
                    idx += 1;
                }
            }
        }
    }

    (
        serde_json::to_string(&nodes).unwrap_or_else(|_| "[]".into()),
        serde_json::to_string(&edges).unwrap_or_else(|_| "[]".into()),
    )
}

#[wasm_bindgen(inline_js = r#"
export function renderGraph(containerId, nodesJson, edgesJson, onClick) {
    const container = document.getElementById(containerId);
    if (!container) return;
    container.innerHTML = '';
    if (!window.cytoscape) {
        container.textContent = '// cytoscape failed to load — check your connection //';
        container.style.color = '#b8442a';
        container.style.padding = '2rem';
        container.style.fontFamily = 'Courier Prime, monospace';
        return;
    }
    const nodes = JSON.parse(nodesJson).map(n => ({ data: { id: n.id, label: n.label } }));
    const edges = JSON.parse(edgesJson).map(e => ({ data: { id: e.id, source: e.source, target: e.target } }));
    const cy = window.cytoscape({
        container,
        elements: { nodes, edges },
        style: [
            { selector: 'node', style: {
                'background-color': '#b8442a',
                'border-color': '#1a1612',
                'border-width': 2,
                'label': 'data(label)',
                'color': '#1a1612',
                'font-family': 'Special Elite, Courier Prime, monospace',
                'font-size': 13,
                'text-margin-y': -10,
                'text-valign': 'top',
                'text-halign': 'center',
                'width': 30, 'height': 30
            }},
            { selector: 'node:selected', style: {
                'background-color': '#1a1612',
                'border-color': '#b8442a'
            }},
            { selector: 'edge', style: {
                'width': 1.5,
                'line-color': '#1a1612',
                'curve-style': 'bezier',
                'opacity': 0.55,
                'target-arrow-color': '#1a1612',
                'target-arrow-shape': 'triangle',
                'arrow-scale': 0.8
            }}
        ],
        layout: { name: 'cose', animate: false, padding: 30, idealEdgeLength: 110 },
        wheelSensitivity: 0.2
    });
    cy.on('tap', 'node', (evt) => onClick(evt.target.id()));
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = renderGraph)]
    fn render_graph(
        container_id: &str,
        nodes_json: &str,
        edges_json: &str,
        on_click: &js_sys::Function,
    );
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum View {
    Edit,
    Graph,
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
    let (view, set_view) = signal(View::Edit);

    Effect::new(move |_| {
        let n = notes.get();
        persist_notes(&n);
    });

    Effect::new(move |_| {
        let id = current_id.get();
        persist_current(&id);
    });

    let on_node_click: Rc<Closure<dyn Fn(String)>> =
        Rc::new(Closure::new(move |id: String| {
            set_current_id.set(Some(id));
            set_view.set(View::Edit);
        }));

    let click_for_effect = on_node_click.clone();
    Effect::new(move |_| {
        let v = view.get();
        let n = notes.get();
        if v != View::Graph {
            return;
        }
        let (nodes_json, edges_json) = build_graph_payload(&n);
        let func: &js_sys::Function =
            click_for_effect.as_ref().as_ref().unchecked_ref();
        render_graph("notepunk-graph", &nodes_json, &edges_json, func);
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
        set_view.set(View::Edit);
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

    let (img_query, set_img_query) = signal(String::new());
    let (img_results, set_img_results) = signal(Vec::<ImageHit>::new());
    let (img_loading, set_img_loading) = signal(false);
    let (img_err, set_img_err) = signal(Option::<String>::None);

    let trigger_image_search = move || {
        let q = img_query.get_untracked();
        if q.trim().is_empty() {
            return;
        }
        set_img_loading.set(true);
        set_img_err.set(None);
        set_img_results.set(Vec::new());
        wasm_bindgen_futures::spawn_local(async move {
            match search_commons(&q).await {
                Ok(items) => set_img_results.set(items),
                Err(e) => set_img_err.set(Some(e)),
            }
            set_img_loading.set(false);
        });
    };

    let insert_image = move |title: String, url: String| {
        let Some(id) = current_id.get_untracked() else { return };
        set_notes.update(|v| {
            if let Some(n) = v.iter_mut().find(|n| n.id == id) {
                let snippet = format!("\n![{}]({})\n", title, url);
                if !n.body.is_empty() && !n.body.ends_with('\n') {
                    n.body.push('\n');
                }
                n.body.push_str(&snippet);
                n.updated_at = js_sys::Date::now();
            }
        });
    };

    view! {
        <main class="page">
            <header class="masthead">
                <h1 class="title">"NOTEPUNK"</h1>
                <p class="tagline">"// capture · remix · remember //"</p>
                <nav class="tabs">
                    <button
                        class:active=move || view.get() == View::Edit
                        on:click=move |_| set_view.set(View::Edit)
                    >
                        "edit"
                    </button>
                    <button
                        class:active=move || view.get() == View::Graph
                        on:click=move |_| set_view.set(View::Graph)
                    >
                        "graph"
                    </button>
                </nav>
            </header>
            {move || match view.get() {
                View::Edit => view! {
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
                                            placeholder="// start writing // use [[note title]] to link //"
                                            prop:value=n.body.clone()
                                            on:input=update_body
                                        ></textarea>
                                        <div class="toolbar">
                                            <button on:click=export_current>"export .md"</button>
                                            <button class="danger" on:click=delete_current>
                                                "delete"
                                            </button>
                                        </div>
                                        <div class="image-search">
                                            <div class="image-search-bar">
                                                <input
                                                    class="image-search-input"
                                                    type="text"
                                                    placeholder="// search wikimedia commons for images //"
                                                    prop:value=move || img_query.get()
                                                    on:input=move |ev| set_img_query.set(event_target_value(&ev))
                                                    on:keydown=move |ev: ev::KeyboardEvent| {
                                                        if ev.key() == "Enter" {
                                                            ev.prevent_default();
                                                            trigger_image_search();
                                                        }
                                                    }
                                                />
                                                <button on:click=move |_| trigger_image_search()>
                                                    "search"
                                                </button>
                                            </div>
                                            {move || if img_loading.get() {
                                                view! { <p class="dim">"// searching commons //"</p> }.into_any()
                                            } else if let Some(e) = img_err.get() {
                                                view! { <p class="error-msg">{e}</p> }.into_any()
                                            } else {
                                                view! { <span></span> }.into_any()
                                            }}
                                            <div class="image-grid">
                                                <For
                                                    each=move || img_results.get()
                                                    key=|r| r.full_url.clone()
                                                    children=move |r: ImageHit| {
                                                        let title = r.title.clone();
                                                        let url = r.full_url.clone();
                                                        let alt = r.title.clone();
                                                        let tooltip = r.title.clone();
                                                        view! {
                                                            <img
                                                                src=r.thumb_url.clone()
                                                                alt=alt
                                                                title=tooltip
                                                                on:click=move |_| {
                                                                    insert_image(title.clone(), url.clone())
                                                                }
                                                            />
                                                        }
                                                    }
                                                />
                                            </div>
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
                }
                .into_any(),
                View::Graph => view! {
                    <div class="graph-frame">
                        <div id="notepunk-graph" class="graph-container"></div>
                        <p class="graph-hint dim">
                            "// click a node to open the note · [[wiki links]] in note bodies create connections //"
                        </p>
                    </div>
                }
                .into_any(),
            }}
        </main>
    }
}
