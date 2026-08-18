//! Structured access-audit logging.
//!
//! When enabled (`telemetry.audit.enabled: true`), every HTTP request emits
//! one self-contained JSON line on stdout describing WHO accessed WHAT:
//!
//! ```json
//! {"audit":"http-access","ts":"2026-08-17T17:16:55Z","user":"alex@example.com",
//!  "subject":"5939ae08-…","source":"10.244.9.49","method":"GET",
//!  "path":"/app/dicom-rst/aets/GEPACS/studies/1.2.840…","aet":"GEPACS",
//!  "study":"1.2.840…","status":200,"duration_ms":4886}
//! ```
//!
//! Identity is read from the `X-Auth-Request-User` / `X-Auth-Request-Email`
//! headers that an authenticating reverse proxy (e.g. oauth2-proxy with
//! `set_xauthrequest`) injects. DICOM-RST itself performs no authentication
//! (see #15/#42): these fields are TRUSTWORTHY ONLY when the deployment
//! guarantees that the proxy is the sole ingress. The record is emitted
//! regardless — an absent identity is itself audit-relevant.
//!
//! Delivery is FAIL-OPEN by design: records flow through a bounded channel
//! to a writer task; when the buffer is full the record is dropped, a
//! counter increments and a warning is logged — a slow disk or collector
//! never blocks request handling. Deployments with stricter requirements
//! should alert on the drop warnings.

use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::extract::{RawPathParams, Request, State};
use axum::middleware::Next;
use axum::response::Response;
use chrono::{SecondsFormat, Utc};
use serde::Serialize;
use tokio::sync::mpsc;
use tracing::warn;

/// One audit record per HTTP request.
#[derive(Debug, Serialize)]
pub struct AuditRecord {
	/// Discriminator for log pipelines; always `"http-access"` for now.
	pub audit: &'static str,
	/// Wall-clock request completion time (UTC, RFC 3339, second precision).
	pub ts: String,
	/// `X-Auth-Request-Email` from the authenticating proxy, if present.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub user: Option<String>,
	/// `X-Auth-Request-User` (the OIDC subject), if present.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub subject: Option<String>,
	/// First `X-Forwarded-For` entry, if present.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub source: Option<String>,
	pub method: String,
	/// Full request path and query. QIDO match parameters are part of
	/// "which data was accessed" and are deliberately included.
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
	#[serde(skip_serializing_if = "Option::is_none")]
	pub user_agent: Option<String>,
}

/// Cloneable handle to the audit writer. `None` inside means auditing is
/// disabled and the middleware is a no-op.
#[derive(Clone)]
pub struct AuditSink {
	tx: Option<mpsc::Sender<AuditRecord>>,
}

/// Records dropped because the buffer was full (fail-open pressure valve).
static DROPPED: AtomicU64 = AtomicU64::new(0);

const BUFFER: usize = 1024;

impl AuditSink {
	/// Create the sink and, when enabled, spawn the stdout writer task.
	pub fn new(enabled: bool) -> Self {
		if !enabled {
			return Self { tx: None };
		}
		let (tx, mut rx) = mpsc::channel::<AuditRecord>(BUFFER);
		tokio::spawn(async move {
			// One locked write per record keeps lines atomic alongside the
			// regular tracing output on the same stream.
			while let Some(record) = rx.recv().await {
				match serde_json::to_string(&record) {
					Ok(mut line) => {
						line.push('\n');
						let mut stdout = std::io::stdout().lock();
						let _ = stdout.write_all(line.as_bytes());
					}
					Err(err) => warn!("failed to serialize audit record: {err}"),
				}
			}
		});
		Self { tx: Some(tx) }
	}

	fn emit(&self, record: AuditRecord) {
		let Some(tx) = &self.tx else { return };
		if tx.try_send(record).is_err() {
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
pub async fn middleware(
	State(sink): State<AuditSink>,
	params: RawPathParams,
	request: Request,
	next: Next,
) -> Response {
	if sink.tx.is_none() {
		return next.run(request).await;
	}

	let mut aet = None;
	let mut study = None;
	let mut series = None;
	let mut instance = None;
	for (name, value) in &params {
		match name {
			"aet" => aet = Some(value.to_owned()),
			"study" => study = Some(value.to_owned()),
			"series" => series = Some(value.to_owned()),
			"instance" => instance = Some(value.to_owned()),
			_ => {}
		}
	}

	// Extract everything BEFORE the await, inside a block that ends first:
	// a closure borrowing `&Request` held across `next.run().await` makes
	// the future `!Send` (axum's `Body` is `!Sync`), failing the middleware
	// `Service` bound with a famously opaque error.
	let (user, subject, source, user_agent) = {
		let headers = request.headers();
		let get = |name: &str| {
			headers
				.get(name)
				.and_then(|value| value.to_str().ok())
				.map(str::to_owned)
		};
		(
			get("x-auth-request-email"),
			get("x-auth-request-user"),
			get("x-forwarded-for").map(|forwarded| {
				forwarded
					.split(',')
					.next()
					.unwrap_or_default()
					.trim()
					.to_owned()
			}),
			get("user-agent"),
		)
	};
	let method = request.method().to_string();
	let path = request.uri().to_string();

	let started = std::time::Instant::now();
	let response = next.run(request).await;

	sink.emit(AuditRecord {
		audit: "http-access",
		ts: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
		user,
		subject,
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
	});

	response
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn record_serializes_without_absent_fields() {
		let record = AuditRecord {
			audit: "http-access",
			ts: "2026-08-17T00:00:00Z".to_owned(),
			user: None,
			subject: None,
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
		};
		let json = serde_json::to_string(&record).expect("serialize");
		assert!(
			!json.contains("user"),
			"absent fields must be omitted: {json}"
		);
		assert!(json.contains("\"audit\":\"http-access\""));
	}
}
