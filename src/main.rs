mod audio;
mod commons;
mod config;
mod graph;
mod js_shim;
mod model;
mod rpg;
mod similarity;
mod supabase;

use audio::{load_tracks, persist_tracks, AudioTrack};
use commons::ImageHit;
use config::{load_handle, persist_handle, SupabaseConfig};
use graph::{build as build_graph, GraphStats};
use js_shim::{
    audio_delete, audio_play, audio_play_mix, audio_set_volume, audio_start_recording,
    audio_stop, audio_stop_all, audio_stop_recording, js_bool, js_f64, js_string, render_3d_graph,
    sfx_mute, sfx_play,
};
use leptos::ev;
use leptos::mount::mount_to_body;
use leptos::prelude::*;
use model::{
    download_markdown, load_current, load_graph_config, load_manual_edges, load_notes,
    persist_current, persist_graph_config, persist_manual_edges, persist_notes, ManualEdge, Note,
};
use pulldown_cmark::{html as md_html, Options as MdOpts, Parser as MdParser};
use rpg::{
    ensure_today_quests, load_quests, load_rpg, load_sfx_muted, persist_quests, persist_rpg,
    persist_sfx_muted, progress_quest, skill_level, today, touch_day, QuestKind, QuestState,
    RpgState,
};
use serde::Serialize;
use similarity::{body_matches_query, count_images, extract_tags, extract_wikilinks};
use std::collections::HashMap;
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
    Quests,
    Board,
    Guide,
}

#[derive(Serialize)]
struct MixTrack<'a> {
    id: &'a str,
    looping: bool,
    volume: f64,
}

fn xp_progress_pct(state: &RpgState) -> f64 {
    let needed = state.xp_for_level();
    if needed == 0 {
        return 1.0;
    }
    (state.xp_into_level() as f64 / needed as f64).clamp(0.0, 1.0)
}

fn flash_message(set_msg: WriteSignal<Option<String>>, text: String) {
    set_msg.set(Some(text));
    let cb = Closure::once(move || set_msg.set(None));
    if let Some(w) = web_sys::window() {
        let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(
            cb.as_ref().unchecked_ref(),
            4200,
        );
    }
    cb.forget();
}

