use crate::config::SupabaseConfig;
use gloo_net::http::{Request, RequestBuilder};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Post {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Comment {
    pub id: String,
    pub post_id: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Serialize)]
struct NewPost<'a> {
    title: &'a str,
    body: &'a str,
    tags: Vec<&'a str>,
    author: &'a str,
}

#[derive(Serialize)]
struct NewComment<'a> {
    post_id: &'a str,
    body: &'a str,
    author: &'a str,
}

fn auth(req: RequestBuilder, key: &str) -> RequestBuilder {
    req.header("apikey", key)
        .header("Authorization", &format!("Bearer {}", key))
}

pub async fn fetch_posts(cfg: &SupabaseConfig) -> Result<Vec<Post>, String> {
    let url = cfg.rest_url("posts?select=*&order=created_at.desc&limit=100");
    let resp = auth(Request::get(&url), &cfg.anon_key)
        .send()
        .await
        .map_err(|e| format!("network: {}", e))?;
    if !resp.ok() {
        return Err(format!("supabase {}: {}", resp.status(), resp.status_text()));
    }
    resp.json::<Vec<Post>>()
        .await
        .map_err(|e| format!("parse: {}", e))
}

pub async fn publish_post(
    cfg: &SupabaseConfig,
    title: &str,
    body: &str,
    tags: &[String],
    author: &str,
) -> Result<Post, String> {
    let url = cfg.rest_url("posts");
    let payload = NewPost {
        title,
        body,
        tags: tags.iter().map(String::as_str).collect(),
        author,
    };
    let req = auth(Request::post(&url), &cfg.anon_key)
        .header("Content-Type", "application/json")
        .header("Prefer", "return=representation")
        .json(&payload)
        .map_err(|e| format!("encode: {}", e))?;
    let resp = req.send().await.map_err(|e| format!("network: {}", e))?;
    if !resp.ok() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("supabase {}: {}", status, body));
    }
    let mut posts: Vec<Post> = resp.json().await.map_err(|e| format!("parse: {}", e))?;
    posts.pop().ok_or_else(|| "empty response".to_string())
}

pub async fn fetch_comments(cfg: &SupabaseConfig, post_id: &str) -> Result<Vec<Comment>, String> {
    let url = cfg.rest_url(&format!(
        "comments?select=*&post_id=eq.{}&order=created_at.asc",
        post_id
    ));
    let resp = auth(Request::get(&url), &cfg.anon_key)
        .send()
        .await
        .map_err(|e| format!("network: {}", e))?;
    if !resp.ok() {
        return Err(format!("supabase {}: {}", resp.status(), resp.status_text()));
    }
    resp.json::<Vec<Comment>>()
        .await
        .map_err(|e| format!("parse: {}", e))
}

pub async fn post_comment(
    cfg: &SupabaseConfig,
    post_id: &str,
    body: &str,
    author: &str,
) -> Result<Comment, String> {
    let url = cfg.rest_url("comments");
    let payload = NewComment {
        post_id,
        body,
        author,
    };
    let req = auth(Request::post(&url), &cfg.anon_key)
        .header("Content-Type", "application/json")
        .header("Prefer", "return=representation")
        .json(&payload)
        .map_err(|e| format!("encode: {}", e))?;
    let resp = req.send().await.map_err(|e| format!("network: {}", e))?;
    if !resp.ok() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("supabase {}: {}", status, body));
    }
    let mut comments: Vec<Comment> = resp.json().await.map_err(|e| format!("parse: {}", e))?;
    comments.pop().ok_or_else(|| "empty response".to_string())
}
