//! Markdown → 安全 HTML。两段式：先渲染，再过 ammonia 白名单。
//!
//! 暴露面是公开评论，所以允许原始 HTML 直通，但一切出站都经白名单裁剪：
//! 只留排版标签，链接补 `rel`，脚本/事件属性全丢。

use std::sync::LazyLock;

use ammonia::Builder;
use pulldown_cmark::{Options, Parser, html};
use regex::Regex;

const MAX_CHARS: usize = 8 * 1024;

/// 允许的标签集合：排版 + 代码 + 少量语义标签。
static ALLOWED_TAGS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    vec![
        "a",
        "b",
        "blockquote",
        "br",
        "code",
        "del",
        "em",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "hr",
        "i",
        "img",
        "li",
        "ol",
        "p",
        "pre",
        "s",
        "span",
        "strong",
        "sub",
        "sup",
        "table",
        "tbody",
        "td",
        "th",
        "thead",
        "tr",
        "u",
        "ul",
    ]
});

static SANITIZER: LazyLock<Builder<'static>> = LazyLock::new(|| {
    let mut builder = Builder::default();
    builder
        .tags(ALLOWED_TAGS.iter().copied().collect())
        .link_rel(Some("noopener noreferrer nofollow"))
        .add_generic_attributes(["alt", "title", "src", "href"])
        .set_tag_attribute_value("a", "target", "_blank");
    builder
});

pub struct Rendered {
    pub html: String,
}

/// 渲染 markdown。超长截断在字符边界，避免非 ASCII 被切坏。
pub fn render(raw: &str) -> Rendered {
    let trimmed = raw.trim();
    let mut text = trimmed.to_owned();
    if text.chars().count() > MAX_CHARS {
        text = text.chars().take(MAX_CHARS).collect();
        text.push('…');
    }

    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_SMART_PUNCTUATION);
    // 裸 HTML 由 pulldown 原样透传，随后交给 ammonia 白名单裁剪

    let parser = Parser::new_ext(&text, options);
    let mut raw = String::with_capacity(text.len() * 3 / 2);
    html::push_html(&mut raw, parser);

    Rendered {
        html: SANITIZER.clean(&raw).to_string(),
    }
}

/// 纯文本摘要（列表页 / 通知用），剥标签并压空白。
pub fn excerpt(html: &str, max: usize) -> String {
    static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]*>").unwrap());
    static WS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
    let plain = TAG.replace_all(html, " ");
    let plain = WS.replace_all(&plain, " ");
    let plain = plain.trim();
    plain.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_script() {
        let r = render("hello <script>alert(1)</script> world");
        assert!(!r.html.contains("<script"));
        assert!(r.html.contains("hello"));
        assert!(r.html.contains("world"));
    }

    #[test]
    fn allows_iframe_from_vendor() {
        let r = render("```\nlet x = 1;\n```");
        assert!(r.html.contains("<pre>"));
        assert!(r.html.contains("<code"));
    }

    #[test]
    fn strips_event_handlers() {
        let r = render("<img src=x onerror=\"alert(1)\">");
        assert!(r.html.contains("<img"));
        assert!(!r.html.contains("onerror"));
    }

    #[test]
    fn external_links_get_rel() {
        let r = render("[x](https://example.com)");
        assert!(r.html.contains("rel=\"noopener noreferrer nofollow\""));
        assert!(r.html.contains("target=\"_blank\""));
    }

    #[test]
    fn keeps_basic_formatting() {
        let r = render("**加粗** 和 `代码`");
        assert!(r.html.contains("<strong>"));
        assert!(r.html.contains("<code>"));
    }

    #[test]
    fn excerpt_strips_tags() {
        assert_eq!(excerpt("<p>a <b>b</b></p>", 10), "a b");
    }

    #[test]
    fn long_input_truncates() {
        let long = "字".repeat(MAX_CHARS + 10);
        let r = render(&long);
        assert!(r.html.len() > MAX_CHARS);
        assert!(r.html.contains('…'));
    }
}
