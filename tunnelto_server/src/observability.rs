use tracing::Span;
use tracing_honeycomb::{register_dist_tracing_root, TraceId};
// use warp::trace::Info;

pub fn remote_trace(source: &str) -> Span {
    let current = tracing::Span::current();

    let trace_id = TraceId::new();
    let id = crate::CONFIG.instance_id.clone();

    // Create a span using tracing macros
    let span = tracing::info_span!(target: "event", parent: &current, "begin span", id = %id, source = %source, req = %trace_id);
    span.in_scope(|| {
        let _ = register_dist_tracing_root(trace_id, None).map_err(|e| {
            eprintln!("register trace root error: {:?}", e);
        });
    });
    span
}
//
// pub fn network_trace(info: Info) -> Span {
//     let request_id = TraceId::new();
//     let method = info.method();
//     let path = info.path();
//     let remote_addr = info
//         .remote_addr()
//         .map(|a| a.to_string())
//         .unwrap_or_default();
//     let id = crate::CONFIG.instance_id.clone();
//
//     // Create a span using tracing macros
//     let span = tracing::info_span!(
//         "net-gossip",
//         id = %id,
//         req = %request_id,
//         ?method,
//         ?path,
//         ?remote_addr
//     );
//
//     span.in_scope(|| {
//         if let Err(err) = register_dist_tracing_root(request_id, None) {
//             eprintln!("register trace root error (warp): {:?}", err);
//         }
//     });
//
//     span
// }
