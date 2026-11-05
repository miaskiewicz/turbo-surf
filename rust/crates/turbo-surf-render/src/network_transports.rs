//! Native transports behind `WebSocket`/`WebSocketStream` (websocket.js,
//! websocket_stream.js) and the liveness of `EventSource` streams (event_source.js),
//! issue #144. A socket is a turbo-surf-core [`websocket`] connection opened with the
//! creating document's origin, cookies (SameSite-filtered like a credentialed request)
//! and identity, after its `connect-src` check. Event reads are unref'd op promises, so
//! an open socket or stream never holds a page render; an awaited evaluation keeps
//! polling while any transport is live ([`live`]).
use super::{
    cookie_now, document_cookie_target, queue_csp_violations, window_origin, Base, CspHandle, Jar,
    Ua,
};
use deno_core::{op2, v8, OpState, ToJsBuffer};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use turbo_surf_core::request_context::{Credentials, Destination, RequestContext, RequestMode};
use turbo_surf_core::websocket::{self, WsCommand, WsEvent, WsHandle, WsRequest};

type Events = Rc<RefCell<Option<tokio::sync::mpsc::UnboundedReceiver<WsEvent>>>>;

struct Socket {
    handle: WsHandle,
    events: Events,
    jar: Jar,
    cookie_url: String,
}

/// Open sockets plus the EventSource streams that asked to be counted as live.
#[derive(Default)]
pub(super) struct NetworkTransports {
    next: u32,
    sockets: HashMap<u32, Socket>,
    holds: usize,
}

/// Whether a transport may still deliver events to a waiting evaluation.
pub(super) fn live(state: &OpState) -> bool {
    state
        .try_borrow::<NetworkTransports>()
        .is_some_and(|transports| !transports.sockets.is_empty() || transports.holds > 0)
}

fn transports(state: &mut OpState) -> &mut NetworkTransports {
    if !state.has::<NetworkTransports>() {
        state.put(NetworkTransports::default());
    }
    state.borrow_mut::<NetworkTransports>()
}

/// The http(s) URL a ws(s) URL shares its origin, cookies and CSP host-source with.
fn http_equivalent(url: &str) -> String {
    if let Some(rest) = url.strip_prefix("wss:") {
        format!("https:{rest}")
    } else if let Some(rest) = url.strip_prefix("ws:") {
        format!("http:{rest}")
    } else {
        url.to_owned()
    }
}

/// The violation's blockedURI: Chrome 156 names a refused WebSocket or EventSource URL
/// in full (fragment dropped), cross-origin ones included.
fn blocked_url(url: &str) -> String {
    url::Url::parse(url).map_or_else(
        |_| url.to_owned(),
        |mut parsed| {
            parsed.set_fragment(None);
            parsed.to_string()
        },
    )
}

/// `connect-src` for a WebSocket or EventSource URL. CSP3 matches ws/wss like http/https
/// ('self' included), so either spelling may satisfy a source.
fn csp_allows<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    document: v8::Local<'s, v8::Object>,
    url: &str,
) -> bool {
    let Some(handle) = context.get_slot::<CspHandle>() else {
        return true;
    };
    let (active, report_only) = {
        let policy = handle.policy.borrow();
        (policy.active.clone(), policy.report_only.clone())
    };
    let http = http_equivalent(url);
    let blocked = blocked_url(url);
    let mut allowed = true;
    for (policy, disposition) in [(&active, "enforce"), (&report_only, "report")] {
        let passes = policy.allows_request("connect-src", &http, "", false)
            || policy.allows_request("connect-src", url, "", false);
        if !passes {
            let records = policy.request_violations("connect-src", &http, "");
            queue_csp_violations(
                scope,
                context,
                document,
                "connect-src",
                &blocked,
                disposition,
                records,
            );
            allowed &= disposition != "enforce";
        }
    }
    allowed
}

/// The `connect-src` check (queueing any violation) a socket or event stream runs
/// before connecting. Separate from [`op_ws_open`]: violation reports borrow the op
/// state themselves.
#[op2(fast, reentrant)]
pub(super) fn op_connect_allowed<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    #[string] url: &str,
) -> bool {
    document
        .get_creation_context(scope)
        .is_some_and(|context| csp_allows(scope, context, document, url))
}

/// Starts a WebSocket for `document` (after [`op_connect_allowed`]); 0 without a realm.
#[op2(reentrant)]
pub(super) fn op_ws_open<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    state: &mut OpState,
    document: v8::Local<'s, v8::Object>,
    #[string] url: String,
    #[serde] protocols: Vec<String>,
) -> u32 {
    let Some(context) = document.get_creation_context(scope) else {
        return 0;
    };
    let origin = window_origin(scope, context);
    let (source, jar) = document_cookie_target(scope, state, document);
    let cookie_url = http_equivalent(&url);
    let mut request_context =
        RequestContext::resource(&source, Destination::Empty, RequestMode::Cors);
    request_context.credentials = Credentials::Include;
    request_context.top_level_source = Some(state.borrow::<Base>().0.clone());
    let cookie =
        jar.borrow()
            .cookie_header_for_request(&cookie_url, cookie_now(), &request_context, "GET");
    let profile = turbo_surf_core::fingerprint::default_profile();
    let user_agent = state
        .try_borrow::<Ua>()
        .map(|ua| ua.0.clone())
        .filter(|ua| !ua.is_empty())
        .unwrap_or_else(|| profile.user_agent.clone());
    let (handle, events) = websocket::open(WsRequest {
        url,
        origin,
        protocols,
        user_agent,
        accept_language: profile.accept_language.to_owned(),
        cookie,
        trust_anchor: None,
    });
    let transports = transports(state);
    transports.next += 1;
    let id = transports.next;
    transports.sockets.insert(
        id,
        Socket {
            handle,
            events: Rc::new(RefCell::new(Some(events))),
            jar,
            cookie_url,
        },
    );
    id
}

