//! The transports of the collections: the proxy (the application's, with
//! its password kept sealed apart, or each collection's own), the trust in
//! servers, and the collection's client certificates.

use crate::messages::Say as _;
use std::path::Path;
use std::sync::Arc;

use apifast_app::Cookies;
use apifast_domain::{Collection, ProxyChoice, Secret, TlsTrust};
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

    /// A transport for the collection at `root`, through its proxy and
    /// with its cookie jar. A wrong CA file does not keep collections from
    /// opening: the transport goes without it, and the error shows.
    pub(super) fn transport_for(
        &mut self,
        root: &Path,
        collection: &Collection,
        cookies: Arc<Cookies>,
    ) -> Result<ReqwestTransport, String> {
        let proxy = collection.proxy.choose(&self.application_proxy());
        let certificates = &collection.client_certificates;
        let transport = match ReqwestTransport::configured(
            root,
            proxy.clone(),
            &self.trust,
            certificates,
            &collection.dns_overrides,
        ) {
            Ok(transport) => transport,
            Err(e) => {
                self.snapshot.error = Some(e.say(self.strings));
                // Without the trust and the certificates, but with the
                // collection's DNS overrides.
                let dns = &collection.dns_overrides;
                ReqwestTransport::configured(root, proxy, &TlsTrust::default(), &[], dns)
                    .map_err(|e| e.say(self.strings))?
            }
        };
        Ok(transport.with_cookies(cookies))
    }

    /// New transports for the loaded collections, after the proxy changed.
    /// One that cannot be built keeps its old transport, and says why.
    pub(super) fn rebuild_transports(&mut self) {
        let roots: Vec<_> = self.open.keys().cloned().collect();
        for root in roots {
            let Some((collection, cookies)) =
                (self.open.get(&root)).map(|o| (o.tree.collection.clone(), o.cookies.clone()))
            else {
                continue;
            };
            match self.transport_for(&root, &collection, cookies) {
                Ok(transport) => {
                    if let Some(open) = self.open.get_mut(&root) {
                        open.transport = Arc::new(transport);
                    }
                }
                Err(message) => self.snapshot.error = Some(message),
            }
        }
    }

    /// Saves the collection's proxy and client certificates into its
    /// file, then sends with them.
    pub(super) fn save_collection_settings(
        &mut self,
        root: &Path,
        (proxy, client_certificates, dns_overrides): (
            apifast_domain::CollectionProxy,
            Vec<apifast_domain::ClientCertificate>,
            Vec<apifast_domain::DnsOverride>,
        ),
    ) {
        let Some(repo) = self.open.get(root).map(|open| open.repo.clone()) else {
            return;
        };
        let saved = apifast_app::CollectionRepository::collection(&*repo).and_then(|mut c| {
            c.proxy = proxy;
            c.client_certificates = client_certificates;
            c.dns_overrides = dns_overrides;
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
            Err(e) => self.snapshot.error = Some(e.say(self.strings)),
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
            Err(e) => self.snapshot.error = Some(e.say(self.strings)),
        }
    }
}
