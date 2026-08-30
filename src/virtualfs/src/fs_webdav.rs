use tokio::sync::mpsc as tokio_mpsc;

fn get_webdav_client() -> reqwest::Client {
    static CLIENT: std::sync::RwLock<Option<(u64, reqwest::Client)>> =
        std::sync::RwLock::new(None);

    let want = common::CONNECT_TIMEOUT_SECS.load(std::sync::atomic::Ordering::Relaxed);
    if let Ok(guard) = CLIENT.read() {
        if let Some((built_with, client)) = guard.as_ref() {
            if *built_with == want {
                return client.clone();
            }
        }
    }

    let mut builder = reqwest::Client::builder().pool_max_idle_per_host(10);
    if let Some(d) = common::connect_timeout() {
        builder = builder.connect_timeout(d);
    }
    let client = builder.build().unwrap_or_else(|_| reqwest::Client::new());
    if let Ok(mut guard) = CLIENT.write() {
        *guard = Some((want, client.clone()));
    }
    client
}

#[derive(serde::Deserialize, serde::Serialize, Clone, Debug)]
pub struct WebDAVConfig {
    pub url: String,
    pub user: Option<String>,
    pub pass: Option<String>,
    pub base_path: Option<String>,
}

fn percent_decode(s: &str) -> String {
    let mut bytes = Vec::new();
    let mut chars = s.as_bytes().iter();
    while let Some(&c) = chars.next() {
        if c == b'%' {
            if let (Some(&h1), Some(&h2)) = (chars.next(), chars.next()) {
                let hex = vec![h1, h2];
                if let Ok(hex_str) = String::from_utf8(hex) {
                    if let Ok(b) = u8::from_str_radix(&hex_str, 16) {
                        bytes.push(b);
                        continue;
                    }
                }
            }
        }
        bytes.push(c);
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn extract_tag_content(segment: &str, tag: &str) -> Option<String> {
    let start_tag = format!("<{}", tag);
    let end_tag = format!("</{}>", tag);

    if let Some(start_idx) = segment.find(&start_tag) {
        if let Some(close_bracket_offset) = segment[start_idx..].find('>') {
            let start_content = start_idx + close_bracket_offset + 1;
            if let Some(end_idx_offset) = segment[start_content..].find(&end_tag) {
                let end_content = start_content + end_idx_offset;
                return Some(segment[start_content..end_content].trim().to_string());
            }
        }
    }
    None
}

pub async fn list_webdav_directory(
    cfg: &WebDAVConfig,
    req_path: &str,
) -> Result<(Vec<(String, String)>, Vec<(String, u64, String)>), String> {
    let mut url = cfg.url.trim_end_matches('/').to_string();
    if !req_path.is_empty() && req_path != "/" {
        let path_clean = req_path.trim_start_matches('/');
        if !path_clean.is_empty() {
            url.push('/');
            url.push_str(path_clean);
        }
    }

    let client = get_webdav_client();
    let mut req = client
        .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), &url)
        .header("Depth", "1")
        .header("Content-Type", "application/xml");

    if let (Some(u), Some(p)) = (&cfg.user, &cfg.pass) {
        req = req.basic_auth(u, Some(p));
    }
    if let Some(d) = common::request_timeout() {
        req = req.timeout(d);
    }

    let res = req
        .send()
        .await
        .map_err(|e| format!("HTTP PROPFIND failed: {}", e))?;
    let status = res.status();
    if !status.is_success() {
        return Err(format!("WebDAV returned error status: {}", status));
    }

    let xml = res
        .text()
        .await
        .map_err(|e| format!("Failed to read XML body: {}", e))?;

    let xml_normalized = xml
        .replace("<D:", "<")
        .replace("<d:", "<")
        .replace("</D:", "</")
        .replace("</d:", "</");

    let mut directories = Vec::new();
    let mut files = Vec::new();

    let segments: Vec<&str> = xml_normalized.split("</response>").collect();
    for segment in segments {
        if !segment.contains("<href") {
            continue;
        }

        let href = extract_tag_content(segment, "href").unwrap_or_default();

        if href.is_empty() {
            continue;
        }

        let decoded_path = percent_decode(&href);
        let clean_path = decoded_path.trim_end_matches('/');
        let name = clean_path.split('/').last().unwrap_or_default().to_string();
        if name.is_empty() {
            continue;
        }

        let url_parsed = reqwest::Url::parse(&url).ok();
        let base_path_decoded = if let Some(u) = url_parsed {
            percent_decode(u.path())
        } else {
            percent_decode(&url)
        };

        if clean_path == base_path_decoded.trim_end_matches('/') {
            continue;
        }

        let is_dir = segment.contains("<collection>")
            || segment.contains("<collection/>")
            || segment.contains("<collection ");

        let last_modified_raw = extract_tag_content(segment, "getlastmodified").unwrap_or_default();
        let date_str = if last_modified_raw.is_empty() {
            "".to_string()
        } else {
            chrono::DateTime::parse_from_rfc2822(&last_modified_raw)
                .map(|dt| {
                    let local_dt: chrono::DateTime<chrono::Local> = dt.into();
                    local_dt.format("%Y-%m-%d %H:%M:%S").to_string()
                })
                .unwrap_or(last_modified_raw)
        };

        if is_dir {
            directories.push((name, date_str));
        } else {
            let size_str = extract_tag_content(segment, "getcontentlength").unwrap_or_default();
            let size = size_str.parse::<u64>().unwrap_or(0);
            files.push((name, size, date_str));
        }
    }

    Ok((directories, files))
}

