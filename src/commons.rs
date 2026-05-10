#[derive(Clone, Debug, PartialEq)]
pub struct ImageHit {
    pub title: String,
    pub thumb_url: String,
    pub full_url: String,
}

pub async fn search(query: &str) -> Result<Vec<ImageHit>, String> {
    let encoded = js_sys::encode_uri_component(query)
        .as_string()
        .ok_or_else(|| "encoding failed".to_string())?;
    let url = format!(
        "https://commons.wikimedia.org/w/api.php?action=query&format=json&origin=*\
         &generator=search&gsrsearch={}&gsrnamespace=6&gsrlimit=24\
         &prop=imageinfo&iiprop=url%7Csize%7Cmime&iiurlwidth=240",
        encoded
    );
    let resp = gloo_net::http::Request::get(&url)
        .send()
        .await
        .map_err(|e| format!("network error: {}", e))?;
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("bad json: {}", e))?;

    let pages = json
        .get("query")
        .and_then(|q| q.get("pages"))
        .and_then(|p| p.as_object());
    let Some(pages) = pages else { return Ok(Vec::new()) };

    let mut items = Vec::new();
    for (_, page) in pages {
        let title = page
            .get("title")
            .and_then(|t| t.as_str())
            .and_then(|s| s.strip_prefix("File:"))
            .unwrap_or("")
            .to_string();
        let info = page.get("imageinfo").and_then(|a| a.get(0));
        let Some(info) = info else { continue };
        let mime = info.get("mime").and_then(|m| m.as_str()).unwrap_or("");
        if !mime.starts_with("image/") || mime == "image/svg+xml" {
            continue;
        }
        let thumb_url = info
            .get("thumburl")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
        let full_url = info
            .get("url")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
        if thumb_url.is_empty() || full_url.is_empty() || title.is_empty() {
            continue;
        }
        items.push(ImageHit {
            title,
            thumb_url,
            full_url,
        });
    }

    Ok(items)
}
