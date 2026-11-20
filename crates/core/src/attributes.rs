//! The attribute names every sisa crate uses for the same thing, so a request can be followed
//! across crates by one name.
//!
//! Their values are unbounded, so they go on spans and logs, never on metrics.

/// The id of one inbound request.
pub const REQUEST_ID: &str = "sisa.request_id";

/// The id of one business flow, carried from the first request through every message it causes.
/// Each consumed message is a trace of its own, so this is what joins them.
pub const CORRELATION_ID: &str = "sisa.correlation_id";

/// The tenant the work is done for.
pub const TENANT: &str = "sisa.tenant";

/// The kind of actor behind the request, such as a user or a service.
pub const ACTOR_TYPE: &str = "sisa.actor.type";
