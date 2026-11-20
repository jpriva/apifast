//! What a script asks of the application (`bru.sendRequest`,
//! `bru.runRequest`). The script's runtime holds no reference to the
//! application: it sends each call down a channel, and the engine serves
//! them with the [`ScriptHost`] while the script runs, answering in JSON.

use apifast_app::{RanRequest, ScriptHost, ScriptHttpRequest, ScriptHttpResponse};
use serde_json::{Value as Json, json};
use tokio::sync::{mpsc, oneshot};

use crate::api;

/// A call of the script, and where its answer goes: `{"ok": …}` or
/// `{"error": "…"}`.
pub(crate) struct Call {
    ask: Ask,
    reply: oneshot::Sender<String>,
}

enum Ask {
    Send(ScriptHttpRequest),
    Run(String),
}

/// Where a script sends its calls.
pub(crate) type Calls = mpsc::UnboundedSender<Call>;

/// Sends the call of `kind` (`send` or `run`) with `payload` (JSON), and
/// waits for its answer.
pub(crate) async fn ask(calls: &Calls, kind: &str, payload: &str) -> String {
    let failed = |message: &str| json!({ "error": message }).to_string();
    let Ok(payload) = serde_json::from_str::<Json>(payload) else {
        return failed("the call is not valid JSON");
    };
    let ask = match kind {
        "send" => Ask::Send(http_request(&payload)),
        "run" => Ask::Run(payload.as_str().unwrap_or_default().to_owned()),
        _ => return failed("unknown call"),
    };
    let (reply, answer) = oneshot::channel();
    if calls.send(Call { ask, reply }).is_err() {
        return failed("the script can no longer ask anything");
    }
    // Dropped unanswered when the script ran out of time.
    answer
        .await
        .unwrap_or_else(|_| failed("the script ran out of time"))
}

/// Answers `call` with `host`.
pub(crate) async fn serve(host: &dyn ScriptHost, call: Call) {
    let answer = match call.ask {
        Ask::Send(request) => host
            .send_request(request)
            .await
            .map(|response| http_response(&response)),
        Ask::Run(path) => host.run_request(&path).await.map(|ran| ran_json(&ran)),
    };
    let text = match answer {
        Ok(value) => json!({ "ok": value }),
        Err(message) => json!({ "error": message }),
    };
    // The script may have ended meanwhile.
    let _ = call.reply.send(text.to_string());
}

fn http_request(json: &Json) -> ScriptHttpRequest {
    let text = |key: &str| json.get(key).and_then(Json::as_str).map(str::to_owned);
    let headers = (json
        .get("headers")
        .and_then(Json::as_array)
        .into_iter()
        .flatten())
    .filter_map(|pair| {
        Some((
            pair.get(0)?.as_str()?.to_owned(),
            pair.get(1)?.as_str()?.to_owned(),
        ))
    })
    .collect();
    ScriptHttpRequest {
        method: text("method").unwrap_or_else(|| "GET".to_owned()),
        url: text("url").unwrap_or_default(),
        headers,
        body: text("body"),
        timeout_ms: json.get("timeout_ms").and_then(Json::as_u64),
    }
}

fn http_response(response: &ScriptHttpResponse) -> Json {
    json!({
        "status": response.status,
        "status_text": response.status_text,
        "headers": response.headers,
        "body": response.body,
        "time_ms": response.time_ms,
        "url": response.url,
    })
}

fn ran_json(ran: &RanRequest) -> Json {
    json!({
        "response": http_response(&ran.response),
        "variables": ran.variables.iter().map(api::change_json).collect::<Vec<_>>(),
    })
}
