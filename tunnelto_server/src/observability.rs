use tracing::Span;
use tracing_honeycomb::{register_dist_tracing_root, TraceId};
use warp::trace::Info;

pub fn begin_trace(source: &str) -> Span {
    let trace_id = TraceId::new();

    // Create a span using tracing macros
    let span = tracing::info_span!("begin span", source = %source, id = %trace_id);
    span.in_scope(|| {
        let _ = register_dist_tracing_root(trace_id, None).map_err(|e| {
            eprintln!("register trace root error: {:?}", e);
        });
    });
    span
}

pub fn warp_trace(info: Info) -> Span {
    let request_id = TraceId::new();
    let method = info.method();
    let path = info.path();

    // Create a span using tracing macros
    let span = tracing::info_span!(
        "t2server",
        id = %request_id,
        method = %method,
        path = %path,
    );

    span.in_scope(|| {
        if let Err(err) = register_dist_tracing_root(request_id.clone(), None) {
            eprintln!("register trace root error (warp): {:?}", err);
        }
        tracing::info!(method = %method, path = %path);
    });

    span
}
