use crate::model::{EdgeKind, GraphConfig, ManualEdge, Note};
use crate::rpg::skill_level;
use crate::similarity::{cosine, extract_first_image, extract_tags, extract_wikilinks, TfIdfModel};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Serialize)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub degree: u32,
    pub recency: f64,
    pub tag_count: u32,
    pub image: String,
    pub skill_level: u32,
    pub top_tag: String,
}

#[derive(Serialize)]
pub struct GraphEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    pub kind: &'static str,
    pub weight: f64,
}

pub struct GraphPayload {
    pub nodes_json: String,
    pub edges_json: String,
    pub stats: GraphStats,
}

#[derive(Clone)]
pub struct GraphStats {
    pub wikilinks: usize,
    pub tags: usize,
    pub similarity: usize,
    pub manual: usize,
    pub auto: usize,
}

pub fn build(notes: &[Note], cfg: &GraphConfig, manual: &[ManualEdge]) -> GraphPayload {
    let title_to_id: HashMap<String, String> = notes
        .iter()
        .map(|n| (n.display_title().to_lowercase(), n.id.clone()))
        .collect();

    let id_set: HashSet<&str> = notes.iter().map(|n| n.id.as_str()).collect();

    let tags_per_note: HashMap<String, Vec<String>> = notes
        .iter()
        .map(|n| (n.id.clone(), extract_tags(&n.body)))
        .collect();

    let mut tag_usage: HashMap<String, u32> = HashMap::new();
    for tags in tags_per_note.values() {
        for t in tags {
            *tag_usage.entry(t.clone()).or_insert(0) += 1;
        }
    }

    let mut edges: Vec<GraphEdge> = Vec::new();
    let mut idx: u64 = 0;
    let mut stats = GraphStats {
        wikilinks: 0,
        tags: 0,
        similarity: 0,
        manual: 0,
        auto: 0,
    };

    let mut connected: HashSet<String> = HashSet::new();

    if cfg.include_wikilinks {
        for n in notes {
            for link in extract_wikilinks(&n.body) {
                if let Some(target_id) = title_to_id.get(&link.to_lowercase()) {
                    if target_id != &n.id {
                        edges.push(GraphEdge {
                            id: format!("e{}", idx),
                            source: n.id.clone(),
                            target: target_id.clone(),
                            kind: EdgeKind::Wikilink.as_str(),
                            weight: 1.0,
                        });
                        idx += 1;
                        stats.wikilinks += 1;
                    }
                }
            }
        }
    }

    if cfg.include_tags {
        for i in 0..notes.len() {
            for j in (i + 1)..notes.len() {
                let a = &notes[i];
                let b = &notes[j];
                let ta = tags_per_note.get(&a.id);
                let tb = tags_per_note.get(&b.id);
                let (Some(ta), Some(tb)) = (ta, tb) else { continue };
                if ta.is_empty() || tb.is_empty() {
                    continue;
                }
                let shared: usize = ta.iter().filter(|t| tb.contains(t)).count();
                if shared == 0 {
                    continue;
                }
                let union: usize = ta.len() + tb.len() - shared;
                let jaccard = shared as f64 / union as f64;
                edges.push(GraphEdge {
                    id: format!("e{}", idx),
                    source: a.id.clone(),
                    target: b.id.clone(),
                    kind: EdgeKind::Tag.as_str(),
                    weight: jaccard,
                });
                idx += 1;
                stats.tags += 1;
            }
        }
    }

    let mut vectors: Vec<HashMap<String, f64>> = Vec::new();
    if notes.len() >= 2 && (cfg.include_similarity || cfg.include_auto) {
        let model = TfIdfModel::build(notes);
        vectors = notes
            .iter()
            .map(|n| model.vector(&n.title, &n.body))
            .collect();
    }

    if cfg.include_similarity && notes.len() >= 2 {
        for i in 0..notes.len() {
            for j in (i + 1)..notes.len() {
                let score = cosine(&vectors[i], &vectors[j]);
                if score >= cfg.similarity_threshold {
                    edges.push(GraphEdge {
                        id: format!("e{}", idx),
                        source: notes[i].id.clone(),
                        target: notes[j].id.clone(),
                        kind: EdgeKind::Similarity.as_str(),
                        weight: score,
                    });
                    idx += 1;
                    stats.similarity += 1;
                }
            }
        }
    }

    if cfg.include_manual {
        for m in manual {
            if !id_set.contains(m.source.as_str()) || !id_set.contains(m.target.as_str()) {
                continue;
            }
            if m.source == m.target {
                continue;
            }
            edges.push(GraphEdge {
                id: format!("e{}", idx),
                source: m.source.clone(),
                target: m.target.clone(),
                kind: EdgeKind::Manual.as_str(),
                weight: 1.0,
            });
            idx += 1;
            stats.manual += 1;
        }
    }

    if cfg.include_auto && notes.len() >= 2 {
        for e in &edges {
            connected.insert(e.source.clone());
            connected.insert(e.target.clone());
        }
        let mut existing: HashSet<(String, String)> = HashSet::new();
        for e in &edges {
            let (a, b) = if e.source < e.target {
                (e.source.clone(), e.target.clone())
            } else {
                (e.target.clone(), e.source.clone())
            };
            existing.insert((a, b));
        }

        for (i, n) in notes.iter().enumerate() {
            if connected.contains(&n.id) {
                continue;
            }
            let mut best: Option<(usize, f64)> = None;
            for (j, other) in notes.iter().enumerate() {
                if i == j {
                    continue;
                }
                let score = if !vectors.is_empty() {
                    cosine(&vectors[i], &vectors[j])
                } else {
                    0.0
                };
                let bumped = if score <= 0.0 {
                    let age_gap = (n.updated_at - other.updated_at).abs();
                    1.0 / (1.0 + age_gap / 86_400_000.0)
                } else {
                    score
                };
                if best.map(|(_, s)| bumped > s).unwrap_or(true) {
                    best = Some((j, bumped));
                }
            }
            if let Some((j, score)) = best {
                let (a, b) = if n.id < notes[j].id {
                    (n.id.clone(), notes[j].id.clone())
                } else {
                    (notes[j].id.clone(), n.id.clone())
                };
                if !existing.contains(&(a.clone(), b.clone())) {
                    edges.push(GraphEdge {
                        id: format!("e{}", idx),
                        source: a.clone(),
                        target: b.clone(),
                        kind: EdgeKind::Auto.as_str(),
                        weight: score.max(0.05).min(0.5),
                    });
                    idx += 1;
                    stats.auto += 1;
                    existing.insert((a.clone(), b.clone()));
                    connected.insert(a);
                    connected.insert(b);
                }
            }
        }
    }

    let mut degree: HashMap<String, u32> = HashMap::new();
    for e in &edges {
        *degree.entry(e.source.clone()).or_insert(0) += 1;
        *degree.entry(e.target.clone()).or_insert(0) += 1;
    }

    let now = js_sys::Date::now();
    let day_ms = 86_400_000.0;
    let max_age_days: f64 = 30.0;

    let nodes: Vec<GraphNode> = notes
        .iter()
        .map(|n| {
            let age_days = ((now - n.updated_at) / day_ms).max(0.0);
            let recency = (1.0 - (age_days / max_age_days).min(1.0)).max(0.0);
            let tag_list = tags_per_note.get(&n.id).cloned().unwrap_or_default();
            let tag_count = tag_list.len() as u32;
            let image = if cfg.show_thumbnails {
                extract_first_image(&n.body).unwrap_or_default()
            } else {
                String::new()
            };
            let (top_tag, top_uses) = tag_list
                .iter()
                .map(|t| (t.clone(), *tag_usage.get(t).unwrap_or(&1)))
                .max_by_key(|(_, u)| *u)
                .unwrap_or((String::new(), 0));
            let skill_lv = if top_uses > 0 { skill_level(top_uses) } else { 0 };
            GraphNode {
                id: n.id.clone(),
                label: n.display_title(),
                degree: *degree.get(&n.id).unwrap_or(&0),
                recency,
                tag_count,
                image,
                skill_level: skill_lv,
                top_tag,
            }
        })
        .collect();

    GraphPayload {
        nodes_json: serde_json::to_string(&nodes).unwrap_or_else(|_| "[]".into()),
        edges_json: serde_json::to_string(&edges).unwrap_or_else(|_| "[]".into()),
        stats,
    }
}
