use serde::{Deserialize, Serialize};
use web_sys::window;

pub const STORAGE_RPG: &str = "notepunk:rpg";
pub const STORAGE_QUESTS: &str = "notepunk:quests";
pub const STORAGE_SFX: &str = "notepunk:sfx";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RpgState {
    pub xp: u64,
    pub gold: u64,
    pub streak: u32,
    pub best_streak: u32,
    pub last_active_day: String,
    pub total_chars: u64,
    pub total_wikilinks: u32,
    pub total_tags: u32,
    pub total_tracks: u32,
    pub total_posts_published: u32,
    pub total_manual_links: u32,
    pub total_images: u32,
    pub total_notes: u32,
    pub initialized: bool,
}

impl Default for RpgState {
    fn default() -> Self {
        Self {
            xp: 0,
            gold: 0,
            streak: 0,
            best_streak: 0,
            last_active_day: String::new(),
            total_chars: 0,
            total_wikilinks: 0,
            total_tags: 0,
            total_tracks: 0,
            total_posts_published: 0,
            total_manual_links: 0,
            total_images: 0,
            total_notes: 0,
            initialized: false,
        }
    }
}

impl RpgState {
    pub fn level(&self) -> u32 {
        ((self.xp as f64 / 100.0).sqrt().floor() as u32) + 1
    }

    pub fn xp_into_level(&self) -> u64 {
        let l = self.level() as u64;
        let base = (l - 1) * (l - 1) * 100;
        self.xp.saturating_sub(base)
    }

