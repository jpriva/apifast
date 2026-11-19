//! WebSocket connections (docs/07, F7): sending a WebSocket request opens a
//! connection that stays open until either side closes it. While it is
//! open the user sends its messages one by one, and what happens is read
//! live into the tab, as an event stream is.

use std::sync::Arc;

use apifast_app::{
    CancelToken, Clock, NoPrompter, NoSecrets, SecretStore, SendOptions, SendPorts, WsOutbox,
    connect_request,
};
use apifast_domain::{Body, WsMessage};
use apifast_ws::TungsteniteTransport;
use tokio::sync::mpsc;

use super::stream::LiveResponses;
use super::{InFlight, LiveStream, Message, ResponseState, State, TabKey, finished};
use crate::system::{SystemClock, SystemRandom};

/// The messages the user sends, as the core passes them on.
struct Outbox(mpsc::UnboundedReceiver<WsMessage>);

impl WsOutbox for Outbox {
    async fn next(&mut self) -> Option<WsMessage> {
        // `None` once the core lets go of the connection: it closes.
        self.0.recv().await
    }
}

impl State {
    /// Connects the WebSocket request of the tab `key`, as it is on screen.
    pub(super) fn start_connect(&mut self, key: TabKey) {
        let Some(open) = self.open.get(&key.root) else {
            return;
        };
        let (repo, transport, responses, history, tokens) = (
            open.repo.clone(),
            open.transport.clone(),
            open.responses.clone(),
            open.history.clone(),
            open.tokens.clone(),
        );
        let secrets: Arc<dyn SecretStore + Send> = match &open.vault {
            Some(vault) => vault.clone(),
            None => Arc::new(NoSecrets),
        };
        let options = SendOptions {
            environment: open.environment.clone(),
            draft: self.tab(&key).and_then(|tab| tab.draft.clone()),
            runtime: open.runtime.clone(),
            ..SendOptions::default()
        };
        let Some(tab) = self.tab_mut(&key) else {
            return;
        };
        let request = tab.request();
        let written = (request.method.as_str().to_owned(), request.url.clone());
        tab.response = ResponseState::Sending;
        let cancel = CancelToken::new();
        let id = self.next_revision();
        let stream = LiveStream::default();
        let (outbox, receiver) = mpsc::unbounded_channel();
        let in_flight = InFlight {
            id,
            cancel: cancel.clone(),
            stream: stream.clone(),
            outbox: Some(outbox),
        };
        self.sending.insert(key.clone(), in_flight);

        let messages = self.messages.clone();
        let live = LiveResponses {
            store: responses.clone(),
            stream,
            id,
            messages: messages.clone(),
        };
        tokio::spawn(async move {
            let authorizer = apifast_http::LoopbackAuthorizer::system();
            let (clock, random) = (SystemClock, SystemRandom::default());
            let ports = SendPorts {
                collections: &*repo,
                environments: &*repo,
                transport: &*transport,
                responses: &live,
                clock: &clock,
                random: &random,
                prompter: &NoPrompter,
                secrets: &*secrets,
                report: &|_| {},
                history: Some(&*history),
                tokens: Some(tokens.as_ref()),
                authorizer: Some(&authorizer),
            };
            let sockets = TungsteniteTransport::new((*transport).clone());
            let at_ms = clock.unix_millis();
            let mut outbox = Outbox(receiver);
            let result =
                connect_request(ports, (&sockets, &mut outbox), &key.path, &options, &cancel).await;
            let finished = finished(id, key, (written, at_ms), result, (&responses, &*history));
            // Gone only when the core is stopping, and then nobody waits.
            if let Some(messages) = messages.upgrade() {
                let _ = messages.send(Message::SendFinished(Box::new(finished)));
            }
        });
    }

    /// Sends message number `index` of the active tab's WebSocket request,
    /// as it is on screen, if its connection is open.
    pub(super) fn send_message(&mut self, index: usize) {
        let Some(key) = self.snapshot.active.clone() else {
            return;
        };
        let message = self.tab(&key).and_then(|tab| match &tab.request().body {
            Body::WebSocket(messages) => messages.get(index).cloned(),
            _ => None,
        });
        let outbox = (self.sending.get(&key)).and_then(|sending| sending.outbox.as_ref());
        if let (Some(message), Some(outbox)) = (message, outbox) {
            // Fails only once the connection closed, when there is no one
            // to send it to.
            let _ = outbox.send(message);
        }
    }
}
