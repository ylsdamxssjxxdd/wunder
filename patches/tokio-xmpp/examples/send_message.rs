use std::env::args;
use std::io::{stdin, Read};
use std::process::exit;
use std::str::FromStr;
use tokio_xmpp::SimpleClient as Client;
use xmpp_parsers::jid::Jid;
use xmpp_parsers::message::{Body, Message};

#[tokio::main]
async fn main() {
    env_logger::init();

    let args: Vec<String> = args().collect();
    if args.len() != 4 {
        println!("Usage: {} <jid> <password> <recipient>", args[0]);
        exit(1);
    }
    // Configuration
    let jid = &args[1];
    let password = &args[2];
    let recipient = Jid::from_str(&args[3]).unwrap();

    // Client instance
    let mut client = Client::new(jid, password.to_owned()).await.unwrap();

    // Read from stdin
    println!("Client connected, type message and submit with Ctrl-D");
    let mut body = String::new();
    stdin().lock().read_to_string(&mut body).unwrap();

    // Send message
    let mut message = Message::new(Some(recipient));
    message.bodies.insert(String::new(), Body(body.to_owned()));
    client.send_stanza(message).await.unwrap();

    // Close client connection
    client.end().await.unwrap();
}
