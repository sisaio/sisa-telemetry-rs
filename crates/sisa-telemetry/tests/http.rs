//! The HTTP helpers: a reqwest call through `TracingMiddleware` to an axum server behind
//! `TraceLayer` is one trace, and gRPC calls follow the RPC conventions.

use axum::{
    Router,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use reqwest_middleware::{ClientBuilder, reqwest::Client};
use sisa_telemetry::{
    http::{TraceLayer, TracingMiddleware},
    opentelemetry::trace::{SpanKind, Status},
    testing,
};

fn attribute(span: &testing::SpanData, key: &str) -> Option<String> {
    let found = span.attributes.iter().find(|each| each.key.as_str() == key);
    found.map(|each| each.value.as_str().into_owned())
}

// Serves `app` behind `TraceLayer` on a free local port, and returns its base URL.
async fn serve(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a free local port");
    let address = listener.local_addr().expect("the bound address");
    tokio::spawn(async move {
        axum::serve(listener, app.layer(TraceLayer::default()))
            .await
            .expect("the test server runs");
    });
    format!("http://{address}")
}

#[tokio::test(flavor = "multi_thread")]
async fn a_call_through_the_middleware_continues_on_the_server() {
    let captured = testing::capture();
    let base = serve(Router::new().route("/orders/{id}", get(|| async { "{}" }))).await;
    let client = ClientBuilder::new(Client::new())
        .with(TracingMiddleware::default())
        .build();

    let response = client
        .get(format!("{base}/orders/7?token=secret"))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let client_span = captured.span("GET").unwrap();
    let server_span = captured.span("GET /orders/{id}").unwrap();
    assert_eq!(client_span.span_kind, SpanKind::Client);
    assert_eq!(server_span.span_kind, SpanKind::Server);
    assert_eq!(
        server_span.span_context.trace_id(),
        client_span.span_context.trace_id()
    );
    assert_eq!(
        server_span.parent_span_id,
        client_span.span_context.span_id()
    );
    assert_eq!(
        attribute(&client_span, "url.full"),
        Some(format!("{base}/orders/7"))
    );
    assert_eq!(
        attribute(&client_span, "http.response.status_code").as_deref(),
        Some("200")
    );
    assert_eq!(
        attribute(&server_span, "http.route").as_deref(),
        Some("/orders/{id}")
    );
    assert!(captured.metric("http.client.request.duration").is_some());
    let server_duration = captured.metric("http.server.request.duration").unwrap();
    assert_eq!(server_duration.unit, "s");
    // The HTTP metrics are shared conventions, so this crate adds no catalog of its own.
    captured.check_catalog("[]").unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_server_error_marks_the_span() {
    let captured = testing::capture();
    let base =
        serve(Router::new().route("/broken", get(|| async { StatusCode::SERVICE_UNAVAILABLE })))
            .await;

    Client::new()
        .get(format!("{base}/broken"))
        .send()
        .await
        .unwrap();

    let span = captured.span("GET /broken").unwrap();
    assert!(matches!(span.status, Status::Error { .. }));
    assert_eq!(attribute(&span, "error.type").as_deref(), Some("503"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_grpc_call_follows_the_rpc_conventions() {
    let captured = testing::capture();
    // A trailers-only gRPC response: INTERNAL, in the headers of an HTTP 200.
    let failing = || async {
        let mut headers = HeaderMap::new();
        headers.insert("content-type", "application/grpc".parse().unwrap());
        headers.insert("grpc-status", "13".parse().unwrap());
        headers
    };
    let base = serve(Router::new().route("/shop.Orders/Place", post(failing))).await;

    Client::new()
        .post(format!("{base}/shop.Orders/Place"))
        .header("content-type", "application/grpc")
        .send()
        .await
        .unwrap();

    let span = captured.span("shop.Orders/Place").unwrap();
    assert_eq!(attribute(&span, "rpc.system").as_deref(), Some("grpc"));
    assert_eq!(
        attribute(&span, "rpc.service").as_deref(),
        Some("shop.Orders")
    );
    assert_eq!(attribute(&span, "rpc.method").as_deref(), Some("Place"));
    assert_eq!(
        attribute(&span, "rpc.grpc.status_code").as_deref(),
        Some("13")
    );
    assert!(matches!(span.status, Status::Error { .. }));
    assert!(captured.metric("rpc.server.call.duration").is_some());
    // The HTTP metrics are shared conventions, so this crate adds no catalog of its own.
    captured.check_catalog("[]").unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_connect_call_is_named_after_its_method() {
    let captured = testing::capture();
    let base = serve(Router::new().route("/shop.Orders/List", post(|| async { "{}" }))).await;

    Client::new()
        .post(format!("{base}/shop.Orders/List"))
        .header("content-type", "application/json")
        .header("connect-protocol-version", "1")
        .send()
        .await
        .unwrap();

    let span = captured.span("shop.Orders/List").unwrap();
    assert_eq!(
        attribute(&span, "rpc.system").as_deref(),
        Some("connect_rpc")
    );
    assert_eq!(span.status, Status::Unset);
}
