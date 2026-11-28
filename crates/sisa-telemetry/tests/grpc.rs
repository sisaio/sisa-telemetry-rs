//! The gRPC client layer: a call through `TraceClientLayer` to a server behind `TraceLayer` is
//! one trace, for gRPC and for Connect.

use std::{
    future::poll_fn,
    task::{Context, Poll},
};

use axum::{Router, body::Bytes, http::HeaderMap, routing::post};
use http_body_util::Full;
use hyper_util::{
    client::legacy::{Client, ResponseFuture, connect::HttpConnector},
    rt::TokioExecutor,
};
use sisa_telemetry::{
    grpc::TraceClientLayer,
    http::TraceLayer,
    opentelemetry::trace::{SpanKind, Status},
    testing,
};
use tower_layer::Layer;
use tower_service::Service;

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

// Sends one call through `TraceClientLayer`, as a tonic channel would.
async fn call(url: String, headers: &[(&'static str, &'static str)]) -> u16 {
    let client = Client::builder(TokioExecutor::new()).build_http::<Full<Bytes>>();
    let mut service = TraceClientLayer::default().layer(client);
    let mut request = axum::http::Request::post(url);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let request = request
        .body(Full::new(Bytes::new()))
        .expect("a valid request");

    poll_fn(|context| service.poll_ready(context))
        .await
        .expect("the client is ready");
    let response = service.call(request).await.expect("the server answers");
    response.status().as_u16()
}

// Adds the origin to a path-only request, below the layer, as a tonic channel does.
struct AddOrigin {
    client: Client<HttpConnector, Full<Bytes>>,
    origin: String,
}

impl Service<axum::http::Request<Full<Bytes>>> for AddOrigin {
    type Response =
        <Client<HttpConnector, Full<Bytes>> as Service<axum::http::Request<Full<Bytes>>>>::Response;
    type Error = hyper_util::client::legacy::Error;
    type Future = ResponseFuture;

    fn poll_ready(&mut self, context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Service::<axum::http::Request<Full<Bytes>>>::poll_ready(&mut self.client, context)
    }

    fn call(&mut self, mut request: axum::http::Request<Full<Bytes>>) -> Self::Future {
        let url = format!("{}{}", self.origin, request.uri());
        *request.uri_mut() = url.parse().expect("a valid URL");
        self.client.request(request)
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_path_only_call_records_the_server_the_layer_names() {
    let captured = testing::capture();
    let base = serve(Router::new().route("/shop.Orders/Ship", post(|| async { "" }))).await;
    let origin = AddOrigin {
        client: Client::builder(TokioExecutor::new()).build_http::<Full<Bytes>>(),
        origin: base,
    };
    let mut service = TraceClientLayer::for_server("orders", 50051).layer(origin);
    let request = axum::http::Request::post("/shop.Orders/Ship")
        .header("content-type", "application/grpc")
        .body(Full::new(Bytes::new()))
        .expect("a valid request");

    poll_fn(|context| service.poll_ready(context))
        .await
        .expect("the client is ready");
    service.call(request).await.expect("the server answers");

    let client = captured
        .spans()
        .into_iter()
        .find(|span| span.name == "shop.Orders/Ship" && span.span_kind == SpanKind::Client)
        .unwrap();
    assert_eq!(
        attribute(&client, "server.address").as_deref(),
        Some("orders")
    );
    assert_eq!(attribute(&client, "server.port").as_deref(), Some("50051"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_grpc_call_continues_on_the_server() {
    let captured = testing::capture();
    let failing = || async {
        let mut headers = HeaderMap::new();
        headers.insert("content-type", "application/grpc".parse().unwrap());
        headers.insert("grpc-status", "5".parse().unwrap());
        headers
    };
    let base = serve(Router::new().route("/shop.Orders/Get", post(failing))).await;

    call(
        format!("{base}/shop.Orders/Get"),
        &[("content-type", "application/grpc")],
    )
    .await;

    let spans = captured.spans();
    let named = |kind: SpanKind| {
        let found = spans
            .iter()
            .find(|span| span.name == "shop.Orders/Get" && span.span_kind == kind);
        found.cloned().unwrap()
    };
    let client = named(SpanKind::Client);
    let server = named(SpanKind::Server);
    assert_eq!(
        server.span_context.trace_id(),
        client.span_context.trace_id()
    );
    assert_eq!(server.parent_span_id, client.span_context.span_id());
    assert_eq!(attribute(&client, "rpc.system").as_deref(), Some("grpc"));
    assert_eq!(
        attribute(&client, "rpc.service").as_deref(),
        Some("shop.Orders")
    );
    assert_eq!(attribute(&client, "rpc.method").as_deref(), Some("Get"));
    assert_eq!(
        attribute(&client, "server.address").as_deref(),
        Some("127.0.0.1")
    );
    // NOT_FOUND is a failed call for the client, and not the server's fault.
    assert_eq!(
        attribute(&client, "rpc.grpc.status_code").as_deref(),
        Some("5")
    );
    assert!(matches!(client.status, Status::Error { .. }));
    assert_eq!(server.status, Status::Unset);
    assert!(captured.metric("rpc.client.call.duration").is_some());
    captured.check_catalog("[]").unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_connect_call_is_named_after_its_method() {
    let captured = testing::capture();
    let base = serve(Router::new().route("/shop.Orders/Find", post(|| async { "{}" }))).await;

    let status = call(
        format!("{base}/shop.Orders/Find"),
        &[
            ("content-type", "application/json"),
            ("connect-protocol-version", "1"),
        ],
    )
    .await;

    assert_eq!(status, 200);
    let client = captured
        .spans()
        .into_iter()
        .find(|span| span.name == "shop.Orders/Find" && span.span_kind == SpanKind::Client)
        .unwrap();
    assert_eq!(
        attribute(&client, "rpc.system").as_deref(),
        Some("connect_rpc")
    );
    assert_eq!(
        attribute(&client, "http.response.status_code").as_deref(),
        Some("200")
    );
    assert_eq!(client.status, Status::Unset);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_connect_error_marks_the_span_whatever_grpc_status_says() {
    let captured = testing::capture();
    let unavailable = || async {
        let mut headers = HeaderMap::new();
        headers.insert("grpc-status", "0".parse().unwrap());
        (axum::http::StatusCode::SERVICE_UNAVAILABLE, headers)
    };
    let base = serve(Router::new().route("/shop.Orders/Cancel", post(unavailable))).await;

    call(
        format!("{base}/shop.Orders/Cancel"),
        &[("content-type", "application/connect+json")],
    )
    .await;

    let client = captured
        .spans()
        .into_iter()
        .find(|span| span.name == "shop.Orders/Cancel" && span.span_kind == SpanKind::Client)
        .unwrap();
    assert!(matches!(client.status, Status::Error { .. }));
    assert_eq!(attribute(&client, "error.type").as_deref(), Some("503"));
}
