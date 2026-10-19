//! CesiumJS `TrustedServers`; registry and page protocol are explicit host state.

use super::uri::Uri;
use std::collections::HashSet;

#[derive(Clone, Debug, Default)]
pub struct TrustedServers {
    servers: HashSet<String>,
}

impl TrustedServers {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn add(&mut self, host: &str, port: u16) {
        self.servers.insert(format!("{}:{port}", host.to_lowercase()));
    }
    pub fn remove(&mut self, host: &str, port: u16) {
        self.servers.remove(&format!("{}:{port}", host.to_lowercase()));
    }
    pub fn clear(&mut self) {
        self.servers.clear();
    }

    pub fn contains(&self, url: &str, page_protocol: &str) -> bool {
        let mut uri = Uri::parse(url);
        uri.normalize();
        uri.username = None;
        uri.password = None;
        let mut authority = super::request_scheduler::authority(&uri);
        if authority.is_empty() {
            return false;
        }
        if !authority.contains(':') {
            let scheme = if uri.scheme().is_empty() { page_protocol.trim_end_matches(':') } else { uri.scheme() };
            match scheme {
                "http" => authority.push_str(":80"),
                "https" => authority.push_str(":443"),
                _ => return false,
            }
        }
        self.servers.contains(&authority)
    }
}
