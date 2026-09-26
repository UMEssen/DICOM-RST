//! Structured access-audit logging.
//!
//! When enabled (`telemetry.audit.enabled: true`), every HTTP request emits
//! one self-contained JSON line on stdout describing WHO accessed WHAT:
//!
//! ```json
//! {"audit":"http-access","ts":"2026-08-17T17:16:55Z","user":"jane.doe@example.org",
//!  "subject":"3f2c9a4e-…","source":"192.0.2.10","method":"GET",
//!  "path":"/aets/PACS/studies/1.2.3.4","aet":"PACS",
//!  "study":"1.2.3.4","status":200,"duration_ms":4886}
//! ```
//!
//! Identity is read from the request headers named by
//! `telemetry.audit.user-header` (default `X-Forwarded-Email`) and
//! `telemetry.audit.subject-header` (default `X-Forwarded-User`).
//! DICOM-RST itself performs no authentication (see #15/#42): these fields
//! are TRUSTWORTHY ONLY when an authenticating proxy replaces any client
//! copies of these headers with values from its verified session and is the
//! only way to reach DICOM-RST. Whether a given proxy does so, and the rest
//! of the trust model, is documented in the "Access Audit Config" section
//! of `docs/topics/configuration.md`. A header that occurs more than once is
//! ambiguous and treated as absent, as is a value that is not 1..=320 bytes
//! of UTF-8 without control characters (never truncated: a shortened
//! identity could equal someone else's). The record is emitted regardless —
//! an absent identity is itself audit-relevant.
//!
//! A caller that acts for someone else (e.g. a backend service fetching
//! images for a signed-in user) can name that end user in the header set by
//! `telemetry.audit.on-behalf-of-header` (default `X-On-Behalf-Of`). The
//! claim is recorded as `on_behalf_of` only if the caller's own verified
//! identity (`user`) is listed in `telemetry.audit.trusted-relays` (ASCII
//! case-insensitive), the header occurs exactly once, and its value is
//! 1..=320 bytes of UTF-8 without whitespace or control characters.
//! Otherwise `on_behalf_of_rejected` says why (`"untrusted-caller"` or
//! `"invalid"`) and the claimed value is NOT recorded. With no trusted
//! relays configured (the default) the header is not read at all and
//! neither field ever appears. `on_behalf_of` is an unverifiable claim by an
//! authenticated relay, recorded beside the relay's own identity; it is
//! never used for authorization.
//!
//! `source` is the leftmost `X-Forwarded-For` entry and `request_id` the
//! incoming `X-Request-Id` (1..=128 printable ASCII characters without
//! space, sent once): both are client-asserted unless the proxy chain
//! overwrites them. `request_id` lets a record be correlated with the
//! access log of the proxy or ingress that set it. `path` (8 KiB),
//! `user_agent` (512 bytes), `source` (64 bytes) and each DICOM coordinate
//! (256 bytes) are capped; a cut value ends in `…` and, marker included,
//! stays within its cap. If a path parameter cannot be decoded, the record carries
//! no DICOM coordinates at all; `path` is still recorded.
//!
//! `status` and `duration_ms` are taken when the response head is produced,
//! so a streamed retrieve that fails mid-body is recorded with the status of
//! its head. A client that disconnects before the head, or a handler panic,
//! can leave no record.
//!
//! Delivery is FAIL-OPEN by design: records flow through a bounded channel
//! to a dedicated writer thread (not a Tokio task, so a stalled stdout never
//! ties up a runtime worker); when the buffer is full the record is dropped
//! and counted, and a warning with the running count is logged for the first
//! drop and every 100th — a slow disk or collector never blocks request
//! handling. Deployments with stricter requirements should alert on the drop
//! warnings. After a graceful shutdown, [`AuditWriter::finish`] waits a
//! bounded time for buffered records to be written; without a graceful
//! shutdown they are lost.

use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use axum::extract::{RawPathParams, Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;
use axum::RequestExt;
use chrono::{SecondsFormat, Utc};
use serde::Serialize;
use tokio::sync::mpsc;
use tracing::warn;

use crate::config::AuditConfig;

/// One audit record per HTTP request.
#[derive(Debug, Serialize)]
pub struct AuditRecord {
	/// Discriminator for log pipelines; always `"http-access"` for now.
	pub audit: &'static str,
	/// Wall-clock request completion time (UTC, RFC 3339, second precision).
	pub ts: String,
	/// Proxy-verified user from `telemetry.audit.user-header`, if present.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub user: Option<String>,
	/// Proxy-verified subject from `telemetry.audit.subject-header`, if present.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub subject: Option<String>,
	/// End user named by a trusted relay (see the module docs).
	#[serde(skip_serializing_if = "Option::is_none")]
	pub on_behalf_of: Option<String>,
	/// Why an on-behalf-of header was ignored. The ignored value itself is
	/// never recorded: it came from a caller that may not make the claim.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub on_behalf_of_rejected: Option<OnBehalfOfRejection>,
	/// First `X-Forwarded-For` entry, if present (at most 64 bytes, marker
	/// included).
	#[serde(skip_serializing_if = "Option::is_none")]
	pub source: Option<String>,
	pub method: String,
	/// Full request path and query (at most 8 KiB, marker included). QIDO
	/// match parameters are part of "which data was accessed" and are
	/// deliberately included.
	pub path: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub aet: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub study: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub series: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub instance: Option<String>,
	pub status: u16,
	pub duration_ms: u128,
	/// `User-Agent`, if present (at most 512 bytes, marker included).
	#[serde(skip_serializing_if = "Option::is_none")]
	pub user_agent: Option<String>,
	/// `X-Request-Id`, if sent once as 1..=128 printable ASCII characters
	/// without space.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub request_id: Option<String>,
}

/// Why an on-behalf-of header was not honoured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OnBehalfOfRejection {
	/// The caller is unidentified or not a configured trusted relay.
	UntrustedCaller,
	/// A trusted relay sent the header more than once or a malformed value.
	Invalid,
}