pub async fn start_webdav_download(
    cfg: WebDAVConfig,
    req_path: String,
    tx: tokio_mpsc::Sender<Result<Vec<u8>, String>>,
) -> Result<u64, String> {
    let mut url = cfg.url.trim_end_matches('/').to_string();
    if !req_path.is_empty() && req_path != "/" {
        let path_clean = req_path.trim_start_matches('/');
        if !path_clean.is_empty() {
            url.push('/');
            url.push_str(path_clean);
        }
    }

    let client = get_webdav_client();
    let mut req = client.get(&url);

    if let (Some(u), Some(p)) = (&cfg.user, &cfg.pass) {
        req = req.basic_auth(u, Some(p));
    }

    let res = req
        .send()
        .await
        .map_err(|e| format!("HTTP GET failed: {}", e))?;
    let status = res.status();
    if !status.is_success() {
        return Err(format!("WebDAV GET returned error status: {}", status));
    }

    let total_size = res.content_length().unwrap_or(0);

    tokio::spawn(async move {
        let mut response = res;
        loop {
            match response.chunk().await {
                Ok(Some(bytes)) => {
                    if tx.send(Ok(bytes.to_vec())).await.is_err() {
                        break;
                    }
                }
                Ok(None) => {
                    break;
                }
                Err(e) => {
                    let _ = tx.send(Err(e.to_string())).await;
                    break;
                }
            }
        }
        let _ = tx.send(Ok(Vec::new())).await;
    });

    Ok(total_size)
}

pub async fn create_webdav_directory(cfg: &WebDAVConfig, path: &str) -> Result<(), String> {
    let mut url = cfg.url.trim_end_matches('/').to_string();
    let path_clean = path.trim_start_matches('/');
    if !path_clean.is_empty() {
        url.push('/');
        url.push_str(path_clean);
    }

    let client = get_webdav_client();
    let mut req = client.request(reqwest::Method::from_bytes(b"MKCOL").unwrap(), &url);

    if let (Some(u), Some(p)) = (&cfg.user, &cfg.pass) {
        req = req.basic_auth(u, Some(p));
    }

    let res = req
        .send()
        .await
        .map_err(|e| format!("HTTP MKCOL failed: {}", e))?;
    let status = res.status();
    if status.is_success() || status == reqwest::StatusCode::CREATED {
        Ok(())
    } else {
        Err(format!("WebDAV returned error status: {}", status))
    }
}

pub async fn delete_webdav_entry(cfg: &WebDAVConfig, path: &str) -> Result<(), String> {
    let mut url = cfg.url.trim_end_matches('/').to_string();
    let path_clean = path.trim_start_matches('/');
    if !path_clean.is_empty() {
        url.push('/');
        url.push_str(path_clean);
    }

    let client = get_webdav_client();
    let mut req = client.delete(&url);

    if let (Some(u), Some(p)) = (&cfg.user, &cfg.pass) {
        req = req.basic_auth(u, Some(p));
    }

    let res = req
        .send()
        .await
        .map_err(|e| format!("HTTP DELETE failed: {}", e))?;
    let status = res.status();
    if status.is_success() || status == reqwest::StatusCode::NO_CONTENT {
        Ok(())
    } else {
        Err(format!("WebDAV returned error status: {}", status))
    }
}

