//! 评论附图。上传只做一次校验并落盘（内容寻址，见 `blob`），返回可以在 `body_md`
//! 里引用的占位符；评论提交时把占位符换成真实 URL 并绑定到评论。
//!
//! 只收位图（PNG/JPEG/WebP/GIF），按文件头判定，不信扩展名和 `Content-Type`。

use uuid::Uuid;

use crate::error::{Code, Problem, Result};

pub const MAX_FILE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_PER_COMMENT: usize = 4;
/// 占位符：`![alt](ic-attachment:<attachment id>)`，评论提交时展开成 `/media/<hash>`。
pub const SCHEME: &str = "ic-attachment:";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Png,
    Jpeg,
    WebP,
    Gif,
}

impl Kind {
    /// 落盘用的扩展名，也是 CAS 键的后缀。
    pub fn ext(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::WebP => "webp",
            Self::Gif => "gif",
        }
    }
}

/// 按魔数认类型，认不出就是 None。
pub fn sniff(bytes: &[u8]) -> Option<Kind> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        return Some(Kind::Png);
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some(Kind::Jpeg);
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some(Kind::Gif);
    }
    if bytes.len() > 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some(Kind::WebP);
    }
    None
}

/// 只读文件头就够判断类型，不必整块读进内存。
pub async fn sniff_reader<R: tokio::io::AsyncRead + Unpin>(reader: &mut R) -> Result<Kind> {
    use tokio::io::AsyncReadExt;
    let mut head = [0u8; 12];
    let n = reader
        .read(&mut head)
        .await
        .map_err(|e| Problem::code(Code::Validation, format!("unreadable upload: {e}")))?;
    sniff(&head[..n]).ok_or_else(|| Problem::code(Code::Validation, "only PNG, JPEG, WebP or GIF"))
}

/// 读尺寸；认不出结构就返回 None（不阻塞上传，只是没有宽高）。
pub fn dimensions(kind: Kind, bytes: &[u8]) -> Option<(i32, i32)> {
    match kind {
        Kind::Png => {
            // IHDR 紧跟 8 字节签名 + 4 字节长度 + 4 字节类型
            let chunk = bytes.get(16..24)?;
            if &bytes[12..16] != b"IHDR" {
                return None;
            }
            Some((
                i32::from_be_bytes(chunk[..4].try_into().ok()?),
                i32::from_be_bytes(chunk[4..].try_into().ok()?),
            ))
        }
        Kind::Gif => Some((
            i32::from_le_bytes(bytes.get(6..8)?.try_into().ok()?),
            i32::from_le_bytes(bytes.get(8..10)?.try_into().ok()?),
        )),
        Kind::WebP => webp_dimensions(bytes),
        Kind::Jpeg => jpeg_dimensions(bytes),
    }
}

fn webp_dimensions(bytes: &[u8]) -> Option<(i32, i32)> {
    let chunk = bytes.get(12..16)?;
    match chunk {
        b"VP8 " => Some((
            i32::from_le_bytes(bytes.get(26..28)?.try_into().ok()?) & 0x3fff,
            i32::from_le_bytes(bytes.get(28..30)?.try_into().ok()?) & 0x3fff,
        )),
        b"VP8L" => {
            let bits = u32::from_le_bytes(bytes.get(21..25)?.try_into().ok()?);
            Some((
                (bits & 0x3fff) as i32 + 1,
                ((bits >> 14) & 0x3fff) as i32 + 1,
            ))
        }
        b"VP8X" => Some((
            u32::from_le_bytes([
                bytes.get(24).copied()?,
                bytes.get(25).copied()?,
                bytes.get(26).copied()?,
                bytes.get(27).copied()?,
            ]) as i32
                + 1,
            u32::from_le_bytes([
                bytes.get(28).copied()?,
                bytes.get(29).copied()?,
                bytes.get(30).copied()?,
                bytes.get(31).copied()?,
            ]) as i32
                + 1,
        )),
        _ => None,
    }
}

