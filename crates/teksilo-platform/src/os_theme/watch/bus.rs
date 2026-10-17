// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The watch's source on Linux: its own connection to the session bus, with
//! one match rule per listened signal.

use std::future::{Future, poll_fn};
use std::pin::{Pin, pin};
use std::sync::Arc;
use std::task::Poll;
use std::time::Instant;

use zbus::export::futures_core::Stream;
use zbus::zvariant::OwnedValue;
use zbus::{MatchRule, Message, MessageStream};

use super::{ChangeSource, Heard, LISTENED, PORTAL_SETTINGS, Stop, signal_matters};
use crate::linux_portal::{CALL_TIMEOUT, within};

/// How many signals of one rule may wait unread before the oldest is
/// dropped. The watch reads them as they come; one is enough to act on.
const QUEUED: usize = 64;

/// The session bus, as a [`ChangeSource`].
pub(crate) struct BusSource {
    /// Held so the streams' connection stays open.
    _connection: zbus::Connection,
    streams: Vec<MessageStream>,
    stop: Arc<Stop>,
}

impl BusSource {
    /// Connect to the bus at `address` (`None`: the session bus the
    /// environment names) and subscribe to every listened signal. `None`
    /// when the bus cannot be reached within
    /// [`CALL_TIMEOUT`], refuses a match
    /// rule, or the watch stops meanwhile.
    pub(crate) fn connect(address: Option<&str>, stop: Arc<Stop>) -> Option<BusSource> {
        let subscribed = async_io::block_on(until_stopped(&stop, async {
            let builder = match address {
                Some(address) => zbus::connection::Builder::address(address),
                None => zbus::connection::Builder::session(),
            };
            let connection = within(
                CALL_TIMEOUT,
                builder.ok()?.method_timeout(CALL_TIMEOUT).build(),
            )
            .await?
            .ok()?;
            let mut streams = Vec::with_capacity(LISTENED.len());
            for (interface, member, path) in LISTENED {
                let rule = MatchRule::builder()
                    .msg_type(zbus::message::Type::Signal)
                    .interface(interface)
                    .ok()?
                    .member(member)
                    .ok()?
                    .path(path)
                    .ok()?
                    .build();
                streams.push(
                    MessageStream::for_match_rule(rule, &connection, Some(QUEUED))
                        .await
                        .ok()?,
                );
            }
            Some((connection, streams))
        }))
        .flatten();
        let (connection, streams) = subscribed?;
        Some(BusSource {
            _connection: connection,
            streams,
            stop,
        })
    }
}

impl ChangeSource for BusSource {
    fn wait(&mut self, deadline: Option<Instant>) -> Heard {
        let mut timer = deadline.map(async_io::Timer::at);
        let stop = &self.stop;
        let streams = &mut self.streams;
        async_io::block_on(poll_fn(|cx| {
            if stop.poll_stopped(cx) {
                return Poll::Ready(Heard::Closed);
            }
            for stream in streams.iter_mut() {
                loop {
                    match Pin::new(&mut *stream).poll_next(cx) {
                        Poll::Ready(Some(Ok(message))) => {
                            if message_matters(&message) {
                                return Poll::Ready(Heard::Change);
                            }
                        }
                        // A message zbus could not read: the next one may
                        // be fine.
                        Poll::Ready(Some(Err(_))) => {}
                        // The connection is gone.
                        Poll::Ready(None) => return Poll::Ready(Heard::Closed),
                        Poll::Pending => break,
                    }
                }
            }
            if let Some(timer) = timer.as_mut()
                && Pin::new(timer).poll(cx).is_ready()
            {
                return Poll::Ready(Heard::Deadline);
            }
            Poll::Pending
        }))
    }
}

/// Whether `message` is a signal the watch acts on
/// ([`signal_matters`]), reading a portal
/// `SettingChanged`'s namespace from its body.
fn message_matters(message: &Message) -> bool {
    let header = message.header();
    let (Some(interface), Some(member), Some(path)) =
        (header.interface(), header.member(), header.path())
    else {
        return false;
    };
    let namespace = if interface.as_str() == PORTAL_SETTINGS {
        message
            .body()
            .deserialize::<(String, String, OwnedValue)>()
            .ok()
            .map(|(namespace, _, _)| namespace)
    } else {
        None
    };
    signal_matters(
        interface.as_str(),
        member.as_str(),
        path.as_str(),
        namespace.as_deref(),
    )
}

/// `future`'s output, or `None` once `stop` is set.
async fn until_stopped<T>(stop: &Stop, future: impl Future<Output = T>) -> Option<T> {
    let mut future = pin!(future);
    poll_fn(|cx| {
        if stop.poll_stopped(cx) {
            return Poll::Ready(None);
        }
        future.as_mut().poll(cx).map(Some)
    })
    .await
}