pub async fn rename_webdav_entry(
    cfg: &WebDAVConfig,
    source_path: &str,
    dest_path: &str,
) -> Result<(), String> {
    let mut url = cfg.url.trim_end_matches('/').to_string();
    let source_clean = source_path.trim_start_matches('/');
    if !source_clean.is_empty() {
        url.push('/');
        url.push_str(source_clean);
    }

    let mut dest_url = cfg.url.trim_end_matches('/').to_string();
    let dest_clean = dest_path.trim_start_matches('/');
    if !dest_clean.is_empty() {
        dest_url.push('/');
        dest_url.push_str(dest_clean);
    }

    let client = get_webdav_client();
    let mut req = client
        .request(reqwest::Method::from_bytes(b"MOVE").unwrap(), &url)
        .header("Destination", dest_url);

    if let (Some(u), Some(p)) = (&cfg.user, &cfg.pass) {
        req = req.basic_auth(u, Some(p));
    }

    let res = req
        .send()
        .await
        .map_err(|e| format!("HTTP MOVE failed: {}", e))?;
    let status = res.status();
    if status.is_success()
        || status == reqwest::StatusCode::CREATED
        || status == reqwest::StatusCode::NO_CONTENT
    {
        Ok(())
    } else {
        Err(format!("WebDAV returned error status: {}", status))
    }
}

