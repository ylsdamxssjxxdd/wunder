//! XML stream parser for XMPP

use crate::Error;
use bytes::{BufMut, BytesMut};
use log::debug;
use minidom::tree_builder::TreeBuilder;
use minidom::Element;
use rxml::{Parse, RawParser};
use std::collections::HashMap;
use std::fmt::Write;
use std::io;
#[cfg(feature = "syntax-highlighting")]
use std::sync::OnceLock;
use tokio_util::codec::{Decoder, Encoder};

#[cfg(feature = "syntax-highlighting")]
static PS: OnceLock<syntect::parsing::SyntaxSet> = OnceLock::new();
#[cfg(feature = "syntax-highlighting")]
static SYNTAX: OnceLock<syntect::parsing::SyntaxReference> = OnceLock::new();
#[cfg(feature = "syntax-highlighting")]
static THEME: OnceLock<syntect::highlighting::Theme> = OnceLock::new();

#[cfg(feature = "syntax-highlighting")]
fn init_syntect() {
    let ps = syntect::parsing::SyntaxSet::load_defaults_newlines();
    let syntax = ps.find_syntax_by_extension("xml").unwrap();
    let ts = syntect::highlighting::ThemeSet::load_defaults();
    let theme = ts.themes["Solarized (dark)"].clone();

    SYNTAX.set(syntax.clone()).unwrap();
    PS.set(ps).unwrap();
    THEME.set(theme).unwrap();
}

#[cfg(feature = "syntax-highlighting")]
fn highlight_xml(xml: &str) -> String {
    let mut h = syntect::easy::HighlightLines::new(SYNTAX.get().unwrap(), THEME.get().unwrap());
    let ranges: Vec<_> = h.highlight_line(&xml, PS.get().unwrap()).unwrap();
    let escaped = syntect::util::as_24_bit_terminal_escaped(&ranges[..], false);
    format!("{}\x1b[0m", escaped)
}

#[cfg(not(feature = "syntax-highlighting"))]
fn highlight_xml(xml: &str) -> &str {
    xml
}

/// Anything that can be sent or received on an XMPP/XML stream
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Packet {
    /// `<stream:stream>` start tag
    StreamStart(HashMap<String, String>),
    /// A complete stanza or nonza
    Stanza(Element),
    /// Plain text (think whitespace keep-alive)
    Text(String),
    /// `</stream:stream>` closing tag
    StreamEnd,
}

/// Stateful encoder/decoder for a bytestream from/to XMPP `Packet`
pub struct XmppCodec {
    /// Outgoing
    ns: Option<String>,
    /// Incoming
    driver: RawParser,
    stanza_builder: TreeBuilder,
}

impl XmppCodec {
    /// Constructor
    pub fn new() -> Self {
        let stanza_builder = TreeBuilder::new();
        let driver = RawParser::new();
        #[cfg(feature = "syntax-highlighting")]
        if log::log_enabled!(log::Level::Debug) && PS.get().is_none() {
            init_syntect();
        }
        XmppCodec {
            ns: None,
            driver,
            stanza_builder,
        }
    }
}

impl Default for XmppCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder for XmppCodec {
    type Item = Packet;
    type Error = Error;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        loop {
            let token = match self.driver.parse_buf(buf, false) {
                Ok(Some(token)) => token,
                Ok(None) => break,
                Err(rxml::Error::IO(e)) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(minidom::Error::from(e).into()),
            };

            let had_stream_root = self.stanza_builder.depth() > 0;
            self.stanza_builder.process_event(token)?;
            let has_stream_root = self.stanza_builder.depth() > 0;

            if !had_stream_root && has_stream_root {
                let root = self.stanza_builder.top().unwrap();
                let attrs =
                    root.attrs()
                        .map(|(name, value)| (name.to_owned(), value.to_owned()))
                        .chain(root.prefixes.declared_prefixes().iter().map(
                            |(prefix, namespace)| {
                                (
                                    prefix
                                        .as_ref()
                                        .map(|prefix| format!("xmlns:{}", prefix))
                                        .unwrap_or_else(|| "xmlns".to_owned()),
                                    namespace.clone(),
                                )
                            },
                        ))
                        .collect();
                debug!("<< {}", highlight_xml(&String::from(root)));
                return Ok(Some(Packet::StreamStart(attrs)));
            } else if self.stanza_builder.depth() == 1 {
                self.driver.release_temporaries();

                if let Some(stanza) = self.stanza_builder.unshift_child() {
                    debug!("<< {}", highlight_xml(&String::from(&stanza)));
                    return Ok(Some(Packet::Stanza(stanza)));
                }
            } else if let Some(_) = self.stanza_builder.root.take() {
                self.driver.release_temporaries();

                debug!("<< {}", highlight_xml("</stream:stream>"));
                return Ok(Some(Packet::StreamEnd));
            }
        }