#[component]
fn Hud(
    rpg: ReadSignal<RpgState>,
    quests: ReadSignal<QuestState>,
    sfx_muted: ReadSignal<bool>,
    set_sfx_muted: WriteSignal<bool>,
    event_msg: ReadSignal<Option<String>>,
    set_view: WriteSignal<View>,
) -> impl IntoView {
    view! {
        <div class="hud">
            <button
                class="hud-level"
                on:click=move |_| set_view.set(View::Quests)
                title="open quest log"
            >
                {move || format!("LVL {}", rpg.get().level())}
            </button>
            <div class="hud-xp-wrap">
                <div class="hud-xp-bar">
                    <div class="hud-xp-fill"
                        style:width=move || format!("{:.1}%", xp_progress_pct(&rpg.get()) * 100.0)
                    ></div>
                </div>
                <div class="hud-xp-num">
                    {move || {
                        let s = rpg.get();
                        format!("{} / {} xp", s.xp_into_level(), s.xp_for_level())
                    }}
                </div>
            </div>
            <div class="hud-stat hud-gold" title="gold">
                <span class="hud-icon">"⌬"</span>
                <span class="hud-value">{move || rpg.get().gold}</span>
            </div>
            <div class="hud-stat hud-streak" title="streak">
                <span class="hud-icon">"※"</span>
                <span class="hud-value">{move || format!("{}d", rpg.get().streak)}</span>
            </div>
            <div class="hud-quest"
                 on:click=move |_| set_view.set(View::Quests)
                 title="open quest log">
                {move || {
                    let qs = quests.get();
                    let first = qs.quests.iter().find(|q| !q.claimed).cloned();
                    match first {
                        Some(q) => {
                            let pct = if q.target == 0 { 1.0 } else {
                                (q.progress as f64 / q.target as f64).clamp(0.0, 1.0)
                            };
                            view! {
                                <div class="hud-quest-line">
                                    <span class="hud-quest-title">{q.title()}</span>
                                    <span class="hud-quest-prog">
                                        {format!(" {}/{}", q.progress.min(q.target), q.target)}
                                    </span>
                                </div>
                                <div class="hud-quest-bar">
                                    <div class="hud-quest-fill"
                                         style:width=format!("{:.0}%", pct * 100.0)>
                                    </div>
                                </div>
                            }.into_any()
                        }
                        None => view! {
                            <div class="hud-quest-line">
                                <span class="hud-quest-title">"all quests cleared today ✶"</span>
                            </div>
                        }.into_any(),
                    }
                }}
            </div>
            <button
                class="hud-sfx"
                class:muted=move || sfx_muted.get()
                on:click=move |_| {
                    let new_val = !sfx_muted.get_untracked();
                    set_sfx_muted.set(new_val);
                    sfx_mute(new_val);
                    if !new_val { sfx_play("tap"); }
                }
                title="toggle sfx"
            >
                {move || if sfx_muted.get() { "♪ off" } else { "♪ on" }}
            </button>
            {move || event_msg.get().map(|m| view! {
                <div class="hud-flash">{m}</div>
            })}
        </div>
    }
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
        auto: 0,
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

    let mut bootstrap_rpg = load_rpg();
    let mut bootstrap_quests = load_quests();
    let today_str = today();
    let initial_muted = load_sfx_muted();
    ensure_today_quests(&mut bootstrap_quests, &today_str);
    let streak_update = touch_day(&mut bootstrap_rpg, &today_str);
    let streak_msg = if streak_update.new_streak {
        Some(format!(
            "// day {} streak · +{} xp //",
            bootstrap_rpg.streak, streak_update.bonus_xp
        ))
    } else {
        None
    };
    let streak_level_up = streak_update.leveled_up;

    let (rpg, set_rpg) = signal(bootstrap_rpg);
    let (quests, set_quests) = signal(bootstrap_quests);
    let (sfx_muted, set_sfx_muted) = signal(initial_muted);
    let (event_msg, set_event_msg) = signal(Option::<String>::None);

    sfx_mute(initial_muted);
    if let Some(m) = streak_msg {
        flash_message(set_event_msg, m);
        if streak_level_up {
            sfx_play("level");
        } else {
            sfx_play("xp");
        }
    }

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
    Effect::new(move |_| {
        persist_rpg(&rpg.get());
    });
    Effect::new(move |_| {
        persist_quests(&quests.get());
    });
    Effect::new(move |_| {
        persist_sfx_muted(sfx_muted.get());
    });

    Effect::new(move |_| {
        let notes_now = notes.get();
        let mut total_chars: u64 = 0;
        let mut total_wikilinks: u32 = 0;
        let mut total_tags: u32 = 0;
        let mut total_images: u32 = 0;
        for n in &notes_now {
            total_chars +=
                n.body.chars().count() as u64 + n.title.chars().count() as u64;
            total_wikilinks += extract_wikilinks(&n.body).len() as u32;
            total_tags += extract_tags(&n.body).len() as u32;
            total_images += count_images(&n.body);
        }
        let total_notes = notes_now.len() as u32;

        let mut rpg_state = rpg.get_untracked();
        let mut quest_state = quests.get_untracked();

        if !rpg_state.initialized {
            rpg_state.total_chars = total_chars;
            rpg_state.total_wikilinks = total_wikilinks;
            rpg_state.total_tags = total_tags;
            rpg_state.total_images = total_images;
            rpg_state.total_notes = total_notes;
            rpg_state.total_manual_links = manual_edges.get_untracked().len() as u32;
            rpg_state.total_tracks = tracks.get_untracked().len() as u32;
            rpg_state.initialized = true;
            set_rpg.set(rpg_state);
            return;
        }

        let chars_delta = total_chars.saturating_sub(rpg_state.total_chars);
        let wikilink_delta = total_wikilinks.saturating_sub(rpg_state.total_wikilinks);
        let tag_delta = total_tags.saturating_sub(rpg_state.total_tags);
        let image_delta = total_images.saturating_sub(rpg_state.total_images);
        let notes_delta = total_notes.saturating_sub(rpg_state.total_notes);

        rpg_state.total_chars = total_chars;
        rpg_state.total_wikilinks = total_wikilinks;
        rpg_state.total_tags = total_tags;
        rpg_state.total_images = total_images;
        rpg_state.total_notes = total_notes;

        if chars_delta == 0
            && wikilink_delta == 0
            && tag_delta == 0
            && image_delta == 0
            && notes_delta == 0
        {
            set_rpg.set(rpg_state);
            return;
        }

        let prev_level = rpg_state.level();
        let mut xp_gain: u64 = 0;
        let mut gold_gain: u64 = 0;
        if chars_delta > 0 {
            xp_gain += chars_delta / 80;
        }
        if wikilink_delta > 0 {
            xp_gain += wikilink_delta as u64 * 5;
            gold_gain += wikilink_delta as u64;
        }
        if tag_delta > 0 {
            xp_gain += tag_delta as u64 * 3;
        }
        if image_delta > 0 {
            xp_gain += image_delta as u64 * 2;
        }
        if notes_delta > 0 {
            xp_gain += notes_delta as u64 * 5;
            gold_gain += notes_delta as u64 * 2;
        }
        rpg_state.xp = rpg_state.xp.saturating_add(xp_gain);
        rpg_state.gold = rpg_state.gold.saturating_add(gold_gain);

        let mut completed_titles: Vec<String> = Vec::new();
        if chars_delta > 0 {
            let amt = chars_delta.min(u32::MAX as u64) as u32;
            if let Some(t) =
                progress_quest(&mut rpg_state, &mut quest_state, QuestKind::WriteChars, amt)
            {
                completed_titles.push(t);
            }
        }
        if wikilink_delta > 0 {
            if let Some(t) = progress_quest(
                &mut rpg_state,
                &mut quest_state,
                QuestKind::AddWikilinks,
                wikilink_delta,
            ) {
                completed_titles.push(t);
            }
        }
        if tag_delta > 0 {
            if let Some(t) =
                progress_quest(&mut rpg_state, &mut quest_state, QuestKind::AddTags, tag_delta)
            {
                completed_titles.push(t);
            }
        }
        if image_delta > 0 {
            if let Some(t) = progress_quest(
                &mut rpg_state,
                &mut quest_state,
                QuestKind::InsertImages,
                image_delta,
            ) {
                completed_titles.push(t);
            }
        }
        if notes_delta > 0 {
            if let Some(t) = progress_quest(
                &mut rpg_state,
                &mut quest_state,
                QuestKind::CreateNotes,
                notes_delta,
            ) {
                completed_titles.push(t);
            }
        }

        let leveled = rpg_state.level() > prev_level;
        set_rpg.set(rpg_state);
        set_quests.set(quest_state);

        if leveled {
            sfx_play("level");
            flash_message(set_event_msg, "// LEVEL UP //".to_string());
        }
        if let Some(t) = completed_titles.first() {
            sfx_play("quest");
            flash_message(set_event_msg, format!("// quest cleared: {} //", t));
        }
    });

    Effect::new(move |_| {
        let edges_now = manual_edges.get();
        let count = edges_now.len() as u32;
        let mut rpg_state = rpg.get_untracked();
        let mut quest_state = quests.get_untracked();
        if !rpg_state.initialized {
            rpg_state.total_manual_links = count;
            set_rpg.set(rpg_state);
            return;
        }
        let delta = count.saturating_sub(rpg_state.total_manual_links);
        rpg_state.total_manual_links = count;
        if delta == 0 {
            set_rpg.set(rpg_state);
            return;
        }
        let prev_level = rpg_state.level();
        rpg_state.xp = rpg_state.xp.saturating_add(delta as u64 * 4);
        rpg_state.gold = rpg_state.gold.saturating_add(delta as u64);
        let completed = progress_quest(
            &mut rpg_state,
            &mut quest_state,
            QuestKind::LinkManual,
            delta,
        );
        let leveled = rpg_state.level() > prev_level;
        set_rpg.set(rpg_state);
        set_quests.set(quest_state);
        if leveled {
            sfx_play("level");
            flash_message(set_event_msg, "// LEVEL UP //".to_string());
        } else if let Some(t) = completed {
            sfx_play("quest");
            flash_message(set_event_msg, format!("// quest cleared: {} //", t));
        } else {
            sfx_play("xp");
        }
    });

    Effect::new(move |_| {
        let tracks_now = tracks.get();
        let count = tracks_now.len() as u32;
        let mut rpg_state = rpg.get_untracked();
        let mut quest_state = quests.get_untracked();
        if !rpg_state.initialized {
            rpg_state.total_tracks = count;
            set_rpg.set(rpg_state);
            return;
        }
        let delta = count.saturating_sub(rpg_state.total_tracks);
        rpg_state.total_tracks = count;
        if delta == 0 {
            set_rpg.set(rpg_state);
            return;
        }
        let prev_level = rpg_state.level();
        rpg_state.xp = rpg_state.xp.saturating_add(delta as u64 * 10);
        rpg_state.gold = rpg_state.gold.saturating_add(delta as u64 * 5);
        let completed = progress_quest(
            &mut rpg_state,
            &mut quest_state,
            QuestKind::RecordAudio,
            delta,
        );
        let leveled = rpg_state.level() > prev_level;
        set_rpg.set(rpg_state);
        set_quests.set(quest_state);
        if leveled {
            sfx_play("level");
            flash_message(set_event_msg, "// LEVEL UP //".to_string());
        } else if let Some(t) = completed {
            sfx_play("quest");
            flash_message(set_event_msg, format!("// quest cleared: {} //", t));
        } else {
            sfx_play("gold");
        }
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
                    let mut rpg_state = rpg.get_untracked();
                    let mut quest_state = quests.get_untracked();
                    let prev_level = rpg_state.level();
                    rpg_state.total_posts_published =
                        rpg_state.total_posts_published.saturating_add(1);
                    rpg_state.xp = rpg_state.xp.saturating_add(25);
                    rpg_state.gold = rpg_state.gold.saturating_add(20);
                    let completed = progress_quest(
                        &mut rpg_state,
                        &mut quest_state,
                        QuestKind::Publish,
                        1,
                    );
                    let leveled = rpg_state.level() > prev_level;
                    set_rpg.set(rpg_state);
                    set_quests.set(quest_state);
                    if leveled {
                        sfx_play("level");
                        flash_message(set_event_msg, "// LEVEL UP //".to_string());
                    } else if let Some(t) = completed {
                        sfx_play("quest");
                        flash_message(set_event_msg, format!("// quest cleared: {} //", t));
                    } else {
                        sfx_play("gold");
                        flash_message(set_event_msg, "// +25 xp · +20 gold //".to_string());
                    }
                }
                Err(e) => {
                    sfx_play("err");
                    set_publish_status.set(Some(format!("// publish failed: {} //", e)));
                }
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
            <Hud rpg=rpg quests=quests sfx_muted=sfx_muted set_sfx_muted=set_sfx_muted event_msg=event_msg set_view=set_view />
            <header class="masthead">
                <h1 class="title">"NOTEPUNK"</h1>
                <p class="tagline">"// capture · loop · publish · level up //"</p>
                <nav class="tabs">
                    <button class:active=move || view.get() == View::Notes
                            on:click=move |_| { set_view.set(View::Notes); sfx_play("tap"); }>"notes"</button>
                    <button class:active=move || view.get() == View::Graph
                            on:click=move |_| { set_view.set(View::Graph); sfx_play("tap"); }>"graph"</button>
                    <button class:active=move || view.get() == View::Quests
                            on:click=move |_| { set_view.set(View::Quests); sfx_play("tap"); }>"quests"</button>
                    <button class:active=move || view.get() == View::Board
                            on:click=move |_| { set_view.set(View::Board); sfx_play("tap"); }>"board"</button>
                    <button class:active=move || view.get() == View::Guide
                            on:click=move |_| { set_view.set(View::Guide); sfx_play("tap"); }>"guide"</button>
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
                                    format!("{} wikilinks · {} tags · {} similar · {} manual · {} auto",
                                        s.wikilinks, s.tags, s.similarity, s.manual, s.auto)
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
                                    <label class="ctrl-toggle">
                                        <input type="checkbox"
                                            prop:checked=move || graph_cfg.get().include_auto
                                            on:change=move |ev| {
                                                let v = event_target_checked(&ev);
                                                set_graph_cfg.update(|c| c.include_auto = v);
                                            }
                                        />
                                        <span class="kind-swatch auto"></span>"auto"
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

                View::Quests => {
                    let notes_for_skills = notes.get();
                    let mut tag_uses: HashMap<String, u32> = HashMap::new();
                    let mut tag_notes: HashMap<String, Vec<String>> = HashMap::new();
                    for n in &notes_for_skills {
                        for t in extract_tags(&n.body) {
                            *tag_uses.entry(t.clone()).or_insert(0) += 1;
                            tag_notes
                                .entry(t)
                                .or_insert_with(Vec::new)
                                .push(n.display_title());
                        }
                    }
                    let mut skill_rows: Vec<(String, u32, u32, Vec<String>)> = tag_uses
                        .into_iter()
                        .map(|(t, uses)| {
                            let lvl = skill_level(uses);
                            let titles = tag_notes.remove(&t).unwrap_or_default();
                            (t, lvl, uses, titles)
                        })
                        .collect();
                    skill_rows.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)).then(a.0.cmp(&b.0)));
                    view! {
                        <div class="quests-frame">
                            <div class="quests-grid">
                                <section class="quest-panel">
                                    <h2 class="panel-h">"// today's quests //"</h2>
                                    <p class="dim quest-date">{format!("// {} //", today())}</p>
                                    {move || {
                                        let qs = quests.get();
                                        if qs.quests.is_empty() {
                                            view! { <p class="dim">"// no quests today //"</p> }.into_any()
                                        } else {
                                            view! {
                                                <ul class="quest-list">
                                                    {qs.quests.into_iter().map(|q| {
                                                        let pct = if q.target == 0 { 1.0 } else {
                                                            (q.progress as f64 / q.target as f64).clamp(0.0, 1.0)
                                                        };
                                                        let done = q.claimed;
                                                        let prog_label = format!("{}/{}", q.progress.min(q.target), q.target);
                                                        let reward = format!("+{} xp · +{} ⌬", q.xp_reward, q.gold_reward);
                                                        view! {
                                                            <li class="quest-row" class:done=done>
                                                                <div class="quest-title-row">
                                                                    <span class="quest-marker">{ if done { "✓" } else { "▢" } }</span>
                                                                    <span class="quest-title">{q.title()}</span>
                                                                    <span class="quest-prog dim">{prog_label}</span>
                                                                </div>
                                                                <div class="quest-bar">
                                                                    <div class="quest-fill"
                                                                         style:width=format!("{:.0}%", pct * 100.0)></div>
                                                                </div>
                                                                <div class="quest-reward dim">{reward}</div>
                                                            </li>
                                                        }
                                                    }).collect_view()}
                                                </ul>
                                            }.into_any()
                                        }
                                    }}
                                </section>
                                <section class="quest-panel">
                                    <h2 class="panel-h">"// player //"</h2>
                                    <ul class="stat-list">
                                        <li>"level " <strong>{move || rpg.get().level().to_string()}</strong></li>
                                        <li>"xp " <strong>{move || rpg.get().xp.to_string()}</strong></li>
                                        <li>"gold ⌬ " <strong>{move || rpg.get().gold.to_string()}</strong></li>
                                        <li>"streak ※ " <strong>{move || format!("{} days", rpg.get().streak)}</strong>
                                            " (best " {move || rpg.get().best_streak.to_string()} ")"</li>
                                        <li class="dim">"notes " {move || rpg.get().total_notes.to_string()}
                                            " · wikilinks " {move || rpg.get().total_wikilinks.to_string()}
                                            " · tags " {move || rpg.get().total_tags.to_string()}
                                            " · tracks " {move || rpg.get().total_tracks.to_string()}
                                            " · published " {move || rpg.get().total_posts_published.to_string()}</li>
                                    </ul>
                                </section>
                                <section class="quest-panel skills-panel">
                                    <h2 class="panel-h">"// skill tree //"</h2>
                                    <p class="dim">"// each #tag is a skill. write more notes with it to level up. //"</p>
                                    {if skill_rows.is_empty() {
                                        view! { <p class="dim">"// no skills yet — drop a #tag into a note //"</p> }.into_any()
                                    } else {
                                        view! {
                                            <ul class="skill-list">
                                                {skill_rows.into_iter().map(|(tag, lvl, uses, titles)| {
                                                    let bar_pct = (uses as f64 / (uses as f64 + 4.0).max(1.0)) * 100.0;
                                                    let preview = titles.iter().take(3).cloned().collect::<Vec<_>>().join(" · ");
                                                    view! {
                                                        <li class="skill-row">
                                                            <div class="skill-head">
                                                                <span class="skill-tag">{format!("#{}", tag)}</span>
                                                                <span class="skill-lvl">{format!("lvl {}", lvl)}</span>
                                                                <span class="skill-uses dim">{format!("{} notes", uses)}</span>
                                                            </div>
                                                            <div class="skill-bar">
                                                                <div class="skill-fill"
                                                                     style:width=format!("{:.0}%", bar_pct)></div>
                                                            </div>
                                                            {(!preview.is_empty()).then(|| view! {
                                                                <div class="skill-preview dim">{preview}</div>
                                                            })}
                                                        </li>
                                                    }
                                                }).collect_view()}
                                            </ul>
                                        }.into_any()
                                    }}
                                </section>
                            </div>
                        </div>
                    }.into_any()
                }

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
