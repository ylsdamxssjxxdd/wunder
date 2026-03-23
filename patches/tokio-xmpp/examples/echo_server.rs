use futures::{SinkExt, StreamExt};
use tokio::{self, io, net::TcpSocket};
use tokio_util::codec::Framed;

use tokio_xmpp::XmppCodec;

#[tokio::main]
async fn main() -> Result<(), io::Error> {
    // TCP socket
    let address = "127.0.0.1:5222".parse().unwrap();
    let socket = TcpSocket::new_v4()?;
    socket.bind(address)?;

    let listener = socket.listen(1024)?;

    // Main loop, accepts incoming connections
    loop {
        let (stream, _addr) = listener.accept().await?;

        // Use the `XMPPCodec` to encode and decode frames
        let mut framed = Framed::new(stream, XmppCodec::new());

        tokio::spawn(async move {
            while let Some(packet) = framed.next().await {
                match packet {
                    Ok(packet) => {
                        println!("Received packet: {:?}", packet);
                        framed.send(packet).await.unwrap();
                    }
                    Err(e) => {
                        eprintln!("Error: {:?}", e);
                    }
                }
            }
        });
    }
}
