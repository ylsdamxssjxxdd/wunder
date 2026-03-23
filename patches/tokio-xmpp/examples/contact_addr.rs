use futures::stream::StreamExt;
use minidom::Element;
use std::env::args;
use std::process::exit;
use std::str::FromStr;
use tokio_xmpp::AsyncClient as Client;
use xmpp_parsers::{
    disco::{DiscoInfoQuery, DiscoInfoResult},
    iq::{Iq, IqType},
    jid::{BareJid, Jid},
    ns,
    server_info::ServerInfo,
};

#[tokio::main]
async fn main() {
    env_logger::init();

    let args: Vec<String> = args().collect();
    if args.len() != 4 {
        println!("Usage: {} <jid> <password> <target>", args[0]);
        exit(1);
    }
    let jid = BareJid::from_str(&args[1]).expect(&format!("Invalid JID: {}", &args[1]));
    let password = args[2].clone();
    let target = &args[3];

    // Client instance
    let mut client = Client::new(jid, password);

    // Main loop, processes events
    let mut wait_for_stream_end = false;
    let mut stream_ended = false;
    while !stream_ended {
        if let Some(event) = client.next().await {
            if wait_for_stream_end {
                /* Do Nothing. */
            } else if event.is_online() {
                println!("Online!");

                let target_jid: Jid = target.clone().parse().unwrap();
                let iq = make_disco_iq(target_jid);
                println!("Sending disco#info request to {}", target.clone());
                println!(">> {}", String::from(&iq));
                client.send_stanza(iq).await.unwrap();
            } else if let Some(stanza) = event.into_stanza() {
                if stanza.is("iq", "jabber:client") {
                    let iq = Iq::try_from(stanza).unwrap();
                    if let IqType::Result(Some(payload)) = iq.payload {
                        if payload.is("query", ns::DISCO_INFO) {
                            if let Ok(disco_info) = DiscoInfoResult::try_from(payload) {
                                for ext in disco_info.extensions {
                                    if let Ok(server_info) = ServerInfo::try_from(ext) {
                                        print_server_info(server_info);
                                    }
                                }
                            }
                        }
                        wait_for_stream_end = true;
                        client.send_end().await.unwrap();
                    }
                }
            }
        } else {
            stream_ended = true;
        }
    }
}

fn make_disco_iq(target: Jid) -> Element {
    Iq::from_get("disco", DiscoInfoQuery { node: None })
        .with_id(String::from("contact"))
        .with_to(target)
        .into()
}

fn convert_field(field: Vec<String>) -> String {
    field
        .iter()
        .fold((field.len(), String::new()), |(l, mut acc), s| {
            acc.push('<');
            acc.push_str(&s);
            acc.push('>');
            if l > 1 {
                acc.push(',');
                acc.push(' ');
            }
            (0, acc)
        })
        .1
}

fn print_server_info(server_info: ServerInfo) {
    if server_info.abuse.len() != 0 {
        println!("abuse: {}", convert_field(server_info.abuse));
    }
    if server_info.admin.len() != 0 {
        println!("admin: {}", convert_field(server_info.admin));
    }
    if server_info.feedback.len() != 0 {
        println!("feedback: {}", convert_field(server_info.feedback));
    }
    if server_info.sales.len() != 0 {
        println!("sales: {}", convert_field(server_info.sales));
    }
    if server_info.security.len() != 0 {
        println!("security: {}", convert_field(server_info.security));
    }
    if server_info.support.len() != 0 {
        println!("support: {}", convert_field(server_info.support));
    }
}
