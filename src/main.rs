mod commons;
mod graph;
mod js_shim;
mod model;
mod similarity;

use commons::ImageHit;
use graph::{build as build_graph, GraphStats};
use js_shim::{js_bool, js_string, render_graph, start_recording, stop_and_transcribe};
use leptos::ev;
use leptos::mount::mount_to_body;
use leptos::prelude::*;
use model::{
    download_markdown, load_current, load_graph_config, load_manual_edges, load_notes,
    persist_current, persist_graph_config, persist_manual_edges, persist_notes, ManualEdge, Note,
};
use pulldown_cmark::{html as md_html, Options as MdOpts, Parser as MdParser};
use similarity::{body_matches_query, extract_tags};
use std::rc::Rc;
use wasm_bindgen::prelude::*;

fn render_markdown(body: &str) -> String {
    let mut opts = MdOpts::empty();
    opts.insert(MdOpts::ENABLE_STRIKETHROUGH);
    opts.insert(MdOpts::ENABLE_TABLES);
    opts.insert(MdOpts::ENABLE_TASKLISTS);
    let parser = MdParser::new_ext(body, opts);
    let mut out = String::new();
    md_html::push_html(&mut out, parser);
    out
}

#[derive(Clone, Debug, PartialEq)]
enum VoiceState {
    Idle,
    Requesting,
    Recording,
    Transcribing,
    Error(String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum View {
    Edit,
    Graph,
    Guide,
}

#[component]
fn GuideView() -> impl IntoView {
    view! {
        <article class="guide">
            <h2>"// the method //"</h2>
            <ul>
                <li>"capture before organize."</li>
                <li>"fragments over essays."</li>
                <li>"link over nest."</li>
                <li>"voice over silence."</li>
                <li>"image over description."</li>
            </ul>

            <h2>"// the four signals //"</h2>
            <p class="frag">"[[wikilink]] — when one note explicitly names another."</p>
            <p class="frag">"#tag — when several notes share a hashtag, they cluster."</p>
            <p class="frag">"text similarity — notes that drift toward the same vocabulary, even without naming each other."</p>
            <p class="frag">"manual — links you draw by hand on the graph: a constellation only you can see."</p>

            <h2>"// the beat in the name //"</h2>
            <p class="frag">"kerouac wrote SCROLLS — one continuous burst, no paragraph breaks."</p>
            <p class="frag">"ginsberg said FIRST THOUGHT BEST THOUGHT."</p>
            <p class="frag">"capture the metaphor before you can defend it."</p>
            <p class="frag">"capture the assumption before you can disown it."</p>
            <p class="frag">"capture the question before you answer it."</p>

            <h2>"// the tools //"</h2>
            <p>"[[wikilinks]] connect notes by title — type the title in double brackets and the graph view will draw a red line."</p>
            <p>"#tags written in your body cluster notes — shared tags pull notes together with blue dashes."</p>
            <p>"the graph view (top right tab) shows the whole constellation. tweak the sliders to filter what counts as a connection. dense clusters are your obsessions."</p>
            <p>"the image search pulls free-use images straight from wikimedia commons. images embedded in a note become the thumbnail on its node."</p>
            <p>"voice capture (whisper) transcribes spoken fragments into the body, in-browser, no upload."</p>
            <p>"link mode (graph view) lets you draw connections by hand — click two nodes and a black line appears. click any edge to delete it."</p>

            <h2>"// a daily loop //"</h2>
            <p class="frag">"morning · 3 fragments. anything. set the day."</p>
            <p class="frag">"during · when a metaphor lands, capture it. one line is enough. add a #tag if you can."</p>
            <p class="frag">"evening · a longer note that [[links back]]. let yesterday touch today."</p>
            <p class="frag">"weekly · open the graph. tune the similarity slider until the constellation surprises you."</p>

            <h2>"// ten prompts to write against //"</h2>
            <ul>
                <li>"what metaphor did i notice today?"</li>
                <li>"what assumption am i making?"</li>
                <li>"what question is hiding inside this question?"</li>
                <li>"what got under my skin?"</li>
                <li>"what would my younger self think of this?"</li>
                <li>"what would my older self think of this?"</li>
                <li>"what am i avoiding?"</li>
                <li>"what am i pretending to know?"</li>
                <li>"what's the simplest version of this?"</li>
                <li>"what would change if i was wrong?"</li>
            </ul>

            <h2>"// philosophy //"</h2>
            <p class="quiet">"these notes don't have to make sense. they just have to be honest."</p>
            <p class="quiet">"the goal isn't to remember everything — it's to surprise yourself later."</p>
            <p class="quiet">"a notebook is a conversation with the version of you that hasn't shown up yet."</p>
        </article>
    }
}

fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(App);
}

#[component]
fn App() -> impl IntoView {
    let initial_notes = load_notes();
    let initial_id = load_current().or_else(|| initial_notes.first().map(|n| n.id.clone()));
    let initial_manual = load_manual_edges();
    let initial_cfg = load_graph_config();

    let (notes, set_notes) = signal(initial_notes);
    let (current_id, set_current_id) = signal(initial_id);
    let (view, set_view) = signal(View::Edit);
    let (manual_edges, set_manual_edges) = signal(initial_manual);
    let (graph_cfg, set_graph_cfg) = signal(initial_cfg);
    let (link_mode, set_link_mode) = signal(false);
    let (search_query, set_search_query) = signal(String::new());
    let (graph_stats, set_graph_stats) = signal(GraphStats {
        wikilinks: 0,
        tags: 0,
        similarity: 0,
        manual: 0,
    });

    Effect::new(move |_| {
        persist_notes(&notes.get());
    });
    Effect::new(move |_| {
        persist_current(&current_id.get());
    });
    Effect::new(move |_| {
        persist_manual_edges(&manual_edges.get());
    });
    Effect::new(move |_| {
        persist_graph_config(&graph_cfg.get());
    });

    let on_node_tap: Rc<Closure<dyn Fn(String)>> = Rc::new(Closure::new(move |payload: String| {
        let v: serde_json::Value = match serde_json::from_str(&payload) {
            Ok(x) => x,
            Err(_) => return,
        };
        let action = v.get("action").and_then(|a| a.as_str()).unwrap_or("");
        match action {
            "open" => {
                if let Some(id) = v.get("id").and_then(|x| x.as_str()) {
                    set_current_id.set(Some(id.to_string()));
                    set_view.set(View::Edit);
                    set_link_mode.set(false);
                }
            }
            "link" => {
                let (Some(a), Some(b)) = (
                    v.get("source").and_then(|x| x.as_str()),
                    v.get("target").and_then(|x| x.as_str()),
                ) else {
                    return;
                };
                if a == b {
                    return;
                }
                let edge = ManualEdge {
                    source: a.to_string(),
                    target: b.to_string(),
                };
                set_manual_edges.update(|v| {
                    let exists = v.iter().any(|e| {
                        (e.source == edge.source && e.target == edge.target)
                            || (e.source == edge.target && e.target == edge.source)
                    });
                    if !exists {
                        v.push(edge);
                    }
                });
            }
            _ => {}
        }
    }));

    let on_edge_tap: Rc<Closure<dyn Fn(String)>> = Rc::new(Closure::new(move |payload: String| {
        let v: serde_json::Value = match serde_json::from_str(&payload) {
            Ok(x) => x,
            Err(_) => return,
        };
        let kind = v.get("kind").and_then(|a| a.as_str()).unwrap_or("");
        if kind != "manual" {
            return;
        }
        let (Some(a), Some(b)) = (
            v.get("source").and_then(|x| x.as_str()),
            v.get("target").and_then(|x| x.as_str()),
        ) else {
            return;
        };
        set_manual_edges.update(|v| {
            v.retain(|e| {
                !((e.source == a && e.target == b) || (e.source == b && e.target == a))
            });
        });
    }));

    let node_for_effect = on_node_tap.clone();
    let edge_for_effect = on_edge_tap.clone();
    Effect::new(move |_| {
        let v = view.get();
        let n = notes.get();
        let m = manual_edges.get();
        let cfg = graph_cfg.get();
        let lm = link_mode.get();
        if v != View::Graph {
            return;
        }
        let payload = build_graph(&n, &cfg, &m);
        set_graph_stats.set(payload.stats);
        let node_fn: &js_sys::Function = node_for_effect.as_ref().as_ref().unchecked_ref();
        let edge_fn: &js_sys::Function = edge_for_effect.as_ref().as_ref().unchecked_ref();
        render_graph(
            "notepunk-graph",
            &payload.nodes_json,
            &payload.edges_json,
            node_fn,
            edge_fn,
            lm,
        );
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
        set_manual_edges.update(|v| v.retain(|e| e.source != id && e.target != id));
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
            match commons::search(&q).await {
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

    let (voice_state, set_voice_state) = signal(VoiceState::Idle);

    let mic_click = move |_| match voice_state.get_untracked() {
        VoiceState::Idle | VoiceState::Error(_) => {
            set_voice_state.set(VoiceState::Requesting);
            wasm_bindgen_futures::spawn_local(async move {
                let result = start_recording().await;
                if js_bool(&result, "ok") {
                    set_voice_state.set(VoiceState::Recording);
                } else {
                    set_voice_state.set(VoiceState::Error(js_string(&result, "error")));
                }
            });
        }
        VoiceState::Recording => {
            set_voice_state.set(VoiceState::Transcribing);
            wasm_bindgen_futures::spawn_local(async move {
                let result = stop_and_transcribe().await;
                if !js_bool(&result, "ok") {
                    set_voice_state.set(VoiceState::Error(js_string(&result, "error")));
                    return;
                }
                let text = js_string(&result, "text");
                if !text.is_empty() {
                    if let Some(id) = current_id.get_untracked() {
                        set_notes.update(|v| {
                            if let Some(n) = v.iter_mut().find(|n| n.id == id) {
                                if !n.body.is_empty() && !n.body.ends_with('\n') {
                                    n.body.push('\n');
                                }
                                n.body.push_str(&text);
                                n.body.push('\n');
                                n.updated_at = js_sys::Date::now();
                            }
                        });
                    }
                }
                set_voice_state.set(VoiceState::Idle);
            });
        }
        VoiceState::Requesting | VoiceState::Transcribing => {}
    };

    let filtered_notes = move || {
        let q = search_query.get();
        notes
            .get()
            .into_iter()
            .filter(|n| body_matches_query(&n.title, &n.body, &q))
            .collect::<Vec<_>>()
    };

    view! {
        <main class="page">
            <header class="masthead">
                <h1 class="title">"NOTEPUNK"</h1>
                <p class="tagline">"// capture · remix · remember · connect //"</p>
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
                    <button
                        class:active=move || view.get() == View::Guide
                        on:click=move |_| set_view.set(View::Guide)
                    >
                        "guide"
                    </button>
                </nav>
            </header>
            {move || match view.get() {
                View::Edit => view! {
                    <div class="layout">
                        <aside class="sidebar">
                            <button class="btn-new" on:click=new_note>"+ new note"</button>
                            <input
                                class="search-input"
                                type="text"
                                placeholder="// search notes //"
                                prop:value=move || search_query.get()
                                on:input=move |ev| set_search_query.set(event_target_value(&ev))
                            />
                            <ul class="note-list">
                                <For
                                    each=filtered_notes
                                    key=|n| n.id.clone()
                                    children=move |n: Note| {
                                        let id = n.id.clone();
                                        let id_for_active = id.clone();
                                        let title = n.display_title();
                                        let preview: String =
                                            n.body.chars().take(60).collect();
                                        let tags = extract_tags(&n.body);
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
                                                {(!tags.is_empty()).then(|| view! {
                                                    <div class="note-tags">
                                                        {tags.into_iter().take(4).map(|t| view! {
                                                            <span class="tag-chip">{format!("#{}", t)}</span>
                                                        }).collect_view()}
                                                    </div>
                                                })}
                                            </li>
                                        }
                                    }
                                />
                            </ul>
                        </aside>
                        <section class="editor">
                            {move || match current_note() {
                                Some(n) => {
                                    let body_for_tags = n.body.clone();
                                    let tags = extract_tags(&body_for_tags);
                                    view! {
                                        <div class="editor-inner">
                                            <input
                                                class="title-input"
                                                type="text"
                                                placeholder="title"
                                                prop:value=n.title.clone()
                                                on:input=update_title
                                            />
                                            {(!tags.is_empty()).then(|| view! {
                                                <div class="tag-row">
                                                    {tags.into_iter().map(|t| view! {
                                                        <span class="tag-chip">{format!("#{}", t)}</span>
                                                    }).collect_view()}
                                                </div>
                                            })}
                                            <textarea
                                                class="body-input"
                                                placeholder="// start writing // [[link]] notes // #tag fragments //"
                                                prop:value=n.body.clone()
                                                on:input=update_body
                                            ></textarea>
                                            <div
                                                class="preview"
                                                inner_html=render_markdown(&n.body)
                                            ></div>
                                            <div class="toolbar">
                                                <button
                                                    class:recording=move || matches!(voice_state.get(), VoiceState::Recording)
                                                    class:busy=move || matches!(voice_state.get(), VoiceState::Requesting | VoiceState::Transcribing)
                                                    on:click=mic_click
                                                >
                                                    {move || match voice_state.get() {
                                                        VoiceState::Idle => "● record".to_string(),
                                                        VoiceState::Error(_) => "● record".to_string(),
                                                        VoiceState::Requesting => "...".to_string(),
                                                        VoiceState::Recording => "■ stop".to_string(),
                                                        VoiceState::Transcribing => "transcribing".to_string(),
                                                    }}
                                                </button>
                                                <button on:click=export_current>"export .md"</button>
                                                <button class="danger" on:click=delete_current>
                                                    "delete"
                                                </button>
                                                {move || match voice_state.get() {
                                                    VoiceState::Recording => view! {
                                                        <span class="voice-hint">"// recording — click stop when done //"</span>
                                                    }.into_any(),
                                                    VoiceState::Transcribing => view! {
                                                        <span class="voice-hint dim">"// first run downloads ~75MB whisper model //"</span>
                                                    }.into_any(),
                                                    VoiceState::Error(e) => view! {
                                                        <span class="voice-hint error-msg">{e}</span>
                                                    }.into_any(),
                                                    _ => view! { <span></span> }.into_any(),
                                                }}
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
                                    }.into_any()
                                }
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
                        <div class="graph-controls">
                            <div class="ctrl-group">
                                <label class="ctrl-toggle">
                                    <input
                                        type="checkbox"
                                        prop:checked=move || graph_cfg.get().include_wikilinks
                                        on:change=move |ev| {
                                            let v = event_target_checked(&ev);
                                            set_graph_cfg.update(|c| c.include_wikilinks = v);
                                        }
                                    />
                                    <span class="kind-swatch wikilink"></span>
                                    "wikilinks "
                                    <span class="ctrl-count">{move || graph_stats.get().wikilinks}</span>
                                </label>
                                <label class="ctrl-toggle">
                                    <input
                                        type="checkbox"
                                        prop:checked=move || graph_cfg.get().include_tags
                                        on:change=move |ev| {
                                            let v = event_target_checked(&ev);
                                            set_graph_cfg.update(|c| c.include_tags = v);
                                        }
                                    />
                                    <span class="kind-swatch tag"></span>
                                    "tags "
                                    <span class="ctrl-count">{move || graph_stats.get().tags}</span>
                                </label>
                                <label class="ctrl-toggle">
                                    <input
                                        type="checkbox"
                                        prop:checked=move || graph_cfg.get().include_similarity
                                        on:change=move |ev| {
                                            let v = event_target_checked(&ev);
                                            set_graph_cfg.update(|c| c.include_similarity = v);
                                        }
                                    />
                                    <span class="kind-swatch similarity"></span>
                                    "similarity "
                                    <span class="ctrl-count">{move || graph_stats.get().similarity}</span>
                                </label>
                                <label class="ctrl-toggle">
                                    <input
                                        type="checkbox"
                                        prop:checked=move || graph_cfg.get().include_manual
                                        on:change=move |ev| {
                                            let v = event_target_checked(&ev);
                                            set_graph_cfg.update(|c| c.include_manual = v);
                                        }
                                    />
                                    <span class="kind-swatch manual"></span>
                                    "manual "
                                    <span class="ctrl-count">{move || graph_stats.get().manual}</span>
                                </label>
                            </div>
                            <div class="ctrl-group">
                                <label class="ctrl-slider">
                                    "similarity ≥ "
                                    <span class="ctrl-value">
                                        {move || format!("{:.2}", graph_cfg.get().similarity_threshold)}
                                    </span>
                                    <input
                                        type="range"
                                        min="0.05"
                                        max="0.6"
                                        step="0.01"
                                        prop:value=move || graph_cfg.get().similarity_threshold.to_string()
                                        on:input=move |ev| {
                                            let v = event_target_value(&ev).parse::<f64>().unwrap_or(0.18);
                                            set_graph_cfg.update(|c| c.similarity_threshold = v);
                                        }
                                    />
                                </label>
                                <label class="ctrl-toggle">
                                    <input
                                        type="checkbox"
                                        prop:checked=move || graph_cfg.get().show_thumbnails
                                        on:change=move |ev| {
                                            let v = event_target_checked(&ev);
                                            set_graph_cfg.update(|c| c.show_thumbnails = v);
                                        }
                                    />
                                    "thumbnails"
                                </label>
                                <button
                                    class="link-btn"
                                    class:active=move || link_mode.get()
                                    on:click=move |_| set_link_mode.update(|v| *v = !*v)
                                >
                                    {move || if link_mode.get() { "■ exit link mode" } else { "+ link mode" }}
                                </button>
                            </div>
                        </div>
                        <div id="notepunk-graph" class="graph-container"></div>
                        <p class="graph-hint dim">
                            {move || if link_mode.get() {
                                "// link mode: click two nodes to draw an edge · click empty space to cancel //".to_string()
                            } else {
                                "// click a node to open · click an edge (manual) to delete · color = recency + degree //".to_string()
                            }}
                        </p>
                    </div>
                }
                .into_any(),
                View::Guide => view! { <GuideView /> }.into_any(),
            }}
        </main>
    }
}
