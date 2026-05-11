use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use web_sys::{window, HtmlAnchorElement};

pub const STORAGE_NOTES: &str = "notepunk:notes";
pub const STORAGE_CURRENT: &str = "notepunk:current";
pub const STORAGE_MANUAL_EDGES: &str = "notepunk:manual_edges";
pub const STORAGE_GRAPH_CONFIG: &str = "notepunk:graph_config";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Note {
    pub id: String,
    pub title: String,
    pub body: String,
    pub updated_at: f64,
}

impl Note {
    pub fn fresh() -> Self {
        let now = js_sys::Date::now();
        let rand = (js_sys::Math::random() * 1e9) as u32;
        Self {
            id: format!("{}-{}", now as u64, rand),
            title: String::new(),
            body: String::new(),
            updated_at: now,
        }
    }

    pub fn display_title(&self) -> String {
        if self.title.trim().is_empty() {
            "untitled".to_string()
        } else {
            self.title.clone()
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum EdgeKind {
    Wikilink,
    Tag,
    Similarity,
    Manual,
    Auto,
}

impl EdgeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            EdgeKind::Wikilink => "wikilink",
            EdgeKind::Tag => "tag",
            EdgeKind::Similarity => "similarity",
            EdgeKind::Manual => "manual",
            EdgeKind::Auto => "auto",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ManualEdge {
    pub source: String,
    pub target: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct GraphConfig {
    pub similarity_threshold: f64,
    pub include_wikilinks: bool,
    pub include_tags: bool,
    pub include_similarity: bool,
    pub include_manual: bool,
    pub include_auto: bool,
    pub show_thumbnails: bool,
}

impl Default for GraphConfig {
    fn default() -> Self {
        Self {
            similarity_threshold: 0.18,
            include_wikilinks: true,
            include_tags: true,
            include_similarity: true,
            include_manual: true,
            include_auto: true,
            show_thumbnails: true,
        }
    }
}

fn local_storage() -> Option<web_sys::Storage> {
    window()?.local_storage().ok().flatten()
}

pub fn load_notes() -> Vec<Note> {
    local_storage()
        .and_then(|s| s.get_item(STORAGE_NOTES).ok().flatten())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn persist_notes(notes: &[Note]) {
    if let (Ok(json), Some(s)) = (serde_json::to_string(notes), local_storage()) {
        let _ = s.set_item(STORAGE_NOTES, &json);
    }
}

pub fn load_current() -> Option<String> {
    local_storage().and_then(|s| s.get_item(STORAGE_CURRENT).ok().flatten())
}

pub fn persist_current(id: &Option<String>) {
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

pub fn load_manual_edges() -> Vec<ManualEdge> {
    local_storage()
        .and_then(|s| s.get_item(STORAGE_MANUAL_EDGES).ok().flatten())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn persist_manual_edges(edges: &[ManualEdge]) {
    if let (Ok(json), Some(s)) = (serde_json::to_string(edges), local_storage()) {
        let _ = s.set_item(STORAGE_MANUAL_EDGES, &json);
    }
}

pub fn load_graph_config() -> GraphConfig {
    local_storage()
        .and_then(|s| s.get_item(STORAGE_GRAPH_CONFIG).ok().flatten())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn persist_graph_config(cfg: &GraphConfig) {
    if let (Ok(json), Some(s)) = (serde_json::to_string(cfg), local_storage()) {
        let _ = s.set_item(STORAGE_GRAPH_CONFIG, &json);
    }
}

pub fn download_markdown(note: &Note) {
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
