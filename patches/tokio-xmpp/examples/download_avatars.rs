use futures::stream::StreamExt;
use minidom::Element;
use std::env::args;
use std::fs::{create_dir_all, File};
use std::io::{self, Write};
use std::process::exit;
use std::str::FromStr;
use tokio_xmpp::AsyncClient as Client;
use xmpp_parsers::{
    avatar::{Data as AvatarData, Metadata as AvatarMetadata},
    caps::{compute_disco, hash_caps, Caps},
    disco::{DiscoInfoQuery, DiscoInfoResult, Feature, Identity},
    hashes::Algo,
    iq::{Iq, IqType},
    jid::{BareJid, Jid},
    message::Message,
    ns,
    presence::{Presence, Type as PresenceType},
    pubsub::{
        event::PubSubEvent,
        pubsub::{Items, PubSub},
        NodeName,
    },
    stanza_error::{DefinedCondition, ErrorType, StanzaError},
};

#[tokio::main]
async fn main() {
    env_logger::init();

    let args: Vec<String> = args().collect();
    if args.len() != 3 {
        println!("Usage: {} <jid> <password>", args[0]);
        exit(1);
    }
    let jid = BareJid::from_str(&args[1]).expect(&format!("Invalid JID: {}", &args[1]));
    let password = args[2].clone();

    // Client instance
    let mut client = Client::new(jid.clone(), password);

    let disco_info = make_disco();

    // Main loop, processes events
    let mut wait_for_stream_end = false;
    let mut stream_ended = false;
    while !stream_ended {
        if let Some(event) = client.next().await {
            if wait_for_stream_end {
                /* Do nothing */
            } else if event.is_online() {
                println!("Online!");

                let caps = get_disco_caps(&disco_info, "https://gitlab.com/xmpp-rs/tokio-xmpp");
                let presence = make_presence(caps);
                client.send_stanza(presence.into()).await.unwrap();
            } else if let Some(stanza) = event.into_stanza() {
                if stanza.is("iq", "jabber:client") {
                    let iq = Iq::try_from(stanza).unwrap();
                    if let IqType::Get(payload) = iq.payload {
                        if payload.is("query", ns::DISCO_INFO) {
                            let query = DiscoInfoQuery::try_from(payload);
                            match query {
                                Ok(query) => {
                                    let mut disco = disco_info.clone();
                                    disco.node = query.node;
                                    let iq = Iq::from_result(iq.id, Some(disco))
                                        .with_to(iq.from.unwrap());
                                    client.send_stanza(iq.into()).await.unwrap();
                                }
                                Err(err) => client
                                    .send_stanza(make_error(
                                        iq.from.unwrap(),
                                        iq.id,
                                        ErrorType::Modify,
                                        DefinedCondition::BadRequest,
                                        &format!("{}", err),
                                    ))
                                    .await
                                    .unwrap(),
                            }
                        } else {
                            // We MUST answer unhandled get iqs with a service-unavailable error.
                            client
                                .send_stanza(make_error(
                                    iq.from.unwrap(),
                                    iq.id,
                                    ErrorType::Cancel,
                                    DefinedCondition::ServiceUnavailable,
                                    "No handler defined for this kind of iq.",
                                ))
                                .await
                                .unwrap();
                        }
                    } else if let IqType::Result(Some(payload)) = iq.payload {
                        if payload.is("pubsub", ns::PUBSUB) {
                            let pubsub = PubSub::try_from(payload).unwrap();
                            let from = iq.from.clone().unwrap_or(jid.clone().into());
                            handle_iq_result(pubsub, &from);
                        }
                    } else if let IqType::Set(_) = iq.payload {
                        // We MUST answer unhandled set iqs with a service-unavailable error.
                        client
                            .send_stanza(make_error(
                                iq.from.unwrap(),
                                iq.id,
                                ErrorType::Cancel,
                                DefinedCondition::ServiceUnavailable,
                                "No handler defined for this kind of iq.",
                            ))
                            .await
                            .unwrap();
                    }
                } else if stanza.is("message", "jabber:client") {
                    let message = Message::try_from(stanza).unwrap();
                    let from = message.from.clone().unwrap();
                    if let Some(body) = message.get_best_body(vec!["en"]) {
                        if body.0 == "die" {
                            println!("Secret die command triggered by {}", from);
                            wait_for_stream_end = true;
                            client.send_end().await.unwrap();
                        }
                    }
                    for child in message.payloads {
                        if child.is("event", ns::PUBSUB_EVENT) {
                            let event = PubSubEvent::try_from(child).unwrap();
                            if let PubSubEvent::PublishedItems { node, items } = event {
                                if node.0 == ns::AVATAR_METADATA {
                                    for item in items.into_iter() {
                                        let payload = item.payload.clone().unwrap();
                                        if payload.is("metadata", ns::AVATAR_METADATA) {
                                            // TODO: do something with these metadata.
                                            let _metadata =
                                                AvatarMetadata::try_from(payload).unwrap();
                                            println!(
                                                "[1m{}[0m has published an avatar, downloading...",
                                                from.clone()
                                            );
                                            let iq = download_avatar(from.clone());
                                            client.send_stanza(iq.into()).await.unwrap();
                                        }
                                    }
                                }
                            }
                        }
                    }
                } else if stanza.is("presence", "jabber:client") {
                    // Nothing to do here.
                    ()
                } else {
                    panic!("Unknown stanza: {}", String::from(&stanza));
                }
            }
        } else {
            println!("stream_ended");
            stream_ended = true;
        }
    }
}

