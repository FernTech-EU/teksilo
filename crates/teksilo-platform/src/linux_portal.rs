// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Desktop settings read over the D-Bus session bus with zbus: the XDG
//! desktop portal's `org.freedesktop.portal.Settings` and a boolean property
//! such as the AT-SPI `ScreenReaderEnabled` flag.
//!
//! Every read is blocking, bounded by [`CALL_TIMEOUT`], and answers `None`
//! on any failure: no bus, no portal, an error reply, a reply of the wrong
//! type, or no reply in time. Callers treat `None` as "cannot tell" and fall
//! back to the desktop's own configuration.
//!
//! One session connection per process is made on the first read and reused
//! by every later one. A connection that breaks is dropped and made again on
//! the next read.
//!
//! The decoding is pure ([`decode_u32`], [`decode_rgb`], [`decode_bool`],
//! [`read_reply_value`]) so its rules are tested without a bus.

use std::future::{Future, poll_fn};
use std::pin::pin;
use std::sync::{Mutex, PoisonError};
use std::task::Poll;
use std::time::{Duration, Instant};

use zbus::zvariant::{OwnedValue, Value};

/// The portal's bus name, object path and settings interface.
const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const SETTINGS_INTERFACE: &str = "org.freedesktop.portal.Settings";
const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";
/// The error a service answers for a method its interface does not have: a
/// portal older than version 2 of the settings interface, which has `Read`
/// and not `ReadOne`.
const UNKNOWN_METHOD: &str = "org.freedesktop.DBus.Error.UnknownMethod";

/// How long one read waits: for the connection to the bus (socket, the
/// authentication handshake and `Hello`) and then for each reply.
///
/// A running portal answers a settings read in a few milliseconds. The slow
/// case is the first read of a session, when the bus starts the portal by
/// D-Bus activation; that usually completes within a few hundred
/// milliseconds. A read that misses its reply costs one value, which the
/// caller then reads from the desktop's configuration instead, whereas
/// waiting longer would hold up the first window: the `busctl` process this
/// replaces waited up to sd-bus's 25 s default on a hung portal. 500 ms
/// keeps a cold activation inside the bound on an ordinary machine and caps
/// what a hung bus can cost.
pub(crate) const CALL_TIMEOUT: Duration = Duration::from_millis(500);

/// How long a destination that let a read time out, or a bus that could not
/// be connected to, is left alone. Startup reads several settings within a
/// few milliseconds of each other; without this, a hung portal would cost
/// [`CALL_TIMEOUT`] once per setting instead of once. A read made after the
/// quiet period (a re-query when the desktop's settings change) tries again.
const QUIET_PERIOD: Duration = Duration::from_secs(5);

/// The session bus named by the environment, shared by every read in the
/// process.
static SESSION: Bus = Bus::session();

/// A `u32` setting of the portal (`color-scheme`, `contrast`,
/// `reduced-motion` in `org.freedesktop.appearance`).
pub(crate) fn read_portal_u32(namespace: &str, key: &str) -> Option<u32> {
    SESSION.read_portal_u32(namespace, key)
}

/// A `(ddd)` colour setting of the portal (`accent-color` in
/// `org.freedesktop.appearance`), as red, green and blue in `0.0..=1.0`.
pub(crate) fn read_portal_rgb(namespace: &str, key: &str) -> Option<(f64, f64, f64)> {
    SESSION.read_portal_rgb(namespace, key)
}

/// A boolean property read with `org.freedesktop.DBus.Properties.Get`.
pub(crate) fn read_bool_property(
    service: &str,
    path: &str,
    interface: &str,
    property: &str,
) -> Option<bool> {
    SESSION.read_bool_property(service, path, interface, property)
}

// ── Decoding ────────────────────────────────────────────────────────────────

/// A `u32` value, and nothing else: a portal that answered with another
/// integer type is not guessed at.
pub(crate) fn decode_u32(value: &Value<'_>) -> Option<u32> {
    match value {
        Value::U32(v) => Some(*v),
        _ => None,
    }
}

