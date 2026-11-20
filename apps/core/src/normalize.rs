//! path 归一化：去查询串、去 fragment、去尾斜杠、折叠重复斜杠、空则 `/`。
//!
//! thread 的唯一键建立在归一化结果上，所以这里必须确定、无歧义。

pub const MAX_PATH_LEN: usize = 512;

/// 归一化一个站内路径。返回 `None` 表示超长或不可用。
pub fn normalize_path(raw: &str) -> Option<String> {
    let raw = raw.split(['?', '#']).next().unwrap_or("");
    let raw = raw.trim();

    let mut out = String::with_capacity(raw.len().min(MAX_PATH_LEN));
    let mut last_slash = false;
    for ch in raw.chars() {
        if ch == '/' {
            if last_slash {
                continue;
            }
            last_slash = true;
        } else {
            last_slash = false;
        }
        out.push(ch);
    }

    if !out.starts_with('/') {
        out.insert(0, '/');
    }
    while out.len() > 1 && out.ends_with('/') {
        out.pop();
    }
    // 去掉百分号编码的大写差异带来的重复键
    let out = out.to_lowercase();

    if out.len() > MAX_PATH_LEN {
        return None;
    }
    Some(out)
}

/// 归一化站点 origin：小写、去尾斜杠、强制 https（裸域补 https）。
pub fn normalize_origin(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.len() > 255 {
        return None;
    }
    let lower = raw.to_lowercase();
    let with_scheme = if lower.starts_with("http://") || lower.starts_with("https://") {
        lower
    } else {
        format!("https://{lower}")
    };
    let trimmed = with_scheme.trim_end_matches('/').to_owned();
    let host_part = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))?;
    if host_part.is_empty() || host_part.contains(' ') {
        return None;
    }
    Some(trimmed.replacen("http://", "https://", 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_collapse() {
        assert_eq!(normalize_path("/a/b/").as_deref(), Some("/a/b"));
        assert_eq!(normalize_path("a//b///c").as_deref(), Some("/a/b/c"));
        assert_eq!(normalize_path("/").as_deref(), Some("/"));
        assert_eq!(normalize_path("").as_deref(), Some("/"));
        assert_eq!(normalize_path("/a?x=1#y").as_deref(), Some("/a"));
        assert_eq!(normalize_path("/A/B").as_deref(), Some("/a/b"));
    }

    #[test]
    fn origins_normalize() {
        assert_eq!(
            normalize_origin("Example.COM/").as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            normalize_origin("http://example.com").as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            normalize_origin("https://example.com/blog").as_deref(),
            Some("https://example.com/blog")
        );
        assert!(normalize_origin(" ").is_none());
    }
}
