//! The proxy of the collections: the application's, from the settings, with
//! its password kept sealed apart, or each collection's own.

use std::path::Path;
use std::sync::Arc;

use apifast_domain::{Collection, ProxyChoice, Secret};
use apifast_http::ReqwestTransport;

use super::State;

/// Where the application proxy's password is kept.
const PASSWORD: &str = "proxy_password";

impl State {
    /// The application's proxy, with its password.
    fn application_proxy(&self) -> ProxyChoice {
        match &self.proxy {
            ProxyChoice::Manual(config) if config.auth => {
                let mut config = config.clone();
                if let Ok(Some(password)) = self.sealed.get(PASSWORD) {
                    config.password = String::from_utf8_lossy(&password).into_owned();
                }
                ProxyChoice::Manual(config)
            }
            other => other.clone(),
        }
    }

    /// A transport for the collection at `root`, through its proxy.
    pub(super) fn transport_for(
        &self,
        root: &Path,
        collection: &Collection,
    ) -> Result<ReqwestTransport, String> {
        let proxy = collection.proxy.choose(&self.application_proxy());
        ReqwestTransport::with_proxy(root, proxy).map_err(|e| e.to_string())
    }

    /// New transports for the loaded collections, after the proxy changed.
    /// One that cannot be built keeps its old transport, and says why.
    pub(super) fn rebuild_transports(&mut self) {
        let roots: Vec<_> = self.open.keys().cloned().collect();
        for root in roots {
            let Some(collection) = self.open.get(&root).map(|o| o.tree.collection.clone()) else {
                continue;
            };
            match self.transport_for(&root, &collection) {
                Ok(transport) => {
                    if let Some(open) = self.open.get_mut(&root) {
                        open.transport = Arc::new(transport);
                    }
                }
                Err(message) => self.snapshot.error = Some(message),
            }
        }
    }

    /// Saves the collection's proxy into its file, then sends through it.
    pub(super) fn save_collection_proxy(
        &mut self,
        root: &Path,
        proxy: apifast_domain::CollectionProxy,
    ) {
        let Some(repo) = self.open.get(root).map(|open| open.repo.clone()) else {
            return;
        };
        let saved = apifast_app::CollectionRepository::collection(&*repo).and_then(|mut c| {
            c.proxy = proxy;
            apifast_app::CollectionRepository::save_collection(&*repo, &c)?;
            Ok(c)
        });
        match saved {
            Ok(collection) => {
                if let Some(open) = self.open.get_mut(root) {
                    Arc::make_mut(&mut open.tree).collection = collection;
                }
                self.rebuild_transports();
                self.snapshot.error = None;
            }
            Err(e) => self.snapshot.error = Some(e.to_string()),
        }
    }

    pub(super) fn set_proxy_password(&mut self, password: Option<&Secret>) {
        let result = match password.filter(|p| !p.expose().is_empty()) {
            Some(password) => self.sealed.set(PASSWORD, password.expose().as_bytes()),
            None => self.sealed.remove(PASSWORD),
        };
        match result {
            Ok(()) => {
                self.snapshot.proxy_password_saved =
                    password.is_some_and(|p| !p.expose().is_empty());
                self.rebuild_transports();
            }
            Err(e) => self.snapshot.error = Some(e.to_string()),
        }
    }
}
