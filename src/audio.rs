use serde::{Deserialize, Serialize};
use web_sys::window;

pub const STORAGE_TRACKS: &str = "notepunk:tracks";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AudioTrack {
    pub id: String,
    pub note_id: String,
    pub name: String,
    pub duration_ms: f64,
    pub created_at: f64,
    #[serde(default = "default_volume")]
    pub volume: f64,
    #[serde(default)]
    pub looping: bool,
}

fn default_volume() -> f64 {
    0.8
}

impl AudioTrack {
    pub fn fresh(note_id: String, name: String, duration_ms: f64) -> Self {
        let now = js_sys::Date::now();
        let rand = (js_sys::Math::random() * 1e9) as u32;
        Self {
            id: format!("trk-{}-{}", now as u64, rand),
            note_id,
            name,
            duration_ms,
            created_at: now,
            volume: 0.8,
            looping: false,
        }
    }
}

fn local_storage() -> Option<web_sys::Storage> {
    window()?.local_storage().ok().flatten()
}

pub fn load_tracks() -> Vec<AudioTrack> {
    local_storage()
        .and_then(|s| s.get_item(STORAGE_TRACKS).ok().flatten())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn persist_tracks(tracks: &[AudioTrack]) {
    if let (Ok(json), Some(s)) = (serde_json::to_string(tracks), local_storage()) {
        let _ = s.set_item(STORAGE_TRACKS, &json);
    }
}