        Ok(None)
    }

    fn decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        self.decode(buf)
    }
}

impl Encoder<Packet> for XmppCodec {
    type Error = Error;

    fn encode(&mut self, item: Packet, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let remaining = dst.capacity() - dst.len();
        let max_stanza_size: usize = 2usize.pow(16);
        if remaining < max_stanza_size {
            dst.reserve(max_stanza_size - remaining);
        }

        fn to_io_err<E: Into<Box<dyn std::error::Error + Send + Sync>>>(e: E) -> io::Error {
            io::Error::new(io::ErrorKind::InvalidInput, e)
        }

        match item {
            Packet::StreamStart(start_attrs) => {
                let mut buf = String::new();
                write!(buf, "<stream:stream").map_err(to_io_err)?;
                for (name, value) in start_attrs {
                    write!(buf, " {}=\"{}\"", escape(&name), escape(&value)).map_err(to_io_err)?;
                    if name == "xmlns" {
                        self.ns = Some(value);
                    }
                }
                write!(buf, ">").map_err(to_io_err)?;

                write!(dst, "{}", buf)?;
                let utf8 = std::str::from_utf8(dst)?;
                debug!(">> {}", highlight_xml(utf8))
            }
            Packet::Stanza(stanza) => {
                let _ = stanza
                    .write_to(&mut WriteBytes::new(dst))
                    .map_err(|e| to_io_err(format!("{}", e)))?;
                let utf8 = std::str::from_utf8(dst)?;
                debug!(">> {}", highlight_xml(utf8));
            }
            Packet::Text(text) => {
                let _ = write_text(&text, dst).map_err(to_io_err)?;
                let utf8 = std::str::from_utf8(dst)?;
                debug!(">> {}", highlight_xml(utf8));
            }
            Packet::StreamEnd => {
                let _ = write!(dst, "</stream:stream>\n").map_err(to_io_err);
                debug!(">> {}", highlight_xml("</stream:stream>"));
            }
        }

        Ok(())
    }
}

/// Write XML-escaped text string
pub fn write_text<W: Write>(text: &str, writer: &mut W) -> Result<(), std::fmt::Error> {
    write!(writer, "{}", escape(text))
}

/// Copied from `RustyXML` for now
pub fn escape(input: &str) -> String {
    let mut result = String::with_capacity(input.len());

    for c in input.chars() {
        match c {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            '\'' => result.push_str("&apos;"),
            '"' => result.push_str("&quot;"),
            o => result.push(o),
        }
    }
    result
}

/// BytesMut impl only std::fmt::Write but not std::io::Write. The
/// latter trait is required for minidom's
/// `Element::write_to_inner()`.
struct WriteBytes<'a> {
    dst: &'a mut BytesMut,
}

impl<'a> WriteBytes<'a> {
    fn new(dst: &'a mut BytesMut) -> Self {
        WriteBytes { dst }
    }
}