pub async fn upload_webdav_file(
    cfg: &WebDAVConfig,
    path: &str,
    content: Vec<u8>,
) -> Result<(), String> {
    let mut url = cfg.url.trim_end_matches('/').to_string();
    let path_clean = path.trim_start_matches('/');
    if !path_clean.is_empty() {
        url.push('/');
        url.push_str(path_clean);
    }

    let client = get_webdav_client();
    let mut req = client.put(&url).body(content);

    if let (Some(u), Some(p)) = (&cfg.user, &cfg.pass) {
        req = req.basic_auth(u, Some(p));
    }

    let res = req
        .send()
        .await
        .map_err(|e| format!("HTTP PUT failed: {}", e))?;
    let status = res.status();
    if status.is_success()
        || status == reqwest::StatusCode::CREATED
        || status == reqwest::StatusCode::NO_CONTENT
    {
        Ok(())
    } else {
        Err(format!("WebDAV returned error status: {}", status))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_space() {
        assert_eq!(percent_decode("%20"), " ");
    }

    #[test]
    fn decode_slash() {
        assert_eq!(percent_decode("%2F"), "/");
    }

    #[test]
    fn decode_mixed_plain_and_encoded() {
        assert_eq!(percent_decode("/remote.php/dav/files/user%40host/docs"), "/remote.php/dav/files/user@host/docs");
    }

    #[test]
    fn decode_cyrillic_utf8() {
        assert_eq!(percent_decode("%D0%B4"), "д");
    }

    #[test]
    fn decode_empty_string() {
        assert_eq!(percent_decode(""), "");
    }

    #[test]
    fn decode_no_encoding_needed() {
        assert_eq!(percent_decode("/simple/path/file.txt"), "/simple/path/file.txt");
    }

    #[test]
    fn decode_incomplete_percent_sequence_passes_through() {
        let result = percent_decode("/path%");
        assert!(result.contains("/path"));
    }

    #[test]
    fn decode_multiple_encoded_chars() {
        assert_eq!(percent_decode("hello%20world%21"), "hello world!");
    }

    #[test]
    fn extract_simple_tag() {
        let xml = "<href>/path/to/file</href>";
        assert_eq!(extract_tag_content(xml, "href"), Some("/path/to/file".into()));
    }

    #[test]
    fn extract_tag_with_whitespace() {
        let xml = "<getcontentlength>  1234  </getcontentlength>";
        assert_eq!(extract_tag_content(xml, "getcontentlength"), Some("1234".into()));
    }

    #[test]
    fn extract_tag_with_attributes() {
        let xml = r#"<response xmlns="DAV:"><href>/file</href></response>"#;
        assert_eq!(extract_tag_content(xml, "href"), Some("/file".into()));
    }

    #[test]
    fn extract_missing_tag_returns_none() {
        let xml = "<href>/path</href>";
        assert_eq!(extract_tag_content(xml, "getcontentlength"), None);
    }

    #[test]
    fn extract_empty_tag_content() {
        let xml = "<prop></prop>";
        assert_eq!(extract_tag_content(xml, "prop"), Some("".into()));
    }

    #[test]
    fn extract_from_larger_xml_segment() {
        let xml = r#"
            <response>
                <href>/remote.php/dav/files/user/docs/</href>
                <propstat>
                    <prop>
                        <getcontentlength>42</getcontentlength>
                        <getlastmodified>Mon, 01 Jan 2024 00:00:00 GMT</getlastmodified>
                    </prop>
                </propstat>
            </response>"#;
        assert_eq!(extract_tag_content(xml, "getcontentlength"), Some("42".into()));
        assert!(extract_tag_content(xml, "getlastmodified").unwrap().contains("2024"));
    }

    #[test]
    fn extract_returns_first_match() {
        let xml = "<name>first</name> <name>second</name>";
        assert_eq!(extract_tag_content(xml, "name"), Some("first".into()));
    }

    #[test]
    fn decode_accepts_either_hex_case() {
        assert_eq!(percent_decode("%2f"), "/");
        assert_eq!(percent_decode("%2F"), "/");
        assert_eq!(percent_decode("%d0%b4"), percent_decode("%D0%B4"));
    }

    #[test]
    fn decode_reassembles_multi_byte_utf8() {
        assert_eq!(percent_decode("%F0%9F%93%81"), "\u{1f4c1}");
        assert_eq!(percent_decode("%E2%82%AC"), "\u{20ac}");
    }

    #[test]
    fn decode_leaves_plus_signs_alone() {
        assert_eq!(percent_decode("/a+b/c+d.txt"), "/a+b/c+d.txt");
    }

    #[test]
    fn decode_keeps_encoded_percent_signs() {
        assert_eq!(percent_decode("/100%25.txt"), "/100%.txt");
    }

    #[test]
    fn decode_replaces_invalid_utf8_bytes() {
        assert_eq!(percent_decode("%FF"), "\u{fffd}");
    }

    #[test]
    fn decode_is_idempotent_on_plain_text() {
        let once = percent_decode("/dav/files/alice/notes.txt");
        assert_eq!(percent_decode(&once), once);
    }

    #[test]
    fn decode_preserves_encoded_path_separators_as_separators() {
        assert_eq!(percent_decode("/a%2Fb"), "/a/b");
    }

    #[test]
    fn extract_self_closing_tag_returns_none() {
        assert_eq!(extract_tag_content("<resourcetype><collection/></resourcetype>", "collection"), None);
    }

    #[test]
    fn extract_handles_content_spread_over_lines() {
        let xml = "<href>\n            /dav/files/alice/\n        </href>";
        assert_eq!(extract_tag_content(xml, "href"), Some("/dav/files/alice/".into()));
    }

    #[test]
    fn extract_unclosed_tag_returns_none() {
        assert_eq!(extract_tag_content("<href>/dav/files", "href"), None);
    }

    #[test]
    fn extract_ignores_a_bare_closing_tag() {
        assert_eq!(extract_tag_content("</href>", "href"), None);
    }

    #[test]
    fn extract_reads_zero_length_content_as_empty_not_missing() {
        assert_eq!(extract_tag_content("<getcontentlength></getcontentlength>", "getcontentlength"), Some("".into()));
        assert_eq!(extract_tag_content("<getcontentlength>0</getcontentlength>", "getcontentlength"), Some("0".into()));
    }
}

pub async fn upload_webdav_file_with_progress(
    cfg: &WebDAVConfig,
    path: &str,
    content: Vec<u8>,
    tx: tokio_mpsc::Sender<u64>,
) -> Result<(), String> {
    let mut url = cfg.url.trim_end_matches('/').to_string();
    let path_clean = path.trim_start_matches('/');
    if !path_clean.is_empty() {
        url.push('/');
        url.push_str(path_clean);
    }

    let client = get_webdav_client();

    let chunk_size = 16 * 1024;
    let stream = futures_util::stream::unfold(
        (content, 0),
        move |(content, offset)| {
            let tx = tx.clone();
            async move {
                if offset >= content.len() {
                    None
                } else {
                    let end = std::cmp::min(offset + chunk_size, content.len());
                    let chunk = content[offset..end].to_vec();
                    let next_offset = end;
                    let _ = tx.send(next_offset as u64).await;
                    Some((Ok::<_, reqwest::Error>(bytes::Bytes::from(chunk)), (content, next_offset)))
                }
            }
        }
    );

    let body = reqwest::Body::wrap_stream(stream);
    let mut req = client.put(&url).body(body);

    if let (Some(u), Some(p)) = (&cfg.user, &cfg.pass) {
        req = req.basic_auth(u, Some(p));
    }

    let res = req
        .send()
        .await
        .map_err(|e| format!("HTTP PUT failed: {}", e))?;
    let status = res.status();
    if status.is_success()
        || status == reqwest::StatusCode::CREATED
        || status == reqwest::StatusCode::NO_CONTENT
    {
        Ok(())
    } else {
        Err(format!("WebDAV returned error status: {}", status))
    }
}