/// Outcome of evaluating the on-behalf-of header for one request.
#[derive(Debug)]
enum Delegation {
	/// Nothing to decide: no trusted relays configured, or no header sent.
	NotClaimed,
	Honoured(String),
	Rejected(OnBehalfOfRejection),
}

impl Delegation {
	fn into_fields(self) -> (Option<String>, Option<OnBehalfOfRejection>) {
		match self {
			Self::NotClaimed => (None, None),
			Self::Honoured(end_user) => (Some(end_user), None),
			Self::Rejected(reason) => (None, Some(reason)),
		}
	}
}

/// Longest accepted identity: room for the longest e-mail address
/// (64-octet local part, `@`, 255-octet domain).
const MAX_IDENTITY_LEN: usize = 320;

const X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");
const MAX_REQUEST_ID_LEN: usize = 128;

// Caps on values copied from the request as-is, so that one oversized
// request cannot produce an oversized audit line. Longer values are cut at
// a character boundary and end in `TRUNCATED`, the marker included in the
// cap.
const MAX_PATH_LEN: usize = 8 * 1024;
const MAX_USER_AGENT_LEN: usize = 512;
const MAX_SOURCE_LEN: usize = 64;
/// Per DICOM coordinate (`aet`, `study`, `series`, `instance`): far above
/// any valid AE title (16) or UID (64), so only garbage is ever cut.
const MAX_COORDINATE_LEN: usize = 256;
const TRUNCATED: &str = "…";

/// Cloneable handle to the audit writer. Only exists while auditing is
/// enabled.
#[derive(Clone)]
pub struct AuditSink {
	tx: mpsc::Sender<AuditRecord>,
	config: Arc<AuditConfig>,
}

/// Records dropped because the buffer was full (fail-open pressure valve).
static DROPPED: AtomicU64 = AtomicU64::new(0);

const BUFFER: usize = 1024;

/// Start the stdout writer thread and return a sink feeding it, or `None`
/// when auditing is disabled. Without a sink the middleware must not be
/// installed at all, so the request path is exactly that of a build without
/// auditing.
///
/// # Errors
/// Returns an error if the writer thread cannot be spawned.
pub fn start(config: &AuditConfig) -> std::io::Result<Option<(AuditSink, AuditWriter)>> {
	if !config.enabled {
		return Ok(None);
	}
	let (tx, rx) = mpsc::channel::<AuditRecord>(BUFFER);
	// `Stdout::write_all` takes the lock once per call, which keeps each
	// line atomic alongside the regular tracing output on the same stream.
	let writer = spawn_writer(rx, std::io::stdout())?;
	let sink = AuditSink {
		tx,
		config: Arc::new(config.clone()),
	};
	Ok(Some((sink, writer)))
}

/// Handle to the thread that writes audit records.
pub struct AuditWriter {
	thread: JoinHandle<()>,
}

impl AuditWriter {
	/// Waits at most `timeout` for the writer to finish, which it does once
	/// every [`AuditSink`] clone is dropped and all buffered records are
	/// written. Returns whether it finished cleanly.
	pub fn finish(self, timeout: Duration) -> bool {
		let deadline = Instant::now() + timeout;
		while !self.thread.is_finished() {
			if Instant::now() >= deadline {
				return false;
			}
			std::thread::sleep(Duration::from_millis(10));
		}
		self.thread.join().is_ok()
	}
}

fn spawn_writer<W>(mut rx: mpsc::Receiver<AuditRecord>, mut out: W) -> std::io::Result<AuditWriter>
where
	W: Write + Send + 'static,
{
	let thread = std::thread::Builder::new()
		.name("audit-writer".to_owned())
		.spawn(move || {
			while let Some(record) = rx.blocking_recv() {
				match serde_json::to_string(&record) {
					Ok(mut line) => {
						line.push('\n');
						let _ = out.write_all(line.as_bytes());
					}
					Err(err) => warn!("failed to serialize audit record: {err}"),
				}
			}
			let _ = out.flush();
		})?;
	Ok(AuditWriter { thread })
}

