//! What a service is, on every span and metric it sends.

use std::collections::BTreeMap;

use opentelemetry::KeyValue;
use opentelemetry_sdk::{
    Resource,
    resource::{EnvResourceDetector, TelemetryResourceDetector},
};

/// The service's identity: `service.name`, `service.version`, and optionally
/// `service.namespace`, `deployment.environment.name` and extra attributes.
///
/// ```
/// use sisa_telemetry::ServiceResource;
///
/// let resource = ServiceResource::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
///     .with_namespace("shop")
///     .with_environment("staging")
///     .with_attribute("cloud.region", "eu-west-1");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceResource {
    name: &'static str,
    version: &'static str,
    namespace: Option<String>,
    environment: Option<String>,
    attributes: Vec<(String, String)>,
}

impl ServiceResource {
    /// A service by its name and version, usually `env!("CARGO_PKG_NAME")` and
    /// `env!("CARGO_PKG_VERSION")`. `OTEL_SERVICE_NAME` wins over the name.
    pub fn new(name: &'static str, version: &'static str) -> Self {
        Self {
            name,
            version,
            namespace: None,
            environment: None,
            attributes: Vec::new(),
        }
    }

    /// Sets `service.namespace`, the group of services this one belongs to.
    #[must_use]
    pub fn with_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Some(namespace.into());
        self
    }

    /// Sets `deployment.environment.name`, such as `production` or `staging`.
    #[must_use]
    pub fn with_environment(mut self, environment: impl Into<String>) -> Self {
        self.environment = Some(environment.into());
        self
    }

    /// Adds one more attribute, such as the cluster or the region.
    #[must_use]
    pub fn with_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.push((key.into(), value.into()));
        self
    }

    // Later sources win: the code, then the settings, then the environment. `service_name` is
    // `OTEL_SERVICE_NAME`.
    pub(crate) fn build(
        self,
        settings: &BTreeMap<String, String>,
        service_name: Option<String>,
    ) -> Resource {
        let mut attributes = vec![
            KeyValue::new("service.name", self.name),
            KeyValue::new("service.version", self.version),
        ];
        if let Some(namespace) = self.namespace {
            attributes.push(KeyValue::new("service.namespace", namespace));
        }
        if let Some(environment) = self.environment {
            attributes.push(KeyValue::new("deployment.environment.name", environment));
        }
        let pairs = self.attributes.into_iter();
        let pairs = pairs.chain(
            settings
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
        attributes.extend(pairs.map(|(key, value)| KeyValue::new(key, value)));

        let builder = Resource::builder_empty()
            .with_detector(Box::new(TelemetryResourceDetector))
            .with_attributes(attributes)
            .with_detector(Box::new(EnvResourceDetector::new()));
        match service_name {
            Some(name) => builder.with_service_name(name).build(),
            None => builder.build(),
        }
    }
}

#[cfg(test)]
mod tests {
    use opentelemetry::{Key, Value};

    use super::*;

    fn get(resource: &Resource, key: &'static str) -> Option<Value> {
        resource.get(&Key::from_static_str(key))
    }

    #[test]
    fn the_resource_names_the_service() {
        let resource = ServiceResource::new("orders", "1.2.3")
            .with_namespace("shop")
            .with_environment("staging")
            .build(&BTreeMap::new(), None);

        assert_eq!(get(&resource, "service.name"), Some("orders".into()));
        assert_eq!(get(&resource, "service.version"), Some("1.2.3".into()));
        assert_eq!(get(&resource, "service.namespace"), Some("shop".into()));
        assert_eq!(
            get(&resource, "deployment.environment.name"),
            Some("staging".into())
        );
        assert!(get(&resource, "telemetry.sdk.name").is_some());
    }

    #[test]
    fn settings_win_over_code_and_the_environment_name_over_both() {
        let settings = BTreeMap::from([("region".to_owned(), "eu-west-1".to_owned())]);

        let resource = ServiceResource::new("orders", "1.2.3")
            .with_attribute("region", "local")
            .build(&settings, Some("orders-canary".to_owned()));

        assert_eq!(get(&resource, "region"), Some("eu-west-1".into()));
        assert_eq!(get(&resource, "service.name"), Some("orders-canary".into()));
    }
}