impl<'a> std::io::Write for WriteBytes<'a> {
    fn write(&mut self, buf: &[u8]) -> std::result::Result<usize, std::io::Error> {
        self.dst.put_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::result::Result<(), std::io::Error> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_start() {
        let mut c = XmppCodec::new();
        let mut b = BytesMut::with_capacity(1024);
        b.put_slice(b"<?xml version='1.0'?><stream:stream xmlns:stream='http://etherx.jabber.org/streams' version='1.0' xmlns='jabber:client'>");
        let r = c.decode(&mut b);
        assert!(match r {
            Ok(Some(Packet::StreamStart(_))) => true,
            _ => false,
        });
    }

    #[test]
    fn test_stream_end() {
        let mut c = XmppCodec::new();
        let mut b = BytesMut::with_capacity(1024);
        b.put_slice(b"<?xml version='1.0'?><stream:stream xmlns:stream='http://etherx.jabber.org/streams' version='1.0' xmlns='jabber:client'>");
        let r = c.decode(&mut b);
        assert!(match r {
            Ok(Some(Packet::StreamStart(_))) => true,
            _ => false,
        });
        b.put_slice(b"</stream:stream>");
        let r = c.decode(&mut b);
        assert!(match r {
            Ok(Some(Packet::StreamEnd)) => true,
            _ => false,
        });
    }

    #[test]
    fn test_truncated_stanza() {
        let mut c = XmppCodec::new();
        let mut b = BytesMut::with_capacity(1024);
        b.put_slice(b"<?xml version='1.0'?><stream:stream xmlns:stream='http://etherx.jabber.org/streams' version='1.0' xmlns='jabber:client'>");
        let r = c.decode(&mut b);
        assert!(match r {
            Ok(Some(Packet::StreamStart(_))) => true,
            _ => false,
        });

        b.put_slice("<test>ß</test".as_bytes());
        let r = c.decode(&mut b);
        assert!(match r {
            Ok(None) => true,
            _ => false,
        });

        b.put_slice(b">");
        let r = c.decode(&mut b);
        assert!(match r {
            Ok(Some(Packet::Stanza(ref el))) if el.name() == "test" && el.text() == "ß" => true,
            _ => false,
        });
    }

    #[test]
    fn test_truncated_utf8() {
        let mut c = XmppCodec::new();
        let mut b = BytesMut::with_capacity(1024);
        b.put_slice(b"<?xml version='1.0'?><stream:stream xmlns:stream='http://etherx.jabber.org/streams' version='1.0' xmlns='jabber:client'>");
        let r = c.decode(&mut b);
        assert!(match r {
            Ok(Some(Packet::StreamStart(_))) => true,
            _ => false,
        });

        b.put(&b"<test>\xc3"[..]);
        let r = c.decode(&mut b);
        assert!(match r {
            Ok(None) => true,
            _ => false,
        });

        b.put(&b"\x9f</test>"[..]);
        let r = c.decode(&mut b);
        assert!(match r {
            Ok(Some(Packet::Stanza(ref el))) if el.name() == "test" && el.text() == "ß" => true,
            _ => false,
        });
    }

    /// test case for https://gitlab.com/xmpp-rs/tokio-xmpp/issues/3
    #[test]
    fn test_atrribute_prefix() {
        let mut c = XmppCodec::new();
        let mut b = BytesMut::with_capacity(1024);
        b.put_slice(b"<?xml version='1.0'?><stream:stream xmlns:stream='http://etherx.jabber.org/streams' version='1.0' xmlns='jabber:client'>");
        let r = c.decode(&mut b);
        assert!(match r {
            Ok(Some(Packet::StreamStart(_))) => true,
            _ => false,
        });

        b.put_slice(b"<status xml:lang='en'>Test status</status>");
        let r = c.decode(&mut b);
        assert!(match r {
            Ok(Some(Packet::Stanza(ref el)))
                if el.name() == "status"
                    && el.text() == "Test status"
                    && el.attr("xml:lang").map_or(false, |a| a == "en") =>
                true,
            _ => false,
        });
    }

    /// By default, encode() only gets a BytesMut that has 8 KiB space reserved.
    #[test]
    fn test_large_stanza() {
        use futures::{executor::block_on, sink::SinkExt};
        use std::io::Cursor;
        use tokio_util::codec::FramedWrite;
        let mut framed = FramedWrite::new(Cursor::new(vec![]), XmppCodec::new());
        let mut text = "".to_owned();
        for _ in 0..2usize.pow(15) {
            text = text + "A";
        }
        let stanza = Element::builder("message", "jabber:client")
            .append(
                Element::builder("body", "jabber:client")
                    .append(text.as_ref())
                    .build(),
            )
            .build();
        block_on(framed.send(Packet::Stanza(stanza))).expect("send");
        assert_eq!(
            framed.get_ref().get_ref(),
            &format!(
                "<message xmlns='jabber:client'><body>{}</body></message>",
                text
            )
            .as_bytes()
        );
    }

    #[test]
    fn test_cut_out_stanza() {
        let mut c = XmppCodec::new();
        let mut b = BytesMut::with_capacity(1024);
        b.put_slice(b"<?xml version='1.0'?><stream:stream xmlns:stream='http://etherx.jabber.org/streams' version='1.0' xmlns='jabber:client'>");
        let r = c.decode(&mut b);
        assert!(match r {
            Ok(Some(Packet::StreamStart(_))) => true,
            _ => false,
        });

        b.put_slice(b"<message ");
        b.put_slice(b"type='chat'><body>Foo</body></message>");
        let r = c.decode(&mut b);
        assert!(match r {
            Ok(Some(Packet::Stanza(_))) => true,
            _ => false,
        });
    }
}