fn make_error(
    to: Jid,
    id: String,
    type_: ErrorType,
    condition: DefinedCondition,
    text: &str,
) -> Element {
    let error = StanzaError::new(type_, condition, "en", text);
    let iq = Iq::from_error(id, error).with_to(to);
    iq.into()
}

fn make_disco() -> DiscoInfoResult {
    let identities = vec![Identity::new("client", "bot", "en", "tokio-xmpp")];
    let features = vec![
        Feature::new(ns::DISCO_INFO),
        Feature::new(format!("{}+notify", ns::AVATAR_METADATA)),
    ];
    DiscoInfoResult {
        node: None,
        identities,
        features,
        extensions: vec![],
    }
}

fn get_disco_caps(disco: &DiscoInfoResult, node: &str) -> Caps {
    let caps_data = compute_disco(disco);
    let hash = hash_caps(&caps_data, Algo::Sha_1).unwrap();
    Caps::new(node, hash)
}

// Construct a <presence/>
fn make_presence(caps: Caps) -> Presence {
    let mut presence = Presence::new(PresenceType::None).with_priority(-1);
    presence.set_status("en", "Downloading avatars.");
    presence.add_payload(caps);
    presence
}

fn download_avatar(from: Jid) -> Iq {
    Iq::from_get(
        "coucou",
        PubSub::Items(Items {
            max_items: None,
            node: NodeName(String::from(ns::AVATAR_DATA)),
            subid: None,
            items: Vec::new(),
        }),
    )
    .with_to(from)
}

fn handle_iq_result(pubsub: PubSub, from: &Jid) {
    if let PubSub::Items(items) = pubsub {
        if items.node.0 == ns::AVATAR_DATA {
            for item in items.items {
                match (item.id.clone(), item.payload.clone()) {
                    (Some(id), Some(payload)) => {
                        let data = AvatarData::try_from(payload).unwrap();
                        save_avatar(from, id.0, &data.data).unwrap();
                    }
                    _ => {}
                }
            }
        }
    }
}

// TODO: may use tokio?
fn save_avatar(from: &Jid, id: String, data: &[u8]) -> io::Result<()> {
    let directory = format!("data/{}", from);
    let filename = format!("data/{}/{}", from, id);
    println!("Saving avatar from [1m{}[0m to [4m{}[0m.", from, filename);
    create_dir_all(directory)?;
    let mut file = File::create(filename)?;
    file.write_all(data)
}
