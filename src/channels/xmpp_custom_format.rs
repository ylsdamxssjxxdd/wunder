use crate::channels::xmpp_tls_connector::XmppTlsServerConfig;
use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use pulldown_cmark::{html, Options, Parser};
use scraper::{Html, Selector};
use serde_json::{json, Value};
use tokio_xmpp::minidom::Element;
use tokio_xmpp::parsers::jid::Jid;
use tokio_xmpp::parsers::ns;
use tokio_xmpp::AsyncClient;
use uuid::Uuid;

/// Namespace used by the custom internal client extension payload.
const NS_JJUX: &str = "http://jvsoftware.com/extend";

#[derive(Debug, Clone)]
pub struct CustomXmppMessage {
    pub text: String,
    pub from: String,
    pub mid: String,
    pub file_transfer: Option<Value>,
}

pub async fn send_custom_format_message(
    client: &mut AsyncClient<XmppTlsServerConfig>,
    to_jid: &Jid,
    text: &str,
    message_type: &str,
) -> Result<()> {
    let mid = format!("{{{}}}", Uuid::new_v4());
    let html_content = build_html_content(text, &mid);
    let encoded_content = triple_base64_encode(&html_content);
    let msg_type = if message_type == "group" {
        "groupchat"
    } else {
        "chat"
    };

    let message = Element::builder("message", ns::DEFAULT_NS)
        .attr("type", msg_type)
        .attr("to", to_jid.to_string())
        .attr("mid", &mid)
        .attr("codec", "base64")
        .append(
            Element::builder("body", ns::DEFAULT_NS)
                .append(encoded_content)
                .build(),
        )
        .append(
            Element::builder("eztalk", NS_JJUX)
                .attr("localpath", "extend")
                .attr("sendtime", chrono::Utc::now().to_rfc3339())
                .build(),
        )
        .build();

    client
        .send_stanza(message)
        .await
        .map_err(|err| anyhow!("xmpp send custom format message failed: {err}"))
}

pub fn try_parse_custom_format_message(stanza: &Element) -> Option<CustomXmppMessage> {
    if !is_custom_format_message(stanza) {
        return None;
    }

    let mid = stanza.attr("mid")?.to_string();
    let body_elem = stanza.get_child("body", ns::DEFAULT_NS)?;
    let encoded_body = body_elem.text();
    let html_content = triple_base64_decode(&encoded_body)?;
    let (text, extracted_mid) = extract_from_html(&html_content)?;
    if extracted_mid != mid {
        return None;
    }
    let from = stanza.attr("from")?.to_string();

    let (display_text, file_transfer) = parse_file_transfer(&text)
        .map(|(display, metadata)| (display, Some(metadata)))
        .unwrap_or((text, None));

    Some(CustomXmppMessage {
        text: display_text,
        from,
        mid,
        file_transfer,
    })
}

fn is_custom_format_message(stanza: &Element) -> bool {
    if stanza.name() != "message" {
        return false;
    }
    if stanza.attr("mid").is_none() || stanza.attr("codec").is_none() {
        return false;
    }
    !is_server_receipt(stanza)
}

fn is_server_receipt(stanza: &Element) -> bool {
    if stanza.attr("mid").is_some_and(|mid| mid.contains("###")) {
        return true;
    }
    stanza.get_child("x", "org.spsoft:ext:reply").is_some()
}

fn parse_file_transfer(text: &str) -> Option<(String, Value)> {
    let content = if let Some(rest) = text.strip_prefix("file?@?") {
        rest
    } else if let Some(rest) = text.strip_prefix("file@") {
        rest
    } else {
        return None;
    };

    let parts: Vec<&str> = content.split('|').collect();
    if parts.len() != 5 {
        return None;
    }

    let filename = parts[1].trim();
    let display_text = if filename.is_empty() {
        "User initiated file transfer".to_string()
    } else {
        format!("User initiated file transfer: {filename}")
    };

    let file_info = json!({
        "file_id": parts[0],
        "filename": parts[1],
        "size": parts[2].parse::<u64>().unwrap_or(0),
        "mime_type": parts[3],
        "download_url": parts[4],
    });
    Some((display_text, file_info))
}