/// A `(ddd)` structure whose three components are in `0.0..=1.0`. The portal
/// specification says an accent colour out of that range is to be treated as
/// unset, so one component outside it (or NaN) rejects the whole colour.
pub(crate) fn decode_rgb(value: &Value<'_>) -> Option<(f64, f64, f64)> {
    let Value::Structure(structure) = value else {
        return None;
    };
    let [Value::F64(r), Value::F64(g), Value::F64(b)] = structure.fields() else {
        return None;
    };
    let in_range = |c: f64| (0.0..=1.0).contains(&c);
    (in_range(*r) && in_range(*g) && in_range(*b)).then_some((*r, *g, *b))
}

/// A `b` value.
pub(crate) fn decode_bool(value: &Value<'_>) -> Option<bool> {
    match value {
        Value::Bool(v) => Some(*v),
        _ => None,
    }
}

/// The setting inside a reply of the deprecated `Read`, which wraps the
/// value in one more variant than `ReadOne` does. A reply without that layer
/// is not a `Read` reply and gives `None`.
pub(crate) fn read_reply_value<'v>(value: &'v Value<'v>) -> Option<&'v Value<'v>> {
    match value {
        Value::Value(inner) => Some(inner),
        _ => None,
    }
}

// ── Quiet periods ───────────────────────────────────────────────────────────

/// Destinations (and the bus itself, under the empty name) that are not to
/// be called before an instant.
#[derive(Debug, Default)]
struct QuietList {
    entries: Vec<(String, Instant)>,
}

impl QuietList {
    const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    fn is_quiet(&self, name: &str, now: Instant) -> bool {
        self.entries
            .iter()
            .any(|(entry, until)| entry == name && now < *until)
    }

    fn silence(&mut self, name: &str, until: Instant) {
        match self.entries.iter_mut().find(|(entry, _)| entry == name) {
            Some((_, entry_until)) => *entry_until = until,
            None => self.entries.push((name.to_owned(), until)),
        }
    }
}

/// The key under which a failed connection is recorded.
const BUS_ITSELF: &str = "";

// ── The bus ─────────────────────────────────────────────────────────────────

/// A session bus and its lazily made connection.
pub(crate) struct Bus {
    /// `None` for the session bus the environment names; an address for a
    /// bus named explicitly (the tests' private bus).
    address: Option<String>,
    state: Mutex<BusState>,
}

struct BusState {
    connection: Option<zbus::Connection>,
    quiet: QuietList,
}

/// Why a call gave no value.
#[derive(Debug, PartialEq, Eq)]
enum CallError {
    /// The service has the interface but not the method.
    UnknownMethod,
    /// Anything else: no bus, no service, another error, a timeout.
    Failed,
}

impl Bus {
    const fn session() -> Self {
        Self::with_address(None)
    }

    /// A bus at an explicit address, so a test passes its private bus in
    /// rather than through the environment.
    #[cfg(test)]
    pub(crate) fn at(address: String) -> Self {
        Self::with_address(Some(address))
    }

    const fn with_address(address: Option<String>) -> Self {
        Self {
            address,
            state: Mutex::new(BusState {
                connection: None,
                quiet: QuietList::new(),
            }),
        }
    }

    pub(crate) fn read_portal_u32(&self, namespace: &str, key: &str) -> Option<u32> {
        self.read_setting(namespace, key, decode_u32)
    }

    pub(crate) fn read_portal_rgb(&self, namespace: &str, key: &str) -> Option<(f64, f64, f64)> {
        self.read_setting(namespace, key, decode_rgb)
    }

    pub(crate) fn read_bool_property(
        &self,
        service: &str,
        path: &str,
        interface: &str,
        property: &str,
    ) -> Option<bool> {
        let value = self
            .call(
                service,
                path,
                PROPERTIES_INTERFACE,
                "Get",
                &(interface, property),
            )
            .ok()?;
        decode_bool(&value)
    }

