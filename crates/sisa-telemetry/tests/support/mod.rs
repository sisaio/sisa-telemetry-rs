//! An OpenTelemetry Collector in Docker that prints what it receives, for the OTLP tests.

use std::time::Duration;

use testcontainers::{
    ContainerAsync, GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};

// Receives OTLP over gRPC and HTTP, and prints every span and metric point in full.
const CONFIG: &str = "
receivers:
  otlp:
    protocols:
      grpc:
        endpoint: 0.0.0.0:4317
      http:
        endpoint: 0.0.0.0:4318
exporters:
  debug:
    verbosity: detailed
service:
  pipelines:
    traces:
      receivers: [otlp]
      exporters: [debug]
    metrics:
      receivers: [otlp]
      exporters: [debug]
";

const ATTEMPTS: u32 = 120;
const PAUSE: Duration = Duration::from_millis(250);

/// A running Collector.
pub struct Collector {
    container: ContainerAsync<GenericImage>,
}

impl Collector {
    /// Starts the image in `OTEL_COLLECTOR_IMAGE`, as `name:tag`;
    /// `otel/opentelemetry-collector-contrib:0.149.0` by default.
    pub async fn start() -> Self {
        let named = std::env::var("OTEL_COLLECTOR_IMAGE")
            .unwrap_or_else(|_| "otel/opentelemetry-collector-contrib:0.149.0".to_owned());
        let (name, tag) = named.split_once(':').unwrap_or((named.as_str(), "latest"));

        let container = GenericImage::new(name, tag)
            .with_exposed_port(4317.tcp())
            .with_exposed_port(4318.tcp())
            .with_wait_for(WaitFor::message_on_stderr("Everything is ready"))
            .with_copy_to("/etc/collector.yaml", CONFIG.as_bytes().to_vec())
            .with_cmd(["--config=/etc/collector.yaml"])
            .start()
            .await
            .expect("Docker must be running to start a Collector for the tests");

        Self { container }
    }

    /// The URL of one of the Collector's receivers on this host: port 4317 is gRPC, 4318 HTTP.
    pub async fn endpoint(&self, port: u16) -> String {
        let host_port = self.container.get_host_port_ipv4(port).await;
        let host_port = host_port.expect("the container exposes its ports");

        // The Collector says it is ready a moment before Docker forwards the port.
        for _ in 0..ATTEMPTS {
            if tokio::net::TcpStream::connect(("127.0.0.1", host_port))
                .await
                .is_ok()
            {
                return format!("http://127.0.0.1:{host_port}");
            }
            tokio::time::sleep(PAUSE).await;
        }
        panic!("the Collector never accepted a connection on port {port}");
    }

    /// Waits until the Collector has printed every one of `expected`, and panics with its output
    /// if it never does.
    pub async fn received(&self, expected: &[&str]) {
        let mut output = String::new();
        for _ in 0..ATTEMPTS {
            let printed = self.container.stderr_to_vec().await;
            let printed = printed.expect("the container's output can be read");
            output = String::from_utf8_lossy(&printed).into_owned();
            if expected.iter().all(|each| output.contains(each)) {
                return;
            }
            tokio::time::sleep(PAUSE).await;
        }
        panic!("the Collector never received {expected:?}; it printed:\n{output}");
    }
}