fn triple_base64_encode(content: &str) -> String {
    let first = BASE64.encode(content);
    let second = BASE64.encode(&first);
    BASE64.encode(&second)
}

fn triple_base64_decode(encoded: &str) -> Option<String> {
    let first = BASE64.decode(encoded).ok()?;
    let second = BASE64.decode(first).ok()?;
    String::from_utf8(BASE64.decode(second).ok()?).ok()
}

fn markdown_to_html(markdown: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(markdown, options);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);
    html_output.trim_end().to_string()
}

fn build_html_content(text: &str, mid: &str) -> String {
    let html_body = markdown_to_html(text);
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <meta name="qrichtext" content="1" />
</head>
<body style="font-family:sans-serif; font-size:14px; line-height:1.6;">
    {}
</body>
</html>
[mid]{}"#,
        html_body, mid
    )
}

fn extract_from_html(html: &str) -> Option<(String, String)> {
    let marker_start = html.rfind("[mid]{")?;
    let content_start = marker_start + 6;
    let relative_end = html[content_start..].find('}')?;
    let content_end = content_start + relative_end;
    if content_start >= content_end {
        return None;
    }

    // Keep braces because stanza `mid` contains the full `{uuid}` token.
    let mid = html[marker_start + 5..=content_end].to_string();
    let html_content = &html[..marker_start];
    let text = extract_text_with_format(html_content);
    Some((text, mid))
}

fn extract_text_with_format(html: &str) -> String {
    fn extract_element_text(element: scraper::ElementRef) -> String {
        let tag_name = element.value().name();
        let children_text: String = element
            .children()
            .filter_map(|child| {
                if let Some(child_elem) = scraper::ElementRef::wrap(child) {
                    Some(extract_element_text(child_elem))
                } else {
                    child.value().as_text().map(|text| text.text.to_string())
                }
            })
            .collect();

        match tag_name {
            "p" | "div" | "br" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "li" | "tr" | "pre" => {
                if children_text.trim().is_empty() {
                    String::new()
                } else {
                    children_text.trim().to_string() + "\n"
                }
            }
            _ => children_text,
        }
    }

    let document = Html::parse_document(html);
    let body_selector = Selector::parse("body").expect("body selector should be valid");
    let result = if let Some(body) = document.select(&body_selector).next() {
        extract_element_text(body)
    } else {
        extract_element_text(document.root_element())
    };

    result
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_message_can_round_trip() {
        let original = "hello **world**";
        let mid = "{550e8400-e29b-41d4-a716-446655440000}";
        let html = build_html_content(original, mid);
        let encoded = triple_base64_encode(&html);
        let stanza = Element::builder("message", ns::DEFAULT_NS)
            .attr("type", "chat")
            .attr("from", "alice@example.com/mobile")
            .attr("to", "bot@example.com/wunder")
            .attr("mid", mid)
            .attr("codec", "base64")
            .append(
                Element::builder("body", ns::DEFAULT_NS)
                    .append(encoded)
                    .build(),
            )
            .build();
        let parsed = try_parse_custom_format_message(&stanza).expect("custom format parse");
        assert_eq!(parsed.text, "hello world");
        assert_eq!(parsed.from, "alice@example.com/mobile");
        assert_eq!(parsed.mid, mid);
    }

    #[test]
    fn server_receipt_is_filtered_out() {
        let receipt = Element::builder("message", ns::DEFAULT_NS)
            .attr("type", "chat")
            .attr("from", "xmpp-server.com")
            .attr("to", "bot@example.com")
            .attr(
                "mid",
                "{550e8400-e29b-41d4-a716-446655440000}###user@example.com",
            )
            .attr("codec", "base64")
            .append(
                Element::builder("body", ns::DEFAULT_NS)
                    .append("content")
                    .build(),
            )
            .build();
        assert!(try_parse_custom_format_message(&receipt).is_none());
    }
}
