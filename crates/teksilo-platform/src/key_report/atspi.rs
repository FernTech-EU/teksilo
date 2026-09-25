// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The AT-SPI registry, over D-Bus.
//!
//! A connection of its own to the accessibility bus (the one
//! `accesskit_unix` holds is private to it), found the way libatspi finds it:
//! `AT_SPI_BUS_ADDRESS` when set, else `org.a11y.Bus.GetAddress` on the
//! session bus. The futures are driven with `pollster` on the worker thread;
//! zbus reads the socket on its own executor thread, and async-io drives its
//! reactor from a thread of its own when nobody else does.

use std::time::Duration;

use super::worker::{LinkError, RegistryLink, ThreadedReporter, Timing};
use super::{DeviceEvent, KeyEventReporter};

const REGISTRY: &str = "org.a11y.atspi.Registry";
const DEVICE_EVENT_CONTROLLER: &str = "/org/a11y/atspi/registry/deviceeventcontroller";
const DEVICE_EVENT_CONTROLLER_INTERFACE: &str = "org.a11y.atspi.DeviceEventController";

/// How long the worker waits on the bus before giving a call up. The event
/// loop never waits this long (see [`Timing::PLATFORM`]); this only bounds
/// how long a hung registry keeps the worker from the reports behind it. The
/// registry itself gives a listener 3 s to answer (`send_and_allow_reentry`
/// in at-spi2-core's `registryd/deviceeventcontroller.c`), so it is given a
/// little more.
const CALL_TIMEOUT: Duration = Duration::from_secs(4);

/// The platform reporter: a worker thread talking to the registry.
pub(super) fn start_reporter() -> Box<dyn KeyEventReporter> {
    Box::new(ThreadedReporter::spawn(
        Box::new(|| AtspiLink::connect().map(|link| Box::new(link) as Box<dyn RegistryLink>)),
        Timing::PLATFORM,
    ))
}

struct AtspiLink {
    connection: zbus::Connection,
}

impl AtspiLink {
    fn connect() -> Result<Self, LinkError> {
        pollster::block_on(async {
            let address = accessibility_bus_address().await?;
            let connection = zbus::connection::Builder::address(address.as_str())
                .map_err(|error| LinkError::NoBus(error.to_string()))?
                .method_timeout(CALL_TIMEOUT)
                .build()
                .await
                .map_err(|error| LinkError::NoBus(error.to_string()))?;
            Ok(Self { connection })
        })
    }
}

async fn accessibility_bus_address() -> Result<String, LinkError> {
    if let Ok(address) = std::env::var("AT_SPI_BUS_ADDRESS")
        && !address.is_empty()
    {
        return Ok(address);
    }
    let no_bus = |error: zbus::Error| LinkError::NoBus(error.to_string());
    let session = zbus::connection::Builder::session()
        .map_err(no_bus)?
        .method_timeout(CALL_TIMEOUT)
        .build()
        .await
        .map_err(no_bus)?;
    let reply = session
        .call_method(
            Some("org.a11y.Bus"),
            "/org/a11y/bus",
            Some("org.a11y.Bus"),
            "GetAddress",
            &(),
        )
        .await
        .map_err(no_bus)?;
    reply.body().deserialize::<String>().map_err(no_bus)
}

/// The body of `NotifyListenersSync`: **one** argument, the `(uiiiisb)`
/// struct, the signed fields as they come. A bare seven-field tuple would
/// serialize as seven arguments (`uiiiisb`), which the registry refuses.
type NotifyBody<'a> = ((u32, i32, i32, i32, i32, &'a str, bool),);

fn notify_body(event: &DeviceEvent) -> NotifyBody<'_> {
    ((
        event.kind as u32,
        event.keysym as i32,
        event.hw_code as i32,
        event.modifiers as i32,
        event.timestamp as i32,
        event.event_string.as_str(),
        event.is_text,
    ),)
}

impl RegistryLink for AtspiLink {
    fn notify(&mut self, event: &DeviceEvent) -> Result<bool, LinkError> {
        pollster::block_on(async {
            let reply = self
                .connection
                .call_method(
                    Some(REGISTRY),
                    DEVICE_EVENT_CONTROLLER,
                    Some(DEVICE_EVENT_CONTROLLER_INTERFACE),
                    "NotifyListenersSync",
                    &notify_body(event),
                )
                .await
                .map_err(classify)?;
            reply
                .body()
                .deserialize::<bool>()
                .map_err(|error| LinkError::Refused(error.to_string()))
        })
    }
}

/// An error the registry answered with, or a call that timed out, leaves the
/// connection usable; anything else means making it again.
fn classify(error: zbus::Error) -> LinkError {
    match &error {
        zbus::Error::MethodError(..) | zbus::Error::FDO(_) => LinkError::Refused(error.to_string()),
        zbus::Error::InputOutput(io) if io.kind() == std::io::ErrorKind::TimedOut => {
            LinkError::Refused(error.to_string())
        }
        _ => LinkError::Broken(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DEVICE_EVENT_CONTROLLER, DEVICE_EVENT_CONTROLLER_INTERFACE, REGISTRY, notify_body,
    };
    use crate::key_report::{DeviceEvent, DeviceEventKind};

    /// The message as it goes on the bus, built the way `call_method` builds
    /// it, with no bus: one argument, a `(uiiiisb)` struct, which is what the
    /// registry's demarshaller reads (`spi_dbus_demarshal_deviceEvent`),
    /// carrying the event's fields in that order.
    #[test]
    fn the_report_is_one_uiiiisb_struct() {
        let event = DeviceEvent {
            kind: DeviceEventKind::Released,
            keysym: 0xff54,
            hw_code: 116,
            modifiers: 1 << 2,
            timestamp: 4_000_000_000,
            event_string: "é".into(),
            is_text: true,
        };
        let message = zbus::Message::method_call(DEVICE_EVENT_CONTROLLER, "NotifyListenersSync")
            .and_then(|builder| builder.destination(REGISTRY))
            .and_then(|builder| builder.interface(DEVICE_EVENT_CONTROLLER_INTERFACE))
            .and_then(|builder| builder.build(&notify_body(&event)))
            .expect("the message builds");
        // The header's SIGNATURE field as it goes on the wire: code 8, a
        // one-byte signature `g`, then the signature's length and text. One
        // struct argument is `(uiiiisb)`; seven arguments would be `uiiiisb`.
        // (zbus's parsed `Signature` reads both the same way, so the bytes
        // are what is checked.)
        let wire = message.data().bytes();
        let field = |signature: &str| {
            let mut bytes = vec![8, 1, b'g', 0, signature.len() as u8];
            bytes.extend_from_slice(signature.as_bytes());
            bytes.push(0);
            wire.windows(bytes.len()).any(|window| window == bytes)
        };
        assert!(
            field("(uiiiisb)") && !field("uiiiisb"),
            "the header's body signature is not one (uiiiisb) struct: {:?}",
            String::from_utf8_lossy(wire)
        );
        let body = message.body();
        let ((kind, keysym, hw_code, modifiers, timestamp, text, is_text),): ((
            u32,
            i32,
            i32,
            i32,
            i32,
            String,
            bool,
        ),) = body.deserialize().expect("the body reads back");
        assert_eq!(
            (kind, keysym, hw_code, modifiers, text.as_str(), is_text),
            (1, 0xff54, 116, 4, "é", true)
        );
        assert_eq!(
            timestamp as u32, 4_000_000_000,
            "the time wraps into the i32"
        );
    }
}
