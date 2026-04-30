//! Background worker holding the tonic Channel and the bidi Ingest
//! stream. The UI thread sends [`WorkerCommand`]s via a tokio mpsc;
//! the worker drains the [`EventQueue`], batches into `EventBatch`
//! messages, forwards them through the gRPC stream, and reads acks
//! back to update [`WorkerStats`].
//!
//! Reconnection: if the stream errors (server restart, network
//! blip), the worker drops the channel and re-establishes on the
//! next batch. Events stay in the queue across the gap.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Instant;

use fern_collector_proto::v1 as proto;
use fern_core::telemetry::{OwnedEvent, OwnedProp, OwnedPropValue};
use fern_telemetry::EventQueue;
use tokio::runtime::Runtime;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;
use tonic::metadata::MetadataValue;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Endpoint, Identity};

use crate::config::FernConfig;

#[derive(Debug)]
pub(crate) enum WorkerCommand {
    Record(OwnedEvent),
    Flush(oneshot::Sender<Result<(), String>>),
    Discard,
    Shutdown,
}

#[derive(Debug, Default)]
pub(crate) struct WorkerStats {
    pub accepted: AtomicUsize,
    pub dropped: AtomicUsize,
    pub queued: AtomicUsize,
}

pub(crate) fn spawn_worker(
    config: FernConfig,
    queue: Arc<dyn EventQueue>,
    runtime: Arc<Runtime>,
    stats: Arc<WorkerStats>,
) -> (mpsc::Sender<WorkerCommand>, thread::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel::<WorkerCommand>(64);
    let rt_for_thread = runtime.clone();

    // Spawn a std::thread that hosts a tokio LocalSet-flavored
    // worker via runtime.block_on. Using a dedicated OS thread
    // (rather than tokio::spawn directly) gives us a deterministic
    // shutdown path that joins from the FernAdapter::Drop site.
    let handle = thread::Builder::new()
        .name("fern-collector-client".to_string())
        .spawn(move || {
            rt_for_thread.block_on(worker_loop(config, queue, rx, stats));
        })
        .expect("spawn fern-collector worker thread");

    (tx, handle)
}

async fn worker_loop(
    config: FernConfig,
    queue: Arc<dyn EventQueue>,
    mut rx: mpsc::Receiver<WorkerCommand>,
    stats: Arc<WorkerStats>,
) {
    let mut last_flush = Instant::now();
    let mut next_batch_id: u64 = 1;
    // Lazily-opened tonic channel. Re-established on transport error.
    let mut channel: Option<Channel> = None;

    loop {
        // Wait either for a command or for the flush interval to
        // elapse, whichever comes first.
        let wait = if queue.is_empty() {
            config.flush_interval
        } else {
            config.flush_interval.saturating_sub(last_flush.elapsed())
        };

        let cmd = tokio::time::timeout(wait, rx.recv()).await;

        match cmd {
            Ok(Some(WorkerCommand::Record(ev))) => {
                queue.push(ev);
                stats.queued.store(queue.len(), Ordering::Relaxed);
                if queue.len() >= config.max_batch_size {
                    let _ = drain_once(
                        &config,
                        &queue,
                        &mut channel,
                        &mut next_batch_id,
                        &stats,
                    )
                    .await;
                    last_flush = Instant::now();
                }
            }
            Ok(Some(WorkerCommand::Flush(reply))) => {
                let result = drain_all(
                    &config,
                    &queue,
                    &mut channel,
                    &mut next_batch_id,
                    &stats,
                )
                .await;
                let _ = reply.send(result);
                last_flush = Instant::now();
            }
            Ok(Some(WorkerCommand::Discard)) => {
                queue.discard_all();
                stats.queued.store(0, Ordering::Relaxed);
            }
            Ok(Some(WorkerCommand::Shutdown)) | Ok(None) => {
                let _ = drain_all(
                    &config,
                    &queue,
                    &mut channel,
                    &mut next_batch_id,
                    &stats,
                )
                .await;
                break;
            }
            Err(_elapsed) => {
                if !queue.is_empty() {
                    let _ = drain_once(
                        &config,
                        &queue,
                        &mut channel,
                        &mut next_batch_id,
                        &stats,
                    )
                    .await;
                    last_flush = Instant::now();
                }
            }
        }
    }
}

