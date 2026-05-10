use crate::model::{EdgeKind, GraphConfig, ManualEdge, Note};
use crate::similarity::{cosine, extract_first_image, extract_tags, extract_wikilinks, TfIdfModel};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Serialize)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub degree: u32,
    pub recency: f64,
    pub tag_count: u32,
    pub image: String,
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
}

pub fn build(notes: &[Note], cfg: &GraphConfig, manual: &[ManualEdge]) -> GraphPayload {
    let title_to_id: HashMap<String, String> = notes
        .iter()
        .map(|n| (n.display_title().to_lowercase(), n.id.clone()))
        .collect();

    let id_set: std::collections::HashSet<&str> = notes.iter().map(|n| n.id.as_str()).collect();

    let tags_per_note: HashMap<String, Vec<String>> = notes
        .iter()
        .map(|n| (n.id.clone(), extract_tags(&n.body)))
        .collect();

    let mut edges: Vec<GraphEdge> = Vec::new();
    let mut idx: u64 = 0;
    let mut stats = GraphStats {
        wikilinks: 0,
        tags: 0,
        similarity: 0,
        manual: 0,
    };

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

    if cfg.include_similarity && notes.len() >= 2 {
        let model = TfIdfModel::build(notes);
        let vectors: Vec<HashMap<String, f64>> = notes
            .iter()
            .map(|n| model.vector(&n.title, &n.body))
            .collect();

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
            let tag_count = tags_per_note
                .get(&n.id)
                .map(|t| t.len() as u32)
                .unwrap_or(0);
            let image = if cfg.show_thumbnails {
                extract_first_image(&n.body).unwrap_or_default()
            } else {
                String::new()
            };
            GraphNode {
                id: n.id.clone(),
                label: n.display_title(),
                degree: *degree.get(&n.id).unwrap_or(&0),
                recency,
                tag_count,
                image,
            }
        })
        .collect();

    GraphPayload {
        nodes_json: serde_json::to_string(&nodes).unwrap_or_else(|_| "[]".into()),
        edges_json: serde_json::to_string(&edges).unwrap_or_else(|_| "[]".into()),
        stats,
    }
}