impl AuditSink {
	/// An enabled sink whose records are handed to the caller instead of
	/// being written to stdout.
	#[cfg(test)]
	fn with_receiver(config: AuditConfig) -> (Self, mpsc::Receiver<AuditRecord>) {
		let (tx, rx) = mpsc::channel(BUFFER);
		let sink = Self {
			tx,
			config: Arc::new(config),
		};
		(sink, rx)
	}

	fn emit(&self, record: AuditRecord) {
		if self.tx.try_send(record).is_err() {
			let dropped = DROPPED.fetch_add(1, Ordering::Relaxed) + 1;
			// Every drop is a warning-worthy event, but do not spam a
			// saturated system: log the first and then every 100th.
			if dropped == 1 || dropped.is_multiple_of(100) {
				warn!("audit buffer full: {dropped} record(s) dropped so far (fail-open)");
			}
		}
	}
}

/// Axum middleware producing one [`AuditRecord`] per request.
///
/// Attach with `axum::middleware::from_fn_with_state(sink, audit::middleware)`
/// OUTSIDE the timeout layer, so timed-out requests are recorded with their
/// 408 as well.
///
/// The middleware only observes: it never answers a request itself. Path
/// parameters are therefore read without a rejecting extractor — a request
/// whose parameters cannot be decoded is passed on unchanged and still
/// audited, with its path but without any of the DICOM coordinates.
pub async fn middleware(
	State(sink): State<AuditSink>,
	mut request: Request,
	next: Next,
) -> Response {
	let params = request.extract_parts::<RawPathParams>().await.ok();

	let mut aet = None;
	let mut study = None;
	let mut series = None;
	let mut instance = None;
	for (name, value) in params.iter().flatten() {
		match name {
			"aet" => aet = Some(bounded(value.to_owned(), MAX_COORDINATE_LEN)),
			"study" => study = Some(bounded(value.to_owned(), MAX_COORDINATE_LEN)),
			"series" => series = Some(bounded(value.to_owned(), MAX_COORDINATE_LEN)),
			"instance" => instance = Some(bounded(value.to_owned(), MAX_COORDINATE_LEN)),
			_ => {}
		}
	}

	// Extract everything BEFORE the await, inside a block that ends first:
	// a closure borrowing `&Request` held across `next.run().await` makes
	// the future `!Send` (axum's `Body` is `!Sync`), failing the middleware
	// `Service` bound with a famously opaque error.
	let (user, subject, delegation, source, user_agent, request_id) = {
		let headers = request.headers();
		let get = |name: &str| {
			headers
				.get(name)
				.and_then(|value| value.to_str().ok())
				.map(str::to_owned)
		};
		let user = identity(headers, &sink.config.user_header);
		let delegation = delegation_for(&sink.config, headers, user.as_deref());
		(
			user,
			identity(headers, &sink.config.subject_header),
			delegation,
			get("x-forwarded-for").map(|forwarded| {
				let first = forwarded.split(',').next().unwrap_or_default();
				bounded(first.trim().to_owned(), MAX_SOURCE_LEN)
			}),
			get("user-agent").map(|user_agent| bounded(user_agent, MAX_USER_AGENT_LEN)),
			request_id(headers),
		)
	};
	let method = request.method().to_string();
	let path = bounded(request.uri().to_string(), MAX_PATH_LEN);

	let (on_behalf_of, on_behalf_of_rejected) = delegation.into_fields();

	let started = std::time::Instant::now();
	let response = next.run(request).await;

	sink.emit(AuditRecord {
		audit: "http-access",
		ts: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
		user,
		subject,
		on_behalf_of,
		on_behalf_of_rejected,
		source,
		method,
		path,
		aet,
		study,
		series,
		instance,
		status: response.status().as_u16(),
		duration_ms: started.elapsed().as_millis(),
		user_agent,
		request_id,
	});

	response
}

/// `value` if it fits in `max` bytes; otherwise cut at a character boundary
/// and followed by [`TRUNCATED`], the whole at most `max` bytes (every cap
/// here is far larger than the marker).
fn bounded(mut value: String, max: usize) -> String {
	if value.len() > max {
		let mut end = max.saturating_sub(TRUNCATED.len());
		while !value.is_char_boundary(end) {
			end -= 1;
		}
		value.truncate(end);
		value.push_str(TRUNCATED);
	}
	value
}

/// The value of `name`, if the header occurs exactly once: a repeated header
/// is ambiguous and must not decide who the caller is.
fn single<'h>(headers: &'h HeaderMap, name: &HeaderName) -> Option<&'h HeaderValue> {
	let mut values = headers.get_all(name).iter();
	let value = values.next()?;
	values.next().is_none().then_some(value)
}

/// A proxy-asserted identity: the header occurs exactly once and its value
/// is an [`identity_value`].
fn identity(headers: &HeaderMap, name: &HeaderName) -> Option<String> {
	single(headers, name)
		.and_then(identity_value)
		.map(str::to_owned)
}