/// Send up to one batch of `max_batch_size` events.
async fn drain_once(
    config: &FernConfig,
    queue: &Arc<dyn EventQueue>,
    channel: &mut Option<Channel>,
    next_batch_id: &mut u64,
    stats: &Arc<WorkerStats>,
) -> Result<(), String> {
    let batch = queue.drain_batch(config.max_batch_size);
    if batch.is_empty() {
        return Ok(());
    }
    let batch_count = batch.len();
    let batch_id = *next_batch_id;
    *next_batch_id = next_batch_id.wrapping_add(1);

    match send_batch(config, channel, batch_id, batch).await {
        Ok(ack) => {
            stats
                .accepted
                .fetch_add(ack.events_accepted as usize, Ordering::Relaxed);
            stats
                .dropped
                .fetch_add(ack.events_rejected as usize, Ordering::Relaxed);
            stats.queued.store(queue.len(), Ordering::Relaxed);
            Ok(())
        }
        Err((events, reason)) => {
            // Re-queue everything; reset the channel so the next
            // attempt re-dials.
            *channel = None;
            for ev in events {
                queue.push(ev);
            }
            stats.queued.store(queue.len(), Ordering::Relaxed);
            tracing_log(format!(
                "fern-collector send failed (batch={batch_id}, count={batch_count}): {reason}"
            ));
            Err(reason)
        }
    }
}

