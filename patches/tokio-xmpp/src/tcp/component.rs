use crate::{Component, Error};

use super::TcpServerConnector;

impl Component<TcpServerConnector> {
    /// Start a new XMPP component
    pub async fn new(jid: &str, password: &str, server: String) -> Result<Self, Error> {
        Self::new_with_connector(jid, password, TcpServerConnector::new(server)).await
    }
}
