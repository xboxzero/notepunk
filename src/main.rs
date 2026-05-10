mod audio;
mod commons;
mod config;
mod graph;
mod js_shim;
mod model;
mod similarity;
mod supabase;

use audio::{load_tracks, persist_tracks, AudioTrack};
use commons::ImageHit;
use config::{load_handle, persist_handle, SupabaseConfig};
use graph::{build as build_graph, GraphStats};
use js_shim::{
    audio_delete, audio_play, audio_play_mix, audio_set_volume, audio_start_recording,
    audio_stop, audio_stop_all, audio_stop_recording, js_bool, js_f64, js_string, render_3d_graph,
};
use leptos::ev;
use leptos::mount::mount_to_body;
use leptos::prelude::*;
use model::{
    download_markdown, load_current, load_graph_config, load_manual_edges, load_notes,
    persist_current, persist_graph_config, persist_manual_edges, persist_notes, ManualEdge, Note,
};
use pulldown_cmark::{html as md_html, Options as MdOpts, Parser as MdParser};
use serde::Serialize;
use similarity::{body_matches_query, extract_tags};
use std::rc::Rc;
use supabase::{fetch_comments, fetch_posts, post_comment, publish_post, Comment, Post};
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
enum RecordState {
    Idle,
    Requesting,
    Recording,
    Saving,
    Error(String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum View {
    Notes,
    Graph,
    Board,
    Guide,
}

#[derive(Serialize)]
struct MixTrack<'a> {
    id: &'a str,
    looping: bool,
    volume: f64,
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
                <li>"loop over linear."</li>
                <li>"image and sound over description."</li>
            </ul>

            <h2>"// the four signals on the graph //"</h2>
            <p class="frag">"[[wikilink]] — when one note explicitly names another."</p>
            <p class="frag">"#tag — when several notes share a hashtag, they cluster."</p>
            <p class="frag">"text similarity — notes that drift toward the same vocabulary, even without naming each other."</p>
            <p class="frag">"manual — links you draw by hand on the graph: a constellation only you can see."</p>

            <h2>"// audio · loop · mix //"</h2>
            <p>"each note has its own multi-track recorder. hit ● record to capture a fragment — the clip becomes a track on the note."</p>
            <p>"each track has play, loop, volume, delete. hit '▶ mix' to play every track on the note simultaneously, like a tape loop pile-up."</p>
            <p class="frag">"loops are infinite. layer them. let your morning rant become a beat under your evening reading."</p>
            <p class="quiet">"audio lives in your browser's IndexedDB — private to this device. nothing uploads."</p>

            <h2>"// the board //"</h2>
            <p>"the board tab is the only public surface. nothing leaves your device unless you hit 'publish' on a note."</p>
            <p>"published posts are open: anyone with the link can read and comment. set your handle once; it's stored locally."</p>

            <h2>"// the beat in the name //"</h2>
            <p class="frag">"kerouac wrote SCROLLS — one continuous burst, no paragraph breaks."</p>
            <p class="frag">"ginsberg said FIRST THOUGHT BEST THOUGHT."</p>
            <p class="frag">"capture the metaphor before you can defend it."</p>

            <h2>"// philosophy //"</h2>
            <p class="quiet">"these notes don't have to make sense. they just have to be honest."</p>
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
    let initial_tracks = load_tracks();
    let initial_handle = load_handle();

    let (notes, set_notes) = signal(initial_notes);
    let (current_id, set_current_id) = signal(initial_id);
    let (view, set_view) = signal(View::Notes);
    let (manual_edges, set_manual_edges) = signal(initial_manual);
    let (graph_cfg, set_graph_cfg) = signal(initial_cfg);
    let (link_mode, set_link_mode) = signal(false);
    let (graph_options_open, set_graph_options_open) = signal(false);
    let (search_query, set_search_query) = signal(String::new());
    let (graph_stats, set_graph_stats) = signal(GraphStats {
        wikilinks: 0,
        tags: 0,
        similarity: 0,
        manual: 0,
    });
    let (tracks, set_tracks) = signal(initial_tracks);
    let (record_state, set_record_state) = signal(RecordState::Idle);
    let (handle, set_handle) = signal(initial_handle);
    let (posts, set_posts) = signal(Vec::<Post>::new());
    let (posts_loading, set_posts_loading) = signal(false);
    let (posts_err, set_posts_err) = signal(Option::<String>::None);
    let (selected_post, set_selected_post) = signal(Option::<Post>::None);
    let (post_comments, set_post_comments) = signal(Vec::<Comment>::new());
    let (comment_draft, set_comment_draft) = signal(String::new());
    let (publish_status, set_publish_status) = signal(Option::<String>::None);

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
    Effect::new(move |_| {
        persist_tracks(&tracks.get());
    });
    Effect::new(move |_| {
        let h = handle.get();
        persist_handle(&h);
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
                    set_view.set(View::Notes);
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
            v.retain(|e| !((e.source == a && e.target == b) || (e.source == b && e.target == a)));
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
        render_3d_graph(
            "notepunk-graph",
            &payload.nodes_json,
            &payload.edges_json,
            node_fn,
            edge_fn,
            lm,
        );
    });

    let load_board_posts = move || {
        let Some(cfg) = SupabaseConfig::from_window() else {
            set_posts_err.set(Some(
                "// supabase not configured — see SETUP.md //".to_string(),
            ));
            return;
        };
        set_posts_loading.set(true);
        set_posts_err.set(None);
        wasm_bindgen_futures::spawn_local(async move {
            match fetch_posts(&cfg).await {
                Ok(items) => set_posts.set(items),
                Err(e) => set_posts_err.set(Some(e)),
            }
            set_posts_loading.set(false);
        });
    };

    Effect::new(move |_| {
        if view.get() == View::Board && posts.get_untracked().is_empty() {
            load_board_posts();
        }
    });

    let select_post = move |post: Post| {
        set_selected_post.set(Some(post.clone()));
        set_post_comments.set(Vec::new());
        set_comment_draft.set(String::new());
        let Some(cfg) = SupabaseConfig::from_window() else { return };
        let post_id = post.id.clone();
        wasm_bindgen_futures::spawn_local(async move {
            if let Ok(items) = fetch_comments(&cfg, &post_id).await {
                set_post_comments.set(items);
            }
        });
    };

    let send_comment = move || {
        let Some(post) = selected_post.get_untracked() else { return };
        let body = comment_draft.get_untracked();
        if body.trim().is_empty() {
            return;
        }
        let Some(cfg) = SupabaseConfig::from_window() else { return };
        let author = handle.get_untracked();
        wasm_bindgen_futures::spawn_local(async move {
            if let Ok(c) = post_comment(&cfg, &post.id, &body, &author).await {
                set_post_comments.update(|v| v.push(c));
                set_comment_draft.set(String::new());
            }
        });
    };

    let current_note = move || {
        let id = current_id.get()?;
        notes.get().into_iter().find(|n| n.id == id)
    };

    let new_note = move |_| {
        let n = Note::fresh();
        let id = n.id.clone();
        set_notes.update(|v| v.insert(0, n));
        set_current_id.set(Some(id));
        set_view.set(View::Notes);
    };

    let delete_current = move |_| {
        let Some(id) = current_id.get() else { return };
        let id_for_tracks = id.clone();
        set_notes.update(|v| v.retain(|n| n.id != id));
        set_manual_edges.update(|v| v.retain(|e| e.source != id && e.target != id));
        let track_ids: Vec<String> = tracks
            .get_untracked()
            .iter()
            .filter(|t| t.note_id == id_for_tracks)
            .map(|t| t.id.clone())
            .collect();
        for tid in &track_ids {
            let id = tid.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let _ = audio_delete(&id).await;
            });
        }
        set_tracks.update(|v| v.retain(|t| t.note_id != id_for_tracks));
        let next = notes.get_untracked().first().map(|n| n.id.clone());
        set_current_id.set(next);
    };

    let export_current = move |_| {
        if let Some(n) = current_note() {
            download_markdown(&n);
        }
    };

    let publish_current = move |_| {
        let Some(n) = current_note() else { return };
        let Some(cfg) = SupabaseConfig::from_window() else {
            set_publish_status.set(Some(
                "// supabase not configured — see SETUP.md //".to_string(),
            ));
            return;
        };
        let title = n.title.clone();
        let body = n.body.clone();
        let tags = extract_tags(&body);
        let author = handle.get_untracked();
        set_publish_status.set(Some("// publishing... //".to_string()));
        wasm_bindgen_futures::spawn_local(async move {
            match publish_post(&cfg, &title, &body, &tags, &author).await {
                Ok(_) => {
                    set_publish_status.set(Some("// published //".to_string()));
                    set_posts.set(Vec::new());
                }
                Err(e) => set_publish_status.set(Some(format!("// publish failed: {} //", e))),
            }
        });
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

    let mic_click = move |_| match record_state.get_untracked() {
        RecordState::Idle | RecordState::Error(_) => {
            set_record_state.set(RecordState::Requesting);
            wasm_bindgen_futures::spawn_local(async move {
                let result = audio_start_recording().await;
                if js_bool(&result, "ok") {
                    set_record_state.set(RecordState::Recording);
                } else {
                    set_record_state.set(RecordState::Error(js_string(&result, "error")));
                }
            });
        }
        RecordState::Recording => {
            let Some(note_id) = current_id.get_untracked() else { return };
            set_record_state.set(RecordState::Saving);
            wasm_bindgen_futures::spawn_local(async move {
                let count = tracks
                    .get_untracked()
                    .iter()
                    .filter(|t| t.note_id == note_id)
                    .count();
                let track_name = format!("track {}", count + 1);
                let track = AudioTrack::fresh(note_id.clone(), track_name, 0.0);
                let track_id = track.id.clone();
                let result = audio_stop_recording(&track_id).await;
                if !js_bool(&result, "ok") {
                    set_record_state.set(RecordState::Error(js_string(&result, "error")));
                    return;
                }
                let mut t = track;
                t.duration_ms = js_f64(&result, "duration_ms");
                set_tracks.update(|v| v.push(t));
                set_record_state.set(RecordState::Idle);
            });
        }
        RecordState::Requesting | RecordState::Saving => {}
    };

    let toggle_track_loop = move |id: String| {
        set_tracks.update(|v| {
            if let Some(t) = v.iter_mut().find(|t| t.id == id) {
                t.looping = !t.looping;
            }
        });
    };

    let set_track_volume = move |id: String, vol: f64| {
        set_tracks.update(|v| {
            if let Some(t) = v.iter_mut().find(|t| t.id == id) {
                t.volume = vol;
            }
        });
        let id_for_js = id.clone();
        audio_set_volume(&id_for_js, vol);
    };

    let play_track_action = move |t: AudioTrack| {
        let id = t.id.clone();
        let looping = t.looping;
        let volume = t.volume;
        wasm_bindgen_futures::spawn_local(async move {
            let _ = audio_play(&id, looping, volume).await;
        });
    };

    let stop_track_action = move |id: String| {
        audio_stop(&id);
    };

    let delete_track = move |id: String| {
        let id_for_js = id.clone();
        set_tracks.update(|v| v.retain(|t| t.id != id));
        wasm_bindgen_futures::spawn_local(async move {
            let _ = audio_delete(&id_for_js).await;
        });
    };

    let mix_play_all = move || {
        let Some(note_id) = current_id.get_untracked() else { return };
        let mix: Vec<MixTrack> = tracks
            .get_untracked()
            .iter()
            .filter(|t| t.note_id == note_id)
            .map(|t| MixTrack {
                id: Box::leak(t.id.clone().into_boxed_str()),
                looping: t.looping,
                volume: t.volume,
            })
            .collect();
        let json = serde_json::to_string(&mix).unwrap_or_else(|_| "[]".into());
        wasm_bindgen_futures::spawn_local(async move {
            let _ = audio_play_mix(&json).await;
        });
    };

    let stop_all_audio = move |_| {
        audio_stop_all();
    };

    let filtered_notes = move || {
        let q = search_query.get();
        notes
            .get()
            .into_iter()
            .filter(|n| body_matches_query(&n.title, &n.body, &q))
            .collect::<Vec<_>>()
    };

    let current_tracks = move || {
        let Some(id) = current_id.get() else {
            return Vec::<AudioTrack>::new();
        };
        tracks
            .get()
            .into_iter()
            .filter(|t| t.note_id == id)
            .collect()
    };

    let supabase_configured = SupabaseConfig::from_window().is_some();

    view! {
        <main class="page">
            <header class="masthead">
                <h1 class="title">"NOTEPUNK"</h1>
                <p class="tagline">"// capture · loop · publish · connect //"</p>
                <nav class="tabs">
                    <button class:active=move || view.get() == View::Notes
                            on:click=move |_| set_view.set(View::Notes)>"notes"</button>
                    <button class:active=move || view.get() == View::Graph
                            on:click=move |_| set_view.set(View::Graph)>"graph"</button>
                    <button class:active=move || view.get() == View::Board
                            on:click=move |_| set_view.set(View::Board)>"board"</button>
                    <button class:active=move || view.get() == View::Guide
                            on:click=move |_| set_view.set(View::Guide)>"guide"</button>
                </nav>
            </header>
            {move || match view.get() {
                View::Notes => view! {
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
                                        let preview: String = n.body.chars().take(60).collect();
                                        let tags = extract_tags(&n.body);
                                        let is_active = move || current_id.get() == Some(id_for_active.clone());
                                        view! {
                                            <li class:active=is_active
                                                on:click=move |_| set_current_id.set(Some(id.clone()))>
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
                                                placeholder="// write // [[link]] // #tag //"
                                                prop:value=n.body.clone()
                                                on:input=update_body
                                            ></textarea>
                                            <div class="preview" inner_html=render_markdown(&n.body)></div>

                                            <div class="audio-panel">
                                                <div class="audio-bar">
                                                    <button
                                                        class:recording=move || matches!(record_state.get(), RecordState::Recording)
                                                        class:busy=move || matches!(record_state.get(), RecordState::Requesting | RecordState::Saving)
                                                        on:click=mic_click
                                                    >
                                                        {move || match record_state.get() {
                                                            RecordState::Idle => "● record".to_string(),
                                                            RecordState::Error(_) => "● record".to_string(),
                                                            RecordState::Requesting => "...".to_string(),
                                                            RecordState::Recording => "■ stop".to_string(),
                                                            RecordState::Saving => "saving".to_string(),
                                                        }}
                                                    </button>
                                                    <button on:click=move |_| mix_play_all()>"▶ mix all"</button>
                                                    <button on:click=stop_all_audio>"■ stop all"</button>
                                                    {move || match record_state.get() {
                                                        RecordState::Recording => view! {
                                                            <span class="voice-hint">"// recording //"</span>
                                                        }.into_any(),
                                                        RecordState::Error(e) => view! {
                                                            <span class="voice-hint error-msg">{e}</span>
                                                        }.into_any(),
                                                        _ => view! { <span></span> }.into_any(),
                                                    }}
                                                </div>
                                                {move || {
                                                    let cur = current_tracks();
                                                    if cur.is_empty() {
                                                        view! { <p class="dim audio-empty">"// no tracks yet — record one //"</p> }.into_any()
                                                    } else {
                                                        view! {
                                                            <ul class="track-list">
                                                                {cur.into_iter().map(|t| {
                                                                    let id_stop = t.id.clone();
                                                                    let id_loop = t.id.clone();
                                                                    let id_vol = t.id.clone();
                                                                    let id_del = t.id.clone();
                                                                    let t_play = t.clone();
                                                                    let dur = format!("{:.1}s", t.duration_ms / 1000.0);
                                                                    let looping = t.looping;
                                                                    let volume = t.volume;
                                                                    let play_action = play_track_action;
                                                                    let stop_action = stop_track_action;
                                                                    let loop_action = toggle_track_loop;
                                                                    let vol_action = set_track_volume;
                                                                    let del_action = delete_track;
                                                                    view! {
                                                                        <li class="track-row">
                                                                            <span class="track-name">{t.name.clone()}</span>
                                                                            <span class="track-dur">{dur}</span>
                                                                            <button on:click=move |_| play_action(t_play.clone())>"▶"</button>
                                                                            <button on:click=move |_| stop_action(id_stop.clone())>"■"</button>
                                                                            <label class="loop-toggle">
                                                                                <input type="checkbox"
                                                                                    prop:checked=looping
                                                                                    on:change=move |_| loop_action(id_loop.clone())
                                                                                />
                                                                                "loop"
                                                                            </label>
                                                                            <input type="range" min="0" max="1" step="0.01"
                                                                                prop:value=volume.to_string()
                                                                                on:input=move |ev| {
                                                                                    let v = event_target_value(&ev).parse::<f64>().unwrap_or(0.8);
                                                                                    vol_action(id_vol.clone(), v);
                                                                                }
                                                                            />
                                                                            <button class="track-del" on:click=move |_| del_action(id_del.clone())>"×"</button>
                                                                        </li>
                                                                    }
                                                                }).collect_view()}
                                                            </ul>
                                                        }.into_any()
                                                    }
                                                }}
                                            </div>

                                            <div class="toolbar">
                                                <button on:click=export_current>"export .md"</button>
                                                <button on:click=publish_current>"publish to board"</button>
                                                <button class="danger" on:click=delete_current>"delete"</button>
                                                {move || match publish_status.get() {
                                                    Some(s) => view! { <span class="voice-hint dim">{s}</span> }.into_any(),
                                                    None => view! { <span></span> }.into_any(),
                                                }}
                                            </div>

                                            <details class="extras">
                                                <summary>"// images from wikimedia commons //"</summary>
                                                <div class="image-search">
                                                    <div class="image-search-bar">
                                                        <input
                                                            class="image-search-input"
                                                            type="text"
                                                            placeholder="// search images //"
                                                            prop:value=move || img_query.get()
                                                            on:input=move |ev| set_img_query.set(event_target_value(&ev))
                                                            on:keydown=move |ev: ev::KeyboardEvent| {
                                                                if ev.key() == "Enter" {
                                                                    ev.prevent_default();
                                                                    trigger_image_search();
                                                                }
                                                            }
                                                        />
                                                        <button on:click=move |_| trigger_image_search()>"search"</button>
                                                    </div>
                                                    {move || if img_loading.get() {
                                                        view! { <p class="dim">"// searching //"</p> }.into_any()
                                                    } else if let Some(e) = img_err.get() {
                                                        view! { <p class="error-msg">{e}</p> }.into_any()
                                                    } else { view! { <span></span> }.into_any() }}
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
                                                                        on:click=move |_| insert_image(title.clone(), url.clone())
                                                                    />
                                                                }
                                                            }
                                                        />
                                                    </div>
                                                </div>
                                            </details>
                                        </div>
                                    }.into_any()
                                }
                                None => view! {
                                    <div class="empty-state">
                                        <p>"no note open."</p>
                                        <p class="dim">"hit + new note to start."</p>
                                    </div>
                                }.into_any(),
                            }}
                        </section>
                    </div>
                }.into_any(),

                View::Graph => view! {
                    <div class="graph-frame">
                        <div class="graph-bar">
                            <button class="link-btn"
                                    class:active=move || link_mode.get()
                                    on:click=move |_| set_link_mode.update(|v| *v = !*v)>
                                {move || if link_mode.get() { "■ exit link mode" } else { "+ link mode" }}
                            </button>
                            <button class="link-btn"
                                    class:active=move || graph_options_open.get()
                                    on:click=move |_| set_graph_options_open.update(|v| *v = !*v)>
                                "options"
                            </button>
                            <span class="graph-bar-stats dim">
                                {move || {
                                    let s = graph_stats.get();
                                    format!("{} wikilinks · {} tags · {} similar · {} manual",
                                        s.wikilinks, s.tags, s.similarity, s.manual)
                                }}
                            </span>
                        </div>
                        {move || graph_options_open.get().then(|| view! {
                            <div class="graph-controls">
                                <div class="ctrl-group">
                                    <label class="ctrl-toggle">
                                        <input type="checkbox"
                                            prop:checked=move || graph_cfg.get().include_wikilinks
                                            on:change=move |ev| {
                                                let v = event_target_checked(&ev);
                                                set_graph_cfg.update(|c| c.include_wikilinks = v);
                                            }
                                        />
                                        <span class="kind-swatch wikilink"></span>"wikilinks"
                                    </label>
                                    <label class="ctrl-toggle">
                                        <input type="checkbox"
                                            prop:checked=move || graph_cfg.get().include_tags
                                            on:change=move |ev| {
                                                let v = event_target_checked(&ev);
                                                set_graph_cfg.update(|c| c.include_tags = v);
                                            }
                                        />
                                        <span class="kind-swatch tag"></span>"tags"
                                    </label>
                                    <label class="ctrl-toggle">
                                        <input type="checkbox"
                                            prop:checked=move || graph_cfg.get().include_similarity
                                            on:change=move |ev| {
                                                let v = event_target_checked(&ev);
                                                set_graph_cfg.update(|c| c.include_similarity = v);
                                            }
                                        />
                                        <span class="kind-swatch similarity"></span>"similarity"
                                    </label>
                                    <label class="ctrl-toggle">
                                        <input type="checkbox"
                                            prop:checked=move || graph_cfg.get().include_manual
                                            on:change=move |ev| {
                                                let v = event_target_checked(&ev);
                                                set_graph_cfg.update(|c| c.include_manual = v);
                                            }
                                        />
                                        <span class="kind-swatch manual"></span>"manual"
                                    </label>
                                </div>
                                <div class="ctrl-group">
                                    <label class="ctrl-slider">
                                        "similarity ≥ "
                                        <span class="ctrl-value">
                                            {move || format!("{:.2}", graph_cfg.get().similarity_threshold)}
                                        </span>
                                        <input type="range" min="0.05" max="0.6" step="0.01"
                                            prop:value=move || graph_cfg.get().similarity_threshold.to_string()
                                            on:input=move |ev| {
                                                let v = event_target_value(&ev).parse::<f64>().unwrap_or(0.18);
                                                set_graph_cfg.update(|c| c.similarity_threshold = v);
                                            }
                                        />
                                    </label>
                                </div>
                            </div>
                        })}
                        <div id="notepunk-graph" class="graph-container"></div>
                        <p class="graph-hint dim">
                            {move || if link_mode.get() {
                                "// link mode: click two nodes to draw an edge //".to_string()
                            } else {
                                "// drag to rotate · scroll to zoom · click a node to open · click an edge (manual only) to delete //".to_string()
                            }}
                        </p>
                    </div>
                }.into_any(),

                View::Board => {
                    let load_again = load_board_posts.clone();
                    let select = select_post;
                    view! {
                        <div class="board-frame">
                            <div class="board-bar">
                                <label class="handle-row">
                                    "handle: "
                                    <input
                                        class="handle-input"
                                        type="text"
                                        placeholder="anon"
                                        prop:value=move || handle.get()
                                        on:input=move |ev| set_handle.set(event_target_value(&ev))
                                    />
                                </label>
                                <button on:click=move |_| load_again()>"refresh"</button>
                                {(!supabase_configured).then(|| view! {
                                    <span class="error-msg">"// supabase not configured — see SETUP.md //"</span>
                                })}
                            </div>
                            <div class="board-layout">
                                <div class="board-list">
                                    {move || if posts_loading.get() {
                                        view! { <p class="dim">"// loading //"</p> }.into_any()
                                    } else if let Some(e) = posts_err.get() {
                                        view! { <p class="error-msg">{e}</p> }.into_any()
                                    } else if posts.get().is_empty() {
                                        view! { <p class="dim">"// no posts yet — publish a note //"</p> }.into_any()
                                    } else {
                                        view! {
                                            <ul class="post-list">
                                                {posts.get().into_iter().map(|p| {
                                                    let p_for_click = p.clone();
                                                    let select = select.clone();
                                                    let title = if p.title.trim().is_empty() { "untitled".to_string() } else { p.title.clone() };
                                                    let author = p.author.clone();
                                                    let preview: String = p.body.chars().take(80).collect();
                                                    let id_for_active = p.id.clone();
                                                    let is_active = move || selected_post.get().map(|s| s.id) == Some(id_for_active.clone());
                                                    view! {
                                                        <li class="post-row" class:active=is_active
                                                            on:click=move |_| select(p_for_click.clone())>
                                                            <div class="post-title">{title}</div>
                                                            <div class="post-meta dim">{format!("@{}", author)}</div>
                                                            <div class="post-preview">{preview}</div>
                                                        </li>
                                                    }
                                                }).collect_view()}
                                            </ul>
                                        }.into_any()
                                    }}
                                </div>
                                <div class="post-detail">
                                    {move || match selected_post.get() {
                                        Some(p) => {
                                            let html = render_markdown(&p.body);
                                            let title = if p.title.trim().is_empty() { "untitled".to_string() } else { p.title.clone() };
                                            view! {
                                                <article>
                                                    <h2 class="post-h">{title}</h2>
                                                    <p class="post-meta dim">{format!("@{}", p.author)}</p>
                                                    <div class="preview" inner_html=html></div>
                                                    <h3 class="comments-h">"// comments //"</h3>
                                                    <ul class="comments">
                                                        {move || post_comments.get().into_iter().map(|c| view! {
                                                            <li class="comment">
                                                                <div class="comment-author dim">{format!("@{}", c.author)}</div>
                                                                <div class="comment-body">{c.body}</div>
                                                            </li>
                                                        }).collect_view()}
                                                    </ul>
                                                    <div class="comment-compose">
                                                        <textarea
                                                            placeholder="// add a comment //"
                                                            prop:value=move || comment_draft.get()
                                                            on:input=move |ev| set_comment_draft.set(event_target_value(&ev))
                                                        ></textarea>
                                                        <button on:click=move |_| send_comment()>"post comment"</button>
                                                    </div>
                                                </article>
                                            }.into_any()
                                        }
                                        None => view! {
                                            <p class="dim">"// click a post on the left to open //"</p>
                                        }.into_any(),
                                    }}
                                </div>
                            </div>
                        </div>
                    }.into_any()
                }

                View::Guide => view! { <GuideView /> }.into_any(),
            }}
        </main>
    }
}