    pub fn xp_for_level(&self) -> u64 {
        let l = self.level() as u64;
        let next = l * l * 100;
        let base = (l - 1) * (l - 1) * 100;
        next - base
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum QuestKind {
    WriteChars,
    AddWikilinks,
    AddTags,
    RecordAudio,
    Publish,
    InsertImages,
    LinkManual,
    CreateNotes,
}

impl QuestKind {
    pub fn all() -> &'static [QuestKind] {
        &[
            QuestKind::WriteChars,
            QuestKind::AddWikilinks,
            QuestKind::AddTags,
            QuestKind::RecordAudio,
            QuestKind::Publish,
            QuestKind::InsertImages,
            QuestKind::LinkManual,
            QuestKind::CreateNotes,
        ]
    }

    pub fn template(&self) -> (&'static str, u32, u32, u32) {
        match self {
            QuestKind::WriteChars => ("write {} characters", 400, 60, 12),
            QuestKind::AddWikilinks => ("forge {} [[wikilinks]]", 3, 70, 18),
            QuestKind::AddTags => ("plant {} #tags", 4, 50, 12),
            QuestKind::RecordAudio => ("record {} audio loops", 1, 50, 15),
            QuestKind::Publish => ("publish {} note to the board", 1, 120, 30),
            QuestKind::InsertImages => ("scavenge {} images", 2, 40, 10),
            QuestKind::LinkManual => ("draw {} manual edges in the graph", 2, 60, 15),
            QuestKind::CreateNotes => ("start {} new notes", 2, 50, 10),
        }
    }

    pub fn render_title(&self, target: u32) -> String {
        let (tmpl, _, _, _) = self.template();
        tmpl.replacen("{}", &target.to_string(), 1)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Quest {
    pub kind: QuestKind,
    pub target: u32,
    pub progress: u32,
    pub xp_reward: u32,
    pub gold_reward: u32,
    pub claimed: bool,
}

impl Quest {
    pub fn fresh(kind: QuestKind) -> Self {
        let (_, target, xp, gold) = kind.template();
        Self {
            kind,
            target,
            progress: 0,
            xp_reward: xp,
            gold_reward: gold,
            claimed: false,
        }
    }

    pub fn done(&self) -> bool {
        self.progress >= self.target
    }

    pub fn title(&self) -> String {
        self.kind.render_title(self.target)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct QuestState {
    pub day: String,
    pub quests: Vec<Quest>,
}

fn local_storage() -> Option<web_sys::Storage> {
    window()?.local_storage().ok().flatten()
}

pub fn load_rpg() -> RpgState {
    local_storage()
        .and_then(|s| s.get_item(STORAGE_RPG).ok().flatten())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn persist_rpg(state: &RpgState) {
    if let (Ok(json), Some(s)) = (serde_json::to_string(state), local_storage()) {
        let _ = s.set_item(STORAGE_RPG, &json);
    }
}

pub fn load_quests() -> QuestState {
    local_storage()
        .and_then(|s| s.get_item(STORAGE_QUESTS).ok().flatten())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn persist_quests(qs: &QuestState) {
    if let (Ok(json), Some(s)) = (serde_json::to_string(qs), local_storage()) {
        let _ = s.set_item(STORAGE_QUESTS, &json);
    }
}

pub fn load_sfx_muted() -> bool {
    local_storage()
        .and_then(|s| s.get_item(STORAGE_SFX).ok().flatten())
        .map(|v| v == "1")
        .unwrap_or(false)
}

pub fn persist_sfx_muted(muted: bool) {
    if let Some(s) = local_storage() {
        let _ = s.set_item(STORAGE_SFX, if muted { "1" } else { "0" });
    }
}

pub fn today() -> String {
    let d = js_sys::Date::new_0();
    format!(
        "{:04}-{:02}-{:02}",
        d.get_full_year(),
        (d.get_month() as u32) + 1,
        d.get_date() as u32
    )
}

fn day_seed(day: &str) -> u64 {
    let mut h: u64 = 14_695_981_039_346_656_037;
    for b in day.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(1_099_511_628_211);
    }
    h
}

pub fn pick_quests_for_day(day: &str) -> Vec<Quest> {
    let kinds = QuestKind::all();
    let mut seed = day_seed(day);
    let mut chosen = Vec::with_capacity(3);
    let mut tried = Vec::new();
    while chosen.len() < 3 && tried.len() < kinds.len() {
        let idx = (seed % kinds.len() as u64) as usize;
        if !tried.contains(&idx) {
            tried.push(idx);
            chosen.push(Quest::fresh(kinds[idx]));
        }
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
    }
    chosen
}

pub fn ensure_today_quests(qs: &mut QuestState, day: &str) -> bool {
    if qs.day != day {
        qs.day = day.to_string();
        qs.quests = pick_quests_for_day(day);
        true
    } else if qs.quests.is_empty() {
        qs.quests = pick_quests_for_day(day);
        true
    } else {
        false
    }
}

fn day_offset(day: &str) -> Option<i64> {
    let parts: Vec<&str> = day.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let y: i32 = parts[0].parse().ok()?;
    let m: i32 = parts[1].parse().ok()?;
    let d: i32 = parts[2].parse().ok()?;
    let iso = format!("{:04}-{:02}-{:02}T00:00:00Z", y, m, d);
    let js_val = wasm_bindgen::JsValue::from_str(&iso);
    let date = js_sys::Date::new(&js_val);
    let ms = date.get_time();
    if ms.is_nan() {
        return None;
    }
    Some((ms / 86_400_000.0) as i64)
}

pub fn day_diff(a: &str, b: &str) -> Option<i64> {
    Some(day_offset(a)? - day_offset(b)?)
}

pub struct StreakUpdate {
    pub leveled_up: bool,
    pub new_streak: bool,
    pub bonus_xp: u32,
}

pub fn touch_day(state: &mut RpgState, today_str: &str) -> StreakUpdate {
    if state.last_active_day == today_str {
        return StreakUpdate {
            leveled_up: false,
            new_streak: false,
            bonus_xp: 0,
        };
    }
    let prev_level = state.level();
    let diff = if state.last_active_day.is_empty() {
        None
    } else {
        day_diff(today_str, &state.last_active_day)
    };
    match diff {
        Some(1) => {
            state.streak = state.streak.saturating_add(1);
        }
        Some(d) if d <= 0 => {}
        _ => {
            state.streak = 1;
        }
    }
    if state.streak == 0 {
        state.streak = 1;
    }
    if state.streak > state.best_streak {
        state.best_streak = state.streak;
    }
    state.last_active_day = today_str.to_string();
    let bonus = 10u32.saturating_add(state.streak.saturating_mul(2));
    state.xp = state.xp.saturating_add(bonus as u64);
    state.gold = state.gold.saturating_add(state.streak as u64);
    let new_level = state.level();
    StreakUpdate {
        leveled_up: new_level > prev_level,
        new_streak: true,
        bonus_xp: bonus,
    }
}

pub fn progress_quest(
    state: &mut RpgState,
    quests: &mut QuestState,
    kind: QuestKind,
    amount: u32,
) -> Option<String> {
    let mut completed_title = None;
    for q in &mut quests.quests {
        if q.kind == kind && !q.claimed {
            let was_done = q.done();
            q.progress = q.progress.saturating_add(amount).min(q.target * 2);
            if !was_done && q.done() {
                let xp = q.xp_reward;
                let gold = q.gold_reward;
                q.claimed = true;
                state.xp = state.xp.saturating_add(xp as u64);
                state.gold = state.gold.saturating_add(gold as u64);
                completed_title = Some(q.title());
            }
            break;
        }
    }
    completed_title
}

pub fn skill_level(uses: u32) -> u32 {
    let l = ((uses as f64).sqrt() / 1.4).floor() as u32 + 1;
    l.min(20)
}