/// 1..=320 bytes of UTF-8 without control characters. Anything else is
/// rejected as a whole, never truncated: a shortened identity could equal
/// someone else's.
fn identity_value(value: &HeaderValue) -> Option<&str> {
	let value = std::str::from_utf8(value.as_bytes()).ok()?;
	let well_formed =
		(1..=MAX_IDENTITY_LEN).contains(&value.len()) && !value.chars().any(char::is_control);
	well_formed.then_some(value)
}

/// Decides whether `caller` (the verified `user`) may name the end user it
/// acts for. See the module docs for the rules.
fn delegation_for(config: &AuditConfig, headers: &HeaderMap, caller: Option<&str>) -> Delegation {
	// Without trusted relays the header is not even looked at.
	if config.trusted_relays.is_empty() {
		return Delegation::NotClaimed;
	}
	let mut values = headers.get_all(&config.on_behalf_of_header).iter();
	let Some(value) = values.next() else {
		return Delegation::NotClaimed;
	};
	if !caller.is_some_and(|caller| config.trusted_relays.contains(caller)) {
		return Delegation::Rejected(OnBehalfOfRejection::UntrustedCaller);
	}
	if values.next().is_some() {
		return Delegation::Rejected(OnBehalfOfRejection::Invalid);
	}
	end_user(value).map_or(
		Delegation::Rejected(OnBehalfOfRejection::Invalid),
		Delegation::Honoured,
	)
}

/// An end-user identity as named by a trusted relay: an [`identity_value`]
/// that also contains no whitespace.
fn end_user(value: &HeaderValue) -> Option<String> {
	identity_value(value)
		.filter(|value| !value.chars().any(char::is_whitespace))
		.map(str::to_owned)
}

/// A correlation id: exactly one value of 1..=128 printable ASCII
/// characters without space.
fn request_id(headers: &HeaderMap) -> Option<String> {
	single(headers, &X_REQUEST_ID)
		.map(HeaderValue::as_bytes)
		.filter(|id| (1..=MAX_REQUEST_ID_LEN).contains(&id.len()))
		.filter(|id| id.iter().all(u8::is_ascii_graphic))
		.and_then(|id| std::str::from_utf8(id).ok())
		.map(str::to_owned)
}

#[cfg(test)]
mod tests {
	use super::*;
	use axum::body::Body;
	use axum::routing::get;
	use axum::Router;
	use serde_json::json;
	use tower::ServiceExt;

	const RELAY: &str = "relay@example.org";
	const END_USER: &str = "jane.doe@example.org";

	fn enabled() -> AuditConfig {
		AuditConfig {
			enabled: true,
			..AuditConfig::default()
		}
	}

	/// Parsed like the real configuration, so relays are normalised.
	fn with_relays(relays: &[&str]) -> AuditConfig {
		serde_json::from_value(json!({ "enabled": true, "trusted-relays": relays }))
			.expect("valid audit config")
	}

	fn from_relay() -> axum::http::request::Builder {
		get_study().header("x-forwarded-email", RELAY)
	}

	/// The record as a JSON object, minus the fields that vary between runs.
	fn stable_json(record: &AuditRecord) -> serde_json::Value {
		let mut json = serde_json::to_value(record).expect("serialize");
		let object = json.as_object_mut().expect("object");
		object.remove("ts");
		object.remove("duration_ms");
		json
	}

	fn app() -> Router {
		Router::new().route("/aets/{aet}/studies/{study}", get(|| async { "ok" }))
	}

	/// Sends one request through a router carrying the audit middleware and
	/// returns the record it produced, if any.
	async fn audit(config: AuditConfig, request: Request) -> Option<AuditRecord> {
		let (sink, mut rx) = AuditSink::with_receiver(config);
		let app = app().layer(axum::middleware::from_fn_with_state(sink, middleware));
		let response = app.oneshot(request).await.expect("infallible");
		assert_eq!(response.status(), 200);
		rx.try_recv().ok()
	}

	fn get_study() -> axum::http::request::Builder {
		Request::get("/aets/PACS/studies/1.2.3.4")
	}

	#[tokio::test]
	async fn records_identity_from_forwarded_headers() {
		let request = get_study()
			.header("x-forwarded-email", "jane.doe@example.org")
			.header("x-forwarded-user", "3f2c9a4e")
			.body(Body::empty())
			.expect("request");
		let record = audit(enabled(), request).await.expect("record");
		assert_eq!(record.user.as_deref(), Some("jane.doe@example.org"));
		assert_eq!(record.subject.as_deref(), Some("3f2c9a4e"));
		assert_eq!(record.aet.as_deref(), Some("PACS"));
		assert_eq!(record.study.as_deref(), Some("1.2.3.4"));
	}

	#[tokio::test]
	async fn ignores_x_auth_request_headers() {
		// Response headers in oauth2-proxy's reverse-proxy mode: never set on
		// the upstream request and never stripped from it, so a client can
		// send them at will.
		let request = get_study()
			.header("x-auth-request-email", "forged@example.com")
			.header("x-auth-request-user", "forged")
			.body(Body::empty())
			.expect("request");
		let record = audit(enabled(), request).await.expect("record");
		assert_eq!(record.user, None);
		assert_eq!(record.subject, None);
	}

