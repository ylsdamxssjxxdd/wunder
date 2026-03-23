use futures::stream::StreamExt;
use minidom::Element;
use std::env::args;
use std::process::exit;
use std::str::FromStr;
use tokio_xmpp::tcp::TcpComponent as Component;
use xmpp_parsers::jid::Jid;
use xmpp_parsers::message::{Body, Message, MessageType};
use xmpp_parsers::presence::{Presence, Show as PresenceShow, Type as PresenceType};

#[tokio::main]
async fn main() {
    env_logger::init();

    let args: Vec<String> = args().collect();
    if args.len() < 3 || args.len() > 4 {
        println!("Usage: {} <jid> <password> [server:port]", args[0]);
        exit(1);
    }
    let jid = &args[1];
    let password = &args[2];
    let server = args
        .get(3)
        .unwrap()
        .parse()
        .unwrap_or("127.0.0.1:5347".to_owned());

    // Component instance
    println!("{} {} {}", jid, password, server);
    let mut component = Component::new(jid, password, server).await.unwrap();

    // Make the two interfaces for sending and receiving independent
    // of each other so we can move one into a closure.
    println!("Online: {}", component.jid);

    // TODO: replace these hardcoded JIDs
    let presence = make_presence(
        Jid::from_str("test@component.linkmauve.fr/coucou").unwrap(),
        Jid::from_str("linkmauve@linkmauve.fr").unwrap(),
    );
    component.send_stanza(presence).await.unwrap();

    // Main loop, processes events
    loop {
        if let Some(stanza) = component.next().await {
            if let Some(message) = Message::try_from(stanza).ok() {
                // This is a message we'll echo
                match (message.from, message.bodies.get("")) {
                    (Some(from), Some(body)) => {
                        if message.type_ != MessageType::Error {
                            let reply = make_reply(from, &body.0);
                            component.send_stanza(reply).await.unwrap();
                        }
                    }
                    _ => (),
                }
            }
        } else {
            break;
        }
    }
}

// Construct a <presence/>
fn make_presence(from: Jid, to: Jid) -> Element {
    let mut presence = Presence::new(PresenceType::None);
    presence.from = Some(from);
    presence.to = Some(to);
    presence.show = Some(PresenceShow::Chat);
    presence
        .statuses
        .insert(String::from("en"), String::from("Echoing messages."));
    presence.into()
}

// Construct a chat <message/>
fn make_reply(to: Jid, body: &str) -> Element {
    let mut message = Message::new(Some(to));
    message.bodies.insert(String::new(), Body(body.to_owned()));
    message.into()
}
