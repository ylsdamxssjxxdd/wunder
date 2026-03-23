use std::str::FromStr;

use xmpp_parsers::jid::Jid;

use crate::{AsyncClient, AsyncConfig, Error, SimpleClient};

use super::ServerConfig;

impl AsyncClient<ServerConfig> {
    /// Start a new XMPP client
    ///
    /// Start polling the returned instance so that it will connect
    /// and yield events.
    pub fn new<J: Into<Jid>, P: Into<String>>(jid: J, password: P) -> Self {
        let config = AsyncConfig {
            jid: jid.into(),
            password: password.into(),
            server: ServerConfig::UseSrv,
        };
        Self::new_with_config(config)
    }
}

impl SimpleClient<ServerConfig> {
    /// Start a new XMPP client and wait for a usable session
    pub async fn new<P: Into<String>>(jid: &str, password: P) -> Result<Self, Error> {
        let jid = Jid::from_str(jid)?;
        Self::new_with_jid(jid, password.into()).await
    }

    /// Start a new client given that the JID is already parsed.
    pub async fn new_with_jid(jid: Jid, password: String) -> Result<Self, Error> {
        Self::new_with_jid_connector(ServerConfig::UseSrv, jid, password).await
    }
}