	#[tokio::test]
	async fn identity_header_is_configurable() {
		let config = AuditConfig {
			user_header: HeaderName::from_static("x-forwarded-preferred-username"),
			..enabled()
		};
		let request = get_study()
			.header("x-forwarded-preferred-username", "jdoe")
			.header("x-forwarded-email", "jane.doe@example.org")
			.body(Body::empty())
			.expect("request");
		let record = audit(config, request).await.expect("record");
		assert_eq!(record.user.as_deref(), Some("jdoe"));
	}

	#[tokio::test]
	async fn records_utf8_identity() {
		let request = get_study()
			.header(
				"x-forwarded-email",
				HeaderValue::from_bytes("jürgen.müller@example.org".as_bytes()).expect("header"),
			)
			.header(
				"x-forwarded-user",
				HeaderValue::from_bytes("Jürgen Müller".as_bytes()).expect("header"),
			)
			.body(Body::empty())
			.expect("request");
		let record = audit(enabled(), request).await.expect("record");
		assert_eq!(record.user.as_deref(), Some("jürgen.müller@example.org"));
		assert_eq!(record.subject.as_deref(), Some("Jürgen Müller"));
	}

	#[tokio::test]
	async fn malformed_identity_is_absent_not_truncated() {
		let longest = "a".repeat(MAX_IDENTITY_LEN);
		let request = get_study()
			.header("x-forwarded-email", longest.as_str())
			.body(Body::empty())
			.expect("request");
		let record = audit(enabled(), request).await.expect("record");
		assert_eq!(record.user.as_deref(), Some(longest.as_str()));

		let too_long = "a".repeat(MAX_IDENTITY_LEN + 1);
		let malformed: [&[u8]; 4] = [
			b"",
			too_long.as_bytes(),
			b"jane\tdoe@example.org",
			b"jane\xFFdoe@example.org",
		];
		for value in malformed {
			let request = get_study()
				.header(
					"x-forwarded-email",
					HeaderValue::from_bytes(value).expect("header"),
				)
				.body(Body::empty())
				.expect("request");
			let record = audit(enabled(), request).await.expect("record");
			assert_eq!(record.user, None, "{value:?}");
		}
	}

	#[tokio::test]
	async fn repeated_identity_header_is_treated_as_absent() {
		let request = get_study()
			.header("x-forwarded-email", "jane.doe@example.org")
			.header("x-forwarded-email", "john.doe@example.org")
			.body(Body::empty())
			.expect("request");
		let record = audit(enabled(), request).await.expect("record");
		assert_eq!(record.user, None);
	}

	#[test]
	fn disabled_audit_has_no_sink() {
		// No sink means no writer thread and no middleware: nothing is emitted.
		assert!(start(&AuditConfig::default()).expect("start").is_none());
	}

	/// An in-memory `Write` target shared with the writer thread.
	#[derive(Clone, Default)]
	struct SharedBuffer(Arc<std::sync::Mutex<Vec<u8>>>);

	impl Write for SharedBuffer {
		fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
			self.0.lock().expect("lock").extend_from_slice(buf);
			Ok(buf.len())
		}