/// 走 SOS 之前的段找 SOF；遇到别的段就按长度跳过。
fn jpeg_dimensions(bytes: &[u8]) -> Option<(i32, i32)> {
    let mut i = 2usize;
    while i + 9 < bytes.len() {
        if bytes[i] != 0xff {
            i += 1;
            continue;
        }
        let marker = bytes[i + 1];
        if (0xc0..=0xc3).contains(&marker) || (0xc5..=0xc7).contains(&marker) {
            let h = u16::from_be_bytes(bytes.get(i + 5..i + 7)?.try_into().ok()?);
            let w = u16::from_be_bytes(bytes.get(i + 7..i + 9)?.try_into().ok()?);
            return Some((i32::from(w), i32::from(h)));
        }
        let len = usize::from(u16::from_be_bytes(
            bytes.get(i + 2..i + 4)?.try_into().ok()?,
        ));
        i += 2 + len.max(2);
    }
    None
}

/// 按扩展名给类型，传 `png` 或 `x.png` 都行。
pub fn content_type(file: &str) -> &'static str {
    match file.rsplit('.').next() {
        Some("png") => "image/png",
        Some("jpg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        _ => "application/octet-stream",
    }
}

/// 把 `ic-attachment:<id>` 换成真实地址；引用了但没绑定的原样保留。
pub fn expand(body_md: &str, links: &std::collections::HashMap<Uuid, String>) -> String {
    if links.is_empty() {
        return body_md.to_owned();
    }
    let mut out = String::with_capacity(body_md.len());
    let mut rest = body_md;
    while let Some(pos) = rest.find(SCHEME) {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + SCHEME.len()..];
        let id_len = after
            .find(|c: char| !c.is_ascii_hexdigit() && c != '-')
            .unwrap_or(after.len());
        match Uuid::try_parse(&after[..id_len])
            .ok()
            .and_then(|id| links.get(&id))
        {
            Some(url) => out.push_str(url),
            None => {
                out.push_str(SCHEME);
                out.push_str(&after[..id_len]);
            }
        }
        rest = &after[id_len..];
    }
    out.push_str(rest);
    out
}

/// 从 `body_md` 里找出引用到的附件 id（保序、去重、最多 MAX_PER_COMMENT）。
pub fn referenced(body_md: &str) -> Vec<Uuid> {
    let mut out: Vec<Uuid> = Vec::new();
    let mut rest = body_md;
    while let Some(pos) = rest.find(SCHEME) {
        let after = &rest[pos + SCHEME.len()..];
        let id_len = after
            .find(|c: char| !c.is_ascii_hexdigit() && c != '-')
            .unwrap_or(after.len());
        if let Ok(id) = Uuid::try_parse(&after[..id_len])
            && !out.contains(&id)
        {
            out.push(id);
        }
        rest = &after[id_len..];
    }
    out.truncate(MAX_PER_COMMENT);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_by_magic_only() {
        assert_eq!(
            sniff(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]),
            Some(Kind::Png)
        );
        assert_eq!(sniff(&[0xff, 0xd8, 0xff, 0xe0]), Some(Kind::Jpeg));
        assert_eq!(sniff(b"GIF89a...."), Some(Kind::Gif));
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some(Kind::WebP));
        assert_eq!(sniff(b"<svg xmlns="), None);
        assert_eq!(sniff(b""), None);
    }

    #[test]
    fn png_size_from_ihdr() {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13];
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&640u32.to_be_bytes());
        bytes.extend_from_slice(&480u32.to_be_bytes());
        assert_eq!(dimensions(Kind::Png, &bytes), Some((640, 480)));
    }

    #[test]
    fn expand_swaps_only_bound_ids() {
        let mut links = std::collections::HashMap::new();
        let id = Uuid::now_v7();
        links.insert(id, format!("/media/{id}"));
        let body = format!(
            "a ![](ic-attachment:{id}) b ![](ic-attachment:{})",
            Uuid::nil()
        );
        let out = expand(&body, &links);
        assert!(out.contains(&format!("/media/{id}")));
        assert!(out.contains("ic-attachment:00000000-0000-0000-0000-000000000000"));
    }

    #[test]
    fn referenced_dedups_and_caps() {
        let ids: Vec<Uuid> = (0..6).map(|_| Uuid::now_v7()).collect();
        let body = ids
            .iter()
            .map(|id| format!("![](ic-attachment:{id})"))
            .chain([format!("![](ic-attachment:{})", ids[0])])
            .collect::<Vec<_>>()
            .join(" ");
        let found = referenced(&body);
        assert_eq!(found.len(), MAX_PER_COMMENT);
        assert_eq!(found[0], ids[0]);
    }
}
