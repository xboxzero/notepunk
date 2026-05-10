use wasm_bindgen::JsValue;
use web_sys::window;

pub struct SupabaseConfig {
    pub url: String,
    pub anon_key: String,
}

impl SupabaseConfig {
    pub fn from_window() -> Option<Self> {
        let win = window()?;
        let cfg = js_sys::Reflect::get(&win, &JsValue::from_str("NOTEPUNK_CONFIG")).ok()?;
        if cfg.is_undefined() || cfg.is_null() {
            return None;
        }
        let url = js_sys::Reflect::get(&cfg, &JsValue::from_str("supabase_url"))
            .ok()
            .and_then(|v| v.as_string())
            .filter(|s| !s.is_empty())?;
        let anon_key = js_sys::Reflect::get(&cfg, &JsValue::from_str("supabase_anon_key"))
            .ok()
            .and_then(|v| v.as_string())
            .filter(|s| !s.is_empty())?;
        Some(Self { url, anon_key })
    }

    pub fn rest_url(&self, path: &str) -> String {
        format!("{}/rest/v1/{}", self.url.trim_end_matches('/'), path)
    }
}

pub fn handle_storage_key() -> &'static str {
    "notepunk:author_handle"
}

pub fn load_handle() -> String {
    window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item(handle_storage_key()).ok().flatten())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "anon".to_string())
}

pub fn persist_handle(handle: &str) {
    if let Some(s) = window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = s.set_item(handle_storage_key(), handle);
    }
}