		fn flush(&mut self) -> std::io::Result<()> {
			Ok(())
		}
	}

	fn sample_record() -> AuditRecord {
		AuditRecord {
			audit: "http-access",
			ts: "2026-08-17T00:00:00Z".to_owned(),
			user: Some(END_USER.to_owned()),
			subject: None,
			on_behalf_of: None,
			on_behalf_of_rejected: None,
			source: None,
			method: "GET".to_owned(),
			path: "/aets".to_owned(),
			aet: None,
			study: None,
			series: None,
			instance: None,
			status: 200,
			duration_ms: 3,
			user_agent: None,
			request_id: None,
		}
	}

	#[test]
	fn writer_thread_flushes_buffered_records_on_finish() {
		// A plain #[test]: the writer must not need a Tokio runtime.
		let (tx, rx) = mpsc::channel(BUFFER);
		let out = SharedBuffer::default();
		let writer = spawn_writer(rx, out.clone()).expect("writer thread");
		for _ in 0..3 {
			tx.try_send(sample_record()).expect("buffer has room");
		}
		drop(tx);
		assert!(writer.finish(Duration::from_secs(5)));

		let written = String::from_utf8(out.0.lock().expect("lock").clone()).expect("UTF-8");
		assert_eq!(written.lines().count(), 3, "{written}");
		for line in written.lines() {
			let json: serde_json::Value = serde_json::from_str(line).expect("JSON line");
			assert_eq!(json["audit"], "http-access");
		}
	}

	#[test]
	fn writer_finish_is_bounded() {
		let (tx, rx) = mpsc::channel::<AuditRecord>(BUFFER);
		let writer = spawn_writer(rx, SharedBuffer::default()).expect("writer thread");
		// A sink that outlives the deadline keeps the writer running.
		let late_drop = std::thread::spawn(move || {
			std::thread::sleep(Duration::from_millis(500));
			drop(tx);
		});
		assert!(!writer.finish(Duration::from_millis(50)));
		late_drop.join().expect("dropper");
	}

	#[tokio::test]
	async fn undecodable_path_parameter_passes_through_and_is_audited() {
		let request = || {
			Request::get("/aets/%FF/studies/1.2.3.4")
				.body(Body::empty())
				.expect("request")
		};
		let unaudited = app().oneshot(request()).await.expect("infallible");
		assert_eq!(unaudited.status(), 200);

		let record = audit(enabled(), request()).await.expect("record");
		assert_eq!(record.status, 200);
		assert_eq!(record.aet, None);
		assert_eq!(record.study, None);
	}

	#[tokio::test]
	async fn trusted_relay_names_end_user() {
		let request = from_relay()
			.header("x-on-behalf-of", END_USER)
			.body(Body::empty())
			.expect("request");
		let record = audit(with_relays(&[RELAY]), request).await.expect("record");
		assert_eq!(record.user.as_deref(), Some(RELAY));
		assert_eq!(record.on_behalf_of.as_deref(), Some(END_USER));
		assert_eq!(record.on_behalf_of_rejected, None);
	}

	#[tokio::test]
	async fn trusted_relay_without_on_behalf_of_header_records_neither_field() {
		let request = from_relay().body(Body::empty()).expect("request");
		let record = audit(with_relays(&[RELAY]), request).await.expect("record");
		assert_eq!(record.user.as_deref(), Some(RELAY));
		assert_eq!(record.on_behalf_of, None);
		assert_eq!(record.on_behalf_of_rejected, None);
		let line = serde_json::to_string(&record).expect("serialize");
		assert!(!line.contains("on_behalf_of"), "{line}");
	}

	#[tokio::test]
	async fn relay_is_matched_through_a_custom_user_header() {
		let config: AuditConfig = serde_json::from_value(json!({
			"enabled": true,
			"user-header": "X-Forwarded-Preferred-Username",
			"trusted-relays": ["viewer-service"],
		}))
		.expect("valid audit config");

		let request = get_study()
			.header("x-forwarded-preferred-username", "viewer-service")
			.header("x-forwarded-email", "other@example.org")
			.header("x-on-behalf-of", END_USER)
			.body(Body::empty())
			.expect("request");
		let record = audit(config.clone(), request).await.expect("record");
		assert_eq!(record.user.as_deref(), Some("viewer-service"));
		assert_eq!(record.on_behalf_of.as_deref(), Some(END_USER));

		// The relay name in the default user header does not count.
		let request = get_study()
			.header("x-forwarded-preferred-username", "jdoe")
			.header("x-forwarded-email", "viewer-service")
			.header("x-on-behalf-of", END_USER)
			.body(Body::empty())
			.expect("request");
		let record = audit(config, request).await.expect("record");
		assert_eq!(record.on_behalf_of, None);
		assert_eq!(
			record.on_behalf_of_rejected,
			Some(OnBehalfOfRejection::UntrustedCaller)
		);
	}

	#[tokio::test]
	async fn records_408_from_a_timeout_layer_inside_the_audit_layer() {
		use axum::http::StatusCode;
		use tower_http::timeout::TimeoutLayer;

		let (sink, mut rx) = AuditSink::with_receiver(enabled());
		let slow = || async {
			tokio::time::sleep(Duration::from_secs(30)).await;
			"late"
		};
		let app = Router::new()
			.route("/aets/{aet}/studies/{study}", get(slow))
			.layer(TimeoutLayer::with_status_code(
				StatusCode::REQUEST_TIMEOUT,
				Duration::from_millis(20),
			))
			.layer(axum::middleware::from_fn_with_state(sink, middleware));
		let request = get_study().body(Body::empty()).expect("request");
		let response = app.oneshot(request).await.expect("infallible");
		assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);

		let record = rx.try_recv().expect("record");
		assert_eq!(record.status, 408);
		assert_eq!(record.study.as_deref(), Some("1.2.3.4"));
	}

	#[tokio::test]
	async fn relay_match_is_ascii_case_insensitive() {
		let request = get_study()
			.header("x-forwarded-email", "RELAY@Example.ORG")
			.header("x-on-behalf-of", END_USER)
			.body(Body::empty())
			.expect("request");
		let record = audit(with_relays(&["Relay@example.org"]), request)
			.await
			.expect("record");
		assert_eq!(record.on_behalf_of.as_deref(), Some(END_USER));
	}

	#[tokio::test]
	async fn untrusted_caller_is_rejected_without_recording_the_claim() {
		let request = get_study()
			.header("x-forwarded-email", "mallory@example.com")
			.header("x-on-behalf-of", END_USER)
			.body(Body::empty())
			.expect("request");
		let record = audit(with_relays(&[RELAY]), request).await.expect("record");
		assert_eq!(record.user.as_deref(), Some("mallory@example.com"));
		assert_eq!(record.on_behalf_of, None);
		assert_eq!(
			record.on_behalf_of_rejected,
			Some(OnBehalfOfRejection::UntrustedCaller)
		);
		let line = serde_json::to_string(&record).expect("serialize");
		assert!(!line.contains(END_USER), "claimed value leaked: {line}");
		assert!(line.contains(r#""on_behalf_of_rejected":"untrusted-caller""#));
	}

	#[tokio::test]
	async fn unidentified_caller_is_rejected() {
		let request = get_study()
			.header("x-on-behalf-of", END_USER)
			.body(Body::empty())
			.expect("request");
		let record = audit(with_relays(&[RELAY]), request).await.expect("record");
		assert_eq!(record.user, None);
		assert_eq!(record.on_behalf_of, None);
		assert_eq!(
			record.on_behalf_of_rejected,
			Some(OnBehalfOfRejection::UntrustedCaller)
		);
	}

	#[tokio::test]
	async fn relay_identity_must_come_from_the_user_header() {
		// The relay's address in any other header (here the subject header)
		// does not make the caller a relay.
		let request = get_study()
			.header("x-forwarded-user", RELAY)
			.header("x-on-behalf-of", END_USER)
			.body(Body::empty())
			.expect("request");
		let record = audit(with_relays(&[RELAY]), request).await.expect("record");
		assert_eq!(
			record.on_behalf_of_rejected,
			Some(OnBehalfOfRejection::UntrustedCaller)
		);
	}

	#[tokio::test]
	async fn without_trusted_relays_the_header_changes_nothing() {
		let plain = from_relay().body(Body::empty()).expect("request");
		let claimed = from_relay()
			.header("x-on-behalf-of", END_USER)
			.body(Body::empty())
			.expect("request");
		let plain = audit(enabled(), plain).await.expect("record");
		let claimed = audit(enabled(), claimed).await.expect("record");
		assert_eq!(stable_json(&claimed), stable_json(&plain));
		let line = serde_json::to_string(&claimed).expect("serialize");
		assert!(!line.contains("on_behalf_of"), "{line}");
	}

	#[tokio::test]
	async fn repeated_on_behalf_of_header_is_invalid() {
		let request = from_relay()
			.header("x-on-behalf-of", END_USER)
			.header("x-on-behalf-of", "john.doe@example.org")
			.body(Body::empty())
			.expect("request");
		let record = audit(with_relays(&[RELAY]), request).await.expect("record");
		assert_eq!(record.on_behalf_of, None);
		assert_eq!(
			record.on_behalf_of_rejected,
			Some(OnBehalfOfRejection::Invalid)
		);
	}

	#[tokio::test]
	async fn honoured_value_is_json_escaped_in_the_audit_line() {
		// A trusted relay's value is recorded verbatim, but as data inside
		// the JSON line: quotes and backslashes can neither close the field
		// nor forge another key.
		let forged = r#"a"b\c","audit":"forged"#;
		let request = from_relay()
			.header("x-on-behalf-of", forged)
			.body(Body::empty())
			.expect("request");
		let record = audit(with_relays(&[RELAY]), request).await.expect("record");
		assert_eq!(record.on_behalf_of.as_deref(), Some(forged));
		let line = serde_json::to_string(&record).expect("serialize");
		let parsed: serde_json::Value = serde_json::from_str(&line).expect("one JSON object");
		assert_eq!(parsed["audit"], "http-access", "{line}");
		assert_eq!(parsed["on_behalf_of"], forged, "{line}");
	}

	#[tokio::test]
	async fn malformed_on_behalf_of_values_are_invalid() {
		let longest = "a".repeat(MAX_IDENTITY_LEN);
		let too_long = "a".repeat(MAX_IDENTITY_LEN + 1);
		let malformed: [&[u8]; 6] = [
			b"",
			too_long.as_bytes(),
			b"jane doe@example.org",
			b"jane\tdoe@example.org",
			"jane\u{85}doe@example.org".as_bytes(),
			b"jane\xFFdoe@example.org",
		];
		for value in malformed {
			let request = from_relay()
				.header(
					"x-on-behalf-of",
					HeaderValue::from_bytes(value).expect("header"),
				)
				.body(Body::empty())
				.expect("request");
			let record = audit(with_relays(&[RELAY]), request).await.expect("record");
			assert_eq!(record.on_behalf_of, None, "{value:?}");
			assert_eq!(
				record.on_behalf_of_rejected,
				Some(OnBehalfOfRejection::Invalid),
				"{value:?}"
			);
		}

		for value in [longest.as_str(), "jürgen@example.org"] {
			let request = from_relay()
				.header(
					"x-on-behalf-of",
					HeaderValue::from_bytes(value.as_bytes()).expect("header"),
				)
				.body(Body::empty())
				.expect("request");
			let record = audit(with_relays(&[RELAY]), request).await.expect("record");
			assert_eq!(record.on_behalf_of.as_deref(), Some(value));
		}
	}

	#[tokio::test]
	async fn records_well_formed_request_id() {
		let longest = "f".repeat(MAX_REQUEST_ID_LEN);
		for id in ["0f8c2b7e-5d1a-4c3b-9e6f-2a1d0c9b8a7e", longest.as_str()] {
			let request = get_study()
				.header("x-request-id", id)
				.body(Body::empty())
				.expect("request");
			let record = audit(enabled(), request).await.expect("record");
			assert_eq!(record.request_id.as_deref(), Some(id));
		}
	}

	#[tokio::test]
	async fn omits_malformed_request_id() {
		let too_long = "f".repeat(MAX_REQUEST_ID_LEN + 1);
		let malformed: [&[&[u8]]; 5] = [
			&[b""],
			&[too_long.as_bytes()],
			&[b"abc def"],
			&["abc\u{e9}".as_bytes()],
			&[b"abc", b"def"],
		];
		for values in malformed {
			let mut request = get_study();
			for value in values {
				request = request.header(
					"x-request-id",
					HeaderValue::from_bytes(value).expect("header"),
				);
			}
			let request = request.body(Body::empty()).expect("request");
			let record = audit(enabled(), request).await.expect("record");
			assert_eq!(record.request_id, None, "{values:?}");
		}
	}

	#[test]
	fn bounded_cuts_at_a_character_boundary() {
		assert_eq!(bounded("abcde".to_owned(), 5), "abcde");
		// The 3-byte marker counts against the cap.
		assert_eq!(bounded("abcdef".to_owned(), 5), "ab…");
		// "ü" is two bytes: cutting at 2 would split it.
		assert_eq!(bounded("aüüb".to_owned(), 5), "a…");
		assert_eq!(bounded("aüüb".to_owned(), 6), "aüüb");
	}

	#[test]
	fn bounded_never_exceeds_its_cap() {
		let value = "aüß€x".repeat(10);
		for max in TRUNCATED.len()..value.len() {
			let cut = bounded(value.clone(), max);
			assert!(cut.len() <= max, "{max}: {} bytes", cut.len());
			assert!(cut.ends_with(TRUNCATED), "{max}: {cut}");
		}
	}

	#[tokio::test]
	async fn copied_request_values_are_bounded() {
		let long_study = "1".repeat(MAX_PATH_LEN);
		let long_agent = "a".repeat(MAX_USER_AGENT_LEN + 1);
		let long_source = format!("{}, 192.0.2.10", "f".repeat(MAX_SOURCE_LEN + 1));
		let request = Request::get(format!("/aets/PACS/studies/{long_study}"))
			.header("user-agent", long_agent.as_str())
			.header("x-forwarded-for", long_source.as_str())
			.body(Body::empty())
			.expect("request");
		let record = audit(enabled(), request).await.expect("record");

		let path_prefix = format!("/aets/PACS/studies/{long_study}");
		let keep = |max: usize| max - TRUNCATED.len();
		assert_eq!(
			record.path,
			format!("{}{TRUNCATED}", &path_prefix[..keep(MAX_PATH_LEN)])
		);
		assert_eq!(record.path.len(), MAX_PATH_LEN);
		assert_eq!(
			record.study,
			Some(format!(
				"{}{TRUNCATED}",
				&long_study[..keep(MAX_COORDINATE_LEN)]
			))
		);
		assert_eq!(record.aet.as_deref(), Some("PACS"));
		assert_eq!(
			record.user_agent,
			Some(format!(
				"{}{TRUNCATED}",
				&long_agent[..keep(MAX_USER_AGENT_LEN)]
			))
		);
		assert_eq!(
			record.source,
			Some(format!("{}{TRUNCATED}", "f".repeat(keep(MAX_SOURCE_LEN))))
		);

		let at_cap = "a".repeat(MAX_USER_AGENT_LEN);
		let request = get_study()
			.header("user-agent", at_cap.as_str())
			.header("x-forwarded-for", "192.0.2.10, 198.51.100.7")
			.body(Body::empty())
			.expect("request");
		let record = audit(enabled(), request).await.expect("record");
		assert_eq!(record.user_agent, Some(at_cap));
		assert_eq!(record.source.as_deref(), Some("192.0.2.10"));
		assert_eq!(record.path, "/aets/PACS/studies/1.2.3.4");
	}

	#[test]
	fn record_serializes_without_absent_fields() {
		let record = AuditRecord {
			audit: "http-access",
			ts: "2026-08-17T00:00:00Z".to_owned(),
			user: None,
			subject: None,
			on_behalf_of: None,
			on_behalf_of_rejected: None,
			source: None,
			method: "GET".to_owned(),
			path: "/aets".to_owned(),
			aet: None,
			study: None,
			series: None,
			instance: None,
			status: 200,
			duration_ms: 3,
			user_agent: None,
			request_id: None,
		};
		let json = serde_json::to_string(&record).expect("serialize");
		assert!(
			!json.contains("user"),
			"absent fields must be omitted: {json}"
		);
		assert!(json.contains("\"audit\":\"http-access\""));
	}
}