/// Drain the whole queue in batch-sized chunks.
async fn drain_all(
    config: &FernConfig,
    queue: &Arc<dyn EventQueue>,
    channel: &mut Option<Channel>,
    next_batch_id: &mut u64,
    stats: &Arc<WorkerStats>,
) -> Result<(), String> {
    let mut last_err: Option<String> = None;
    while !queue.is_empty() {
        if let Err(e) = drain_once(config, queue, channel, next_batch_id, stats).await {
            // On hard failure, stop draining — events stay queued
            // for the next opportunity.
            last_err = Some(e);
            break;
        }
    }
    match last_err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Single-batch RPC: opens a fresh stream, sends one batch,
/// reads exactly one ack, drops the stream. Sub-phase A keeps it
/// simple at the cost of efficiency — a long-lived bidi stream
/// would be more efficient but adds reconnection complexity.
async fn send_batch(
    config: &FernConfig,
    channel: &mut Option<Channel>,
    batch_id: u64,
    events: Vec<OwnedEvent>,
) -> Result<proto::IngestAck, (Vec<OwnedEvent>, String)> {
    use proto::telemetry_client::TelemetryClient;

    let chan = match ensure_channel(config, channel).await {
        Ok(c) => c,
        Err(e) => return Err((events, e)),
    };

    let mut client = TelemetryClient::new(chan);

    let mode = if config.install_id.is_some() {
        proto::TelemetryMode::Pseudonymous
    } else {
        proto::TelemetryMode::Anonymous
    };
    // Build the wire-format batch from the owned events. The
    // adapter's install_id (when configured) overrides anything
    // the event carried — events emitted by the adapter all
    // belong to the same install.
    let proto_batch = proto::EventBatch {
        batch_id,
        product_id: config.product_id.clone(),
        mode: mode as i32,
        schema_version: config.schema_version,
        events: events
            .iter()
            .map(|ev| {
                let mut e = owned_to_proto_event(ev);
                e.install_id = config.install_id.clone();
                e
            })
            .collect(),
    };

    // The tonic bidi-stream API takes a request stream; we build
    // a one-shot stream containing only this batch.
    let (tx, rx) = mpsc::channel::<proto::EventBatch>(1);
    let send_result = tx.send(proto_batch).await;
    if send_result.is_err() {
        return Err((events, "internal channel closed".into()));
    }
    drop(tx); // close the stream so the server sees end-of-input

    let request_stream = ReceiverStream::new(rx);
    // Build an explicit Request so we can attach the bearer token
    // (when configured) to the call's metadata.
    let mut request = tonic::Request::new(request_stream);
    if let Some(token) = config.bearer_token.as_ref() {
        match MetadataValue::try_from(format!("Bearer {token}")) {
            Ok(v) => {
                request.metadata_mut().insert("authorization", v);
            }
            Err(e) => return Err((events, format!("bearer token not ASCII: {e}"))),
        }
    }
    let response = match client.ingest(request).await {
        Ok(r) => r,
        Err(e) => return Err((events, format!("ingest rpc: {e}"))),
    };

    let mut response_stream = response.into_inner();
    match response_stream.message().await {
        Ok(Some(ack)) => Ok(ack),
        Ok(None) => Err((events, "no ack from server".into())),
        Err(e) => Err((events, format!("ack stream: {e}"))),
    }
}

async fn ensure_channel(
    config: &FernConfig,
    channel: &mut Option<Channel>,
) -> Result<Channel, String> {
    if let Some(c) = channel {
        return Ok(c.clone());
    }
    let mut endpoint = Endpoint::from_shared(config.endpoint.clone())
        .map_err(|e| format!("bad endpoint: {e}"))?
        .timeout(config.request_timeout)
        .connect_timeout(config.request_timeout);

    if let Some(tls) = &config.tls {
        let mut client_tls = ClientTlsConfig::new();
        if let Some(ca) = &tls.ca_pem {
            client_tls = client_tls.ca_certificate(Certificate::from_pem(ca));
        }
        if let (Some(cert), Some(key)) = (&tls.client_cert_pem, &tls.client_key_pem) {
            client_tls = client_tls.identity(Identity::from_pem(cert, key));
        }
        if let Some(domain) = &tls.domain_name {
            client_tls = client_tls.domain_name(domain);
        }
        endpoint = endpoint
            .tls_config(client_tls)
            .map_err(|e| format!("tls config: {e}"))?;
    }

    let c = endpoint
        .connect()
        .await
        .map_err(|e| format!("connect: {e}"))?;
    *channel = Some(c.clone());
    Ok(c)
}

// -------------------- conversions --------------------

fn owned_to_proto_event(ev: &OwnedEvent) -> proto::Event {
    use std::time::UNIX_EPOCH;
    let ts = ev.timestamp.duration_since(UNIX_EPOCH).unwrap_or_default();
    let timestamp = Some(prost_types::Timestamp {
        seconds: ts.as_secs() as i64,
        nanos: ts.subsec_nanos() as i32,
    });
    let category = match ev.category {
        fern_core::telemetry::EventCategory::Intent => proto::EventCategory::Intent,
        fern_core::telemetry::EventCategory::Lifecycle => proto::EventCategory::Lifecycle,
        fern_core::telemetry::EventCategory::Navigation => proto::EventCategory::Navigation,
        fern_core::telemetry::EventCategory::Census => proto::EventCategory::Census,
        fern_core::telemetry::EventCategory::Custom => proto::EventCategory::Custom,
    } as i32;
    proto::Event {
        name: ev.name.clone(),
        category,
        timestamp,
        install_id: ev.install_id.clone(),
        session_id: ev.session_id.clone(),
        props: ev.props.iter().map(owned_to_proto_prop).collect(),
    }
}

fn owned_to_proto_prop(p: &OwnedProp) -> proto::Prop {
    let value = match &p.value {
        OwnedPropValue::Str(s) => Some(proto::prop::Value::Str(s.clone())),
        OwnedPropValue::U32(n) => Some(proto::prop::Value::U32(*n)),
        OwnedPropValue::I64(n) => Some(proto::prop::Value::I64(*n)),
        OwnedPropValue::Bool(b) => Some(proto::prop::Value::Boolean(*b)),
        OwnedPropValue::F64Bucket(b) => Some(proto::prop::Value::F64Bucket(proto::F64Bucket {
            min_x100: b.min_x100,
            max_x100: b.max_x100,
        })),
        OwnedPropValue::HistogramStrU32(entries) => {
            Some(proto::prop::Value::HistogramStrU32(proto::HistogramStrU32 {
                entries: entries
                    .iter()
                    .map(|(k, v)| proto::HistogramEntry {
                        key: k.clone(),
                        count: *v,
                    })
                    .collect(),
            }))
        }
    };
    proto::Prop {
        key: p.key.clone(),
        value,
    }
}

// Telemetry must never panic the host application; failed sends
// are reported via stderr (visible in dev) and the dropped/queued
// counters (visible to the privacy widget). Stays minimal in
// sub-phase A; sub-phase B can wire `tracing` properly.
fn tracing_log(msg: String) {
    eprintln!("{msg}");
}

// -------------------- fetch / erase one-shots (sub-phase C) --------------------
//
// These run on the adapter's tokio runtime, called via `block_on`
// from the sync `fetch_remote_data` / `erase_remote_data` trait
// methods. They open a fresh tonic Channel per call (no pooling)
// since fetch/erase are user-driven, low-frequency operations —
// the cost of dialing the endpoint is dwarfed by the round-trip.

use fern_core::telemetry::{RemoteEvent, RemoteValue};
use std::collections::BTreeMap;
use std::time::{Duration, UNIX_EPOCH};

use crate::config::TlsClientConfig;

async fn open_channel(
    endpoint: &str,
    tls: Option<&TlsClientConfig>,
    timeout: Duration,
) -> Result<Channel, String> {
    let mut ep = Endpoint::from_shared(endpoint.to_string())
        .map_err(|e| format!("bad endpoint: {e}"))?
        .timeout(timeout)
        .connect_timeout(timeout);
    if let Some(tls) = tls {
        let mut client_tls = ClientTlsConfig::new();
        if let Some(ca) = &tls.ca_pem {
            client_tls = client_tls.ca_certificate(Certificate::from_pem(ca));
        }
        if let (Some(cert), Some(key)) = (&tls.client_cert_pem, &tls.client_key_pem) {
            client_tls = client_tls.identity(Identity::from_pem(cert, key));
        }
        if let Some(domain) = &tls.domain_name {
            client_tls = client_tls.domain_name(domain);
        }
        ep = ep
            .tls_config(client_tls)
            .map_err(|e| format!("tls config: {e}"))?;
    }
    ep.connect().await.map_err(|e| format!("connect: {e}"))
}

fn attach_bearer<T>(req: &mut tonic::Request<T>, bearer: Option<&str>) -> Result<(), String> {
    if let Some(token) = bearer {
        let v = MetadataValue::try_from(format!("Bearer {token}"))
            .map_err(|e| format!("bearer not ASCII: {e}"))?;
        req.metadata_mut().insert("authorization", v);
    }
    Ok(())
}

pub(crate) async fn fetch_via_grpc(
    endpoint: &str,
    product_id: &str,
    install_id: &str,
    bearer: Option<&str>,
    tls: Option<&TlsClientConfig>,
    timeout: Duration,
) -> Result<Vec<RemoteEvent>, String> {
    use proto::telemetry_client::TelemetryClient;

    let chan = open_channel(endpoint, tls, timeout).await?;
    let mut client = TelemetryClient::new(chan);
    let mut req = tonic::Request::new(proto::FetchRequest {
        product_id: product_id.to_string(),
        install_id: install_id.to_string(),
    });
    attach_bearer(&mut req, bearer)?;
    let response = client
        .fetch(req)
        .await
        .map_err(|e| format!("fetch rpc: {e}"))?;
    let mut stream = response.into_inner();
    let mut out: Vec<RemoteEvent> = Vec::new();
    while let Some(page) = stream
        .message()
        .await
        .map_err(|e| format!("fetch stream: {e}"))?
    {
        for ev in page.events {
            out.push(proto_event_to_remote(&ev));
        }
        if page.is_last {
            break;
        }
    }
    Ok(out)
}

pub(crate) async fn erase_via_grpc(
    endpoint: &str,
    product_id: &str,
    install_id: &str,
    bearer: Option<&str>,
    tls: Option<&TlsClientConfig>,
    timeout: Duration,
) -> Result<u64, String> {
    use proto::telemetry_client::TelemetryClient;

    let chan = open_channel(endpoint, tls, timeout).await?;
    let mut client = TelemetryClient::new(chan);
    let mut req = tonic::Request::new(proto::EraseRequest {
        product_id: product_id.to_string(),
        install_id: install_id.to_string(),
    });
    attach_bearer(&mut req, bearer)?;
    let ack = client
        .erase(req)
        .await
        .map_err(|e| format!("erase rpc: {e}"))?
        .into_inner();
    Ok(ack.events_erased)
}

fn proto_event_to_remote(ev: &proto::Event) -> RemoteEvent {
    let timestamp = ev
        .timestamp
        .as_ref()
        .map(|t| {
            UNIX_EPOCH
                + Duration::from_secs(t.seconds.max(0) as u64)
                + Duration::from_nanos(t.nanos.max(0) as u64)
        })
        .unwrap_or(UNIX_EPOCH);
    let mut properties: BTreeMap<String, RemoteValue> = BTreeMap::new();
    for prop in &ev.props {
        let value = match &prop.value {
            Some(proto::prop::Value::Str(s)) => RemoteValue::String(s.clone()),
            Some(proto::prop::Value::U32(n)) => RemoteValue::Int(*n as i64),
            Some(proto::prop::Value::I64(n)) => RemoteValue::Int(*n),
            Some(proto::prop::Value::Boolean(b)) => RemoteValue::Bool(*b),
            Some(proto::prop::Value::F64Bucket(b)) => {
                RemoteValue::String(format!("[{}, {}]", b.min_x100, b.max_x100))
            }
            Some(proto::prop::Value::HistogramStrU32(_)) => {
                RemoteValue::String("{histogram}".into())
            }
            None => RemoteValue::Null,
        };
        properties.insert(prop.key.clone(), value);
    }
    RemoteEvent {
        name: ev.name.clone(),
        timestamp,
        properties,
    }
}