    /// `ReadOne`, or `Read` when the portal does not have `ReadOne`.
    fn read_setting<T>(
        &self,
        namespace: &str,
        key: &str,
        decode: impl Fn(&Value<'_>) -> Option<T>,
    ) -> Option<T> {
        let body = (namespace, key);
        match self.call(PORTAL, PORTAL_PATH, SETTINGS_INTERFACE, "ReadOne", &body) {
            Ok(value) => decode(&value),
            Err(CallError::UnknownMethod) => {
                let value = self
                    .call(PORTAL, PORTAL_PATH, SETTINGS_INTERFACE, "Read", &body)
                    .ok()?;
                decode(read_reply_value(&value)?)
            }
            Err(CallError::Failed) => None,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BusState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The shared connection, made on first use. The lock is held while
    /// connecting, so concurrent first reads make one connection; the wait
    /// is bounded by [`CALL_TIMEOUT`].
    fn connection(&self) -> Option<zbus::Connection> {
        let mut state = self.lock();
        if let Some(connection) = &state.connection {
            return Some(connection.clone());
        }
        let now = Instant::now();
        if state.quiet.is_quiet(BUS_ITSELF, now) {
            return None;
        }
        let address = self.address.as_deref();
        let connected = async_io::block_on(within(CALL_TIMEOUT, async move {
            let builder = match address {
                Some(address) => zbus::connection::Builder::address(address),
                None => zbus::connection::Builder::session(),
            };
            builder
                .ok()?
                .method_timeout(CALL_TIMEOUT)
                .build()
                .await
                .ok()
        }))
        .flatten();
        match connected {
            Some(connection) => {
                state.connection = Some(connection.clone());
                Some(connection)
            }
            None => {
                state
                    .quiet
                    .silence(BUS_ITSELF, Instant::now() + QUIET_PERIOD);
                None
            }
        }
    }

    /// One method call whose reply body is a single variant.
    fn call<B>(
        &self,
        destination: &str,
        path: &str,
        interface: &str,
        method: &str,
        body: &B,
    ) -> Result<OwnedValue, CallError>
    where
        B: zbus::export::serde::Serialize + zbus::zvariant::DynamicType,
    {
        if self.lock().quiet.is_quiet(destination, Instant::now()) {
            return Err(CallError::Failed);
        }
        let connection = self.connection().ok_or(CallError::Failed)?;
        // The connection carries the method timeout, so a reply that does
        // not come back in time is a `TimedOut` error rather than a wait.
        let reply = async_io::block_on(connection.call_method(
            Some(destination),
            path,
            Some(interface),
            method,
            body,
        ));
        match reply {
            Ok(message) => message
                .body()
                .deserialize::<OwnedValue>()
                .map_err(|_| CallError::Failed),
            Err(zbus::Error::MethodError(name, _, _)) if name.as_str() == UNKNOWN_METHOD => {
                Err(CallError::UnknownMethod)
            }
            Err(zbus::Error::MethodError(..) | zbus::Error::FDO(_)) => Err(CallError::Failed),
            Err(zbus::Error::InputOutput(error))
                if error.kind() == std::io::ErrorKind::TimedOut =>
            {
                self.lock()
                    .quiet
                    .silence(destination, Instant::now() + QUIET_PERIOD);
                Err(CallError::Failed)
            }
            Err(zbus::Error::InputOutput(_)) => {
                // The connection itself failed: make a new one next time.
                self.lock().connection = None;
                Err(CallError::Failed)
            }
            // A name or body zbus refused before sending, or a reply it
            // could not read: the connection is still good.
            Err(_) => Err(CallError::Failed),
        }
    }
}

/// `future`'s output, or `None` when `limit` passes first.
pub(crate) async fn within<T>(limit: Duration, future: impl Future<Output = T>) -> Option<T> {
    let mut future = pin!(future);
    let mut timer = pin!(async_io::Timer::after(limit));
    poll_fn(|cx| {
        if let Poll::Ready(output) = future.as_mut().poll(cx) {
            return Poll::Ready(Some(output));
        }
        if timer.as_mut().poll(cx).is_ready() {
            return Poll::Ready(None);
        }
        Poll::Pending
    })
    .await
}

#[cfg(test)]
pub(crate) mod tests;