fn command(state: &mut OpState, id: u32, command: WsCommand) {
    if let Some(socket) = transports(state).sockets.get(&id) {
        socket.handle.send(command);
    }
}

#[op2(fast)]
pub(super) fn op_ws_send_text(state: &mut OpState, id: u32, #[string] text: String) {
    command(state, id, WsCommand::Text(text));
}

#[op2(fast)]
pub(super) fn op_ws_send_binary(state: &mut OpState, id: u32, #[buffer] bytes: &[u8]) {
    command(state, id, WsCommand::Binary(bytes.to_vec()));
}

/// Starts the closing handshake; a negative code sends a close frame without a status.
#[op2(fast)]
pub(super) fn op_ws_close(state: &mut OpState, id: u32, code: i32, #[string] reason: String) {
    let code = u16::try_from(code).ok();
    command(state, id, WsCommand::Close(code, reason));
}

/// Drops the connection (closing before it opened fails it without a handshake).
#[op2(fast)]
pub(super) fn op_ws_drop(state: &mut OpState, id: u32) {
    transports(state).sockets.remove(&id);
}

/// One transport event for the JS side.
#[derive(serde::Serialize, Default)]
pub(super) struct WsOut {
    kind: &'static str,
    text: String,
    bytes: Option<ToJsBuffer>,
    protocol: String,
    extensions: String,
    clean: bool,
    code: u16,
    reason: String,
    sent: usize,
}

/// The next event of socket `id` ("gone" once it was dropped or finished).
#[op2]
#[serde]
pub(super) async fn op_ws_next(state: Rc<RefCell<OpState>>, id: u32) -> WsOut {
    let slot = transports(&mut state.borrow_mut())
        .sockets
        .get(&id)
        .map(|socket| socket.events.clone());
    let receiver = slot.as_ref().and_then(|slot| slot.borrow_mut().take());
    let (Some(slot), Some(mut receiver)) = (slot, receiver) else {
        return WsOut {
            kind: "gone",
            ..WsOut::default()
        };
    };
    let event = receiver.recv().await;
    *slot.borrow_mut() = Some(receiver);
    deliver(&mut state.borrow_mut(), id, event)
}

fn deliver(state: &mut OpState, id: u32, event: Option<WsEvent>) -> WsOut {
    let transports = transports(state);
    let out = match event {
        Some(WsEvent::Open(accepted)) => {
            if let Some(socket) = transports.sockets.get(&id) {
                socket.jar.borrow_mut().set_from_response(
                    &socket.cookie_url,
                    &accepted.set_cookies,
                    cookie_now(),
                );
            }
            WsOut {
                kind: "open",
                protocol: accepted.protocol,
                extensions: accepted.extensions,
                ..WsOut::default()
            }
        }
        Some(event) => event_out(event),
        None => WsOut {
            kind: "gone",
            ..WsOut::default()
        },
    };
    if matches!(out.kind, "closed" | "failed" | "gone") {
        transports.sockets.remove(&id);
    }
    out
}

fn event_out(event: WsEvent) -> WsOut {
    match event {
        WsEvent::Text(text) => WsOut {
            kind: "text",
            text,
            ..WsOut::default()
        },
        WsEvent::Binary(bytes) => WsOut {
            kind: "binary",
            bytes: Some(bytes.into()),
            ..WsOut::default()
        },
        WsEvent::Sent(sent) => WsOut {
            kind: "sent",
            sent,
            ..WsOut::default()
        },
        WsEvent::Closed {
            clean,
            code,
            reason,
        } => WsOut {
            kind: "closed",
            clean,
            code,
            reason,
            ..WsOut::default()
        },
        WsEvent::Failed | WsEvent::Open(_) => WsOut {
            kind: "failed",
            ..WsOut::default()
        },
    }
}

/// An EventSource stream counts as live from construction until it is CLOSED.
#[op2(fast)]
pub(super) fn op_net_transport_hold(state: &mut OpState, live: bool) {
    hold(state, live);
}

fn hold(state: &mut OpState, live: bool) {
    let transports = transports(state);
    if live {
        transports.holds += 1;
    } else {
        transports.holds = transports.holds.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ws_urls_share_their_http_origin() {
        assert_eq!(http_equivalent("ws://a.test:1/x?y"), "http://a.test:1/x?y");
        assert_eq!(http_equivalent("wss://a.test/"), "https://a.test/");
        assert_eq!(http_equivalent("https://a.test/"), "https://a.test/");
    }

    #[test]
    fn event_records_name_their_kind() {
        let kinds: Vec<_> = [
            WsEvent::Text("t".into()),
            WsEvent::Binary(vec![1]),
            WsEvent::Sent(3),
            WsEvent::Closed {
                clean: true,
                code: 1000,
                reason: String::new(),
            },
            WsEvent::Failed,
        ]
        .into_iter()
        .map(|event| event_out(event).kind)
        .collect();
        assert_eq!(kinds, ["text", "binary", "sent", "closed", "failed"]);
        let mut state = OpState::new(None);
        assert!(!live(&state));
        hold(&mut state, true);
        assert!(live(&state));
        hold(&mut state, false);
        hold(&mut state, false);
        assert!(!live(&state));
        assert_eq!(deliver(&mut state, 9, None).kind, "gone");
    }
}
