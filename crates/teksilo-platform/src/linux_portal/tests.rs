// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use std::time::{Duration, Instant};

use zbus::zvariant::Value;

use super::{QuietList, decode_bool, decode_rgb, decode_u32, read_reply_value};

fn rgb(r: f64, g: f64, b: f64) -> Value<'static> {
    Value::from((r, g, b))
}

#[test]
fn a_u32_setting_decodes() {
    assert_eq!(decode_u32(&Value::U32(0)), Some(0));
    assert_eq!(decode_u32(&Value::U32(1)), Some(1));
    assert_eq!(decode_u32(&Value::U32(2)), Some(2));
}

#[test]
fn a_u32_setting_of_another_type_is_unknown() {
    assert_eq!(decode_u32(&Value::I32(1)), None);
    assert_eq!(decode_u32(&Value::U8(1)), None);
    assert_eq!(decode_u32(&Value::Bool(true)), None);
    assert_eq!(decode_u32(&Value::from("1")), None);
    // A `Read` reply handed to the decoder without its extra layer removed.
    assert_eq!(decode_u32(&Value::Value(Box::new(Value::U32(1)))), None);
}

#[test]
fn a_read_reply_loses_exactly_one_variant() {
    let reply = Value::Value(Box::new(Value::U32(1)));
    let inner = read_reply_value(&reply).expect("one variant layer");
    assert_eq!(decode_u32(inner), Some(1));

    let colour = Value::Value(Box::new(rgb(0.25, 0.5, 1.0)));
    assert_eq!(
        decode_rgb(read_reply_value(&colour).expect("one variant layer")),
        Some((0.25, 0.5, 1.0))
    );

    // Two extra layers leave one, which no decoder accepts.
    let doubled = Value::Value(Box::new(Value::Value(Box::new(Value::U32(1)))));
    assert_eq!(read_reply_value(&doubled).and_then(decode_u32), None);
}

#[test]
fn a_read_reply_without_its_extra_variant_is_unknown() {
    assert!(read_reply_value(&Value::U32(1)).is_none());
    assert!(read_reply_value(&rgb(0.0, 0.0, 0.0)).is_none());
}

#[test]
fn an_accent_colour_in_range_decodes() {
    assert_eq!(
        decode_rgb(&rgb(0.2078, 0.5176, 0.8941)),
        Some((0.2078, 0.5176, 0.8941))
    );
    // Both ends of the range are in it.
    assert_eq!(decode_rgb(&rgb(0.0, 0.0, 0.0)), Some((0.0, 0.0, 0.0)));
    assert_eq!(decode_rgb(&rgb(1.0, 1.0, 1.0)), Some((1.0, 1.0, 1.0)));
}

#[test]
fn an_accent_colour_out_of_range_is_unset() {
    assert_eq!(decode_rgb(&rgb(-0.01, 0.5, 0.5)), None);
    assert_eq!(decode_rgb(&rgb(0.5, 1.01, 0.5)), None);
    assert_eq!(decode_rgb(&rgb(0.5, 0.5, 255.0)), None);
    assert_eq!(decode_rgb(&rgb(f64::NAN, 0.5, 0.5)), None);
    assert_eq!(decode_rgb(&rgb(0.5, f64::INFINITY, 0.5)), None);
}

#[test]
fn an_accent_colour_of_another_shape_is_unknown() {
    // Two or four components.
    assert_eq!(decode_rgb(&Value::from((0.5, 0.5))), None);
    assert_eq!(decode_rgb(&Value::from((0.5, 0.5, 0.5, 1.0))), None);
    // Integers where doubles belong.
    assert_eq!(decode_rgb(&Value::from((128_u32, 128_u32, 128_u32))), None);
    assert_eq!(decode_rgb(&Value::from((0.5, 0.5, 1_u32))), None);
    // Not a structure at all.
    assert_eq!(decode_rgb(&Value::F64(0.5)), None);
    assert_eq!(decode_rgb(&Value::U32(1)), None);
    assert_eq!(decode_rgb(&Value::from(vec![0.5, 0.5, 0.5])), None);
}

#[test]
fn a_boolean_property_decodes_and_nothing_else_does() {
    assert_eq!(decode_bool(&Value::Bool(true)), Some(true));
    assert_eq!(decode_bool(&Value::Bool(false)), Some(false));
    assert_eq!(decode_bool(&Value::U32(1)), None);
    assert_eq!(decode_bool(&Value::from("true")), None);
    assert_eq!(
        decode_bool(&Value::Value(Box::new(Value::Bool(true)))),
        None
    );
}

#[test]
fn a_silenced_name_is_quiet_until_its_instant_and_no_other_is() {
    let start = Instant::now();
    let mut quiet = QuietList::new();
    assert!(!quiet.is_quiet("org.freedesktop.portal.Desktop", start));

    quiet.silence(
        "org.freedesktop.portal.Desktop",
        start + Duration::from_secs(5),
    );
    assert!(quiet.is_quiet("org.freedesktop.portal.Desktop", start));
    assert!(quiet.is_quiet(
        "org.freedesktop.portal.Desktop",
        start + Duration::from_millis(4999)
    ));
    assert!(!quiet.is_quiet(
        "org.freedesktop.portal.Desktop",
        start + Duration::from_secs(5)
    ));
    assert!(!quiet.is_quiet("org.a11y.Bus", start));

    // Silencing again moves the instant rather than adding a second entry.
    quiet.silence(
        "org.freedesktop.portal.Desktop",
        start + Duration::from_secs(1),
    );
    assert!(!quiet.is_quiet(
        "org.freedesktop.portal.Desktop",
        start + Duration::from_secs(2)
    ));
    assert_eq!(quiet.entries.len(), 1);
}

/// Reads against a private `dbus-daemon` serving a fake portal and a fake
/// AT-SPI status object. Opt-in, since it needs `dbus-daemon` installed:
/// run with `TEKSILO_DBUS_TESTS=1`. The bus itself is shared with the
/// theme watch's real-bus test.
pub(crate) mod real_bus {
    use std::io::{BufRead, BufReader};
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use zbus::zvariant::{OwnedValue, Value};

    use super::super::{Bus, CALL_TIMEOUT, PORTAL, PORTAL_PATH};

    const APPEARANCE: &str = "org.freedesktop.appearance";
    /// A namespace whose reads the fake portal never answers in time.
    const HANG: &str = "test.hang";

    pub(crate) fn enabled() -> bool {
        if std::env::var("TEKSILO_DBUS_TESTS").as_deref() == Ok("1") {
            return true;
        }
        println!("skipped: set TEKSILO_DBUS_TESTS=1 to run the private dbus-daemon tests");
        false
    }

    /// A `dbus-daemon` of our own, on a configuration that activates no
    /// service (so nothing on the machine is started on it) and listens in
    /// a directory of its own. Killed and cleaned up on drop.
    pub(crate) struct PrivateBus {
        child: Child,
        dir: PathBuf,
        pub(crate) address: String,
    }

    impl PrivateBus {
        pub(crate) fn start() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!(
                "teksilo-dbus-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&dir).expect("create the bus directory");
            let config = dir.join("bus.conf");
            std::fs::write(
                &config,
                format!(
                    r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:dir={}</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#,
                    dir.display()
                ),
            )
            .expect("write the bus configuration");
            let mut child = Command::new("dbus-daemon")
                .arg(format!("--config-file={}", config.display()))
                .args(["--nofork", "--print-address"])
                .env_clear()
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .expect("start dbus-daemon (is it installed?)");
            let mut address = String::new();
            BufReader::new(child.stdout.take().expect("piped stdout"))
                .read_line(&mut address)
                .expect("read the bus address");
            let address = address.trim().to_owned();
            assert!(!address.is_empty(), "dbus-daemon printed no address");
            Self {
                child,
                dir,
                address,
            }
        }
    }

    impl Drop for PrivateBus {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn setting(namespace: &str, key: &str) -> Option<Value<'static>> {
        match (namespace, key) {
            (APPEARANCE, "color-scheme") => Some(Value::U32(1)),
            (APPEARANCE, "contrast") => Some(Value::U32(0)),
            (APPEARANCE, "reduced-motion") => Some(Value::U32(1)),
            (APPEARANCE, "accent-color") => Some(Value::from((0.25, 0.5, 0.75))),
            ("test.wrong", "type") => Some(Value::from("dark")),
            ("test.out-of-range", "accent-color") => Some(Value::from((0.25, 1.5, 0.75))),
            _ => None,
        }
    }

    fn owned(value: Value<'static>) -> OwnedValue {
        OwnedValue::try_from(value).expect("no file descriptors")
    }

    fn not_found(namespace: &str, key: &str) -> zbus::fdo::Error {
        zbus::fdo::Error::Failed(format!("{namespace} {key} not found"))
    }

    /// A version 2 portal: `ReadOne` and `Read`.
    struct CurrentPortal;

    #[zbus::interface(name = "org.freedesktop.portal.Settings")]
    impl CurrentPortal {
        async fn read_one(&self, namespace: &str, key: &str) -> zbus::fdo::Result<OwnedValue> {
            if namespace == HANG {
                async_io::Timer::after(CALL_TIMEOUT * 6).await;
            }
            setting(namespace, key)
                .map(owned)
                .ok_or_else(|| not_found(namespace, key))
        }

        /// Present, so a test can tell a reader that wrongly fell back from
        /// one that read `ReadOne`: this answers a different value.
        async fn read(&self, _namespace: &str, _key: &str) -> zbus::fdo::Result<OwnedValue> {
            Ok(owned(Value::Value(Box::new(Value::U32(7)))))
        }
    }

    /// A portal older than version 2: only `Read`, whose reply wraps the
    /// value in one more variant.
    struct LegacyPortal;

    #[zbus::interface(name = "org.freedesktop.portal.Settings")]
    impl LegacyPortal {
        async fn read(&self, namespace: &str, key: &str) -> zbus::fdo::Result<OwnedValue> {
            setting(namespace, key)
                .map(|value| owned(Value::Value(Box::new(value))))
                .ok_or_else(|| not_found(namespace, key))
        }
    }

    struct FakeStatus;

    #[zbus::interface(name = "org.a11y.Status")]
    impl FakeStatus {
        #[zbus(property)]
        fn screen_reader_enabled(&self) -> bool {
            true
        }

        #[zbus(property)]
        fn is_enabled(&self) -> u32 {
            1
        }
    }

    fn serve<I: zbus::object_server::Interface>(bus: &PrivateBus, portal: I) -> zbus::Connection {
        async_io::block_on(async {
            zbus::connection::Builder::address(bus.address.as_str())?
                .name(PORTAL)?
                .serve_at(PORTAL_PATH, portal)?
                .name("org.a11y.Bus")?
                .serve_at("/org/a11y/bus", FakeStatus)?
                .build()
                .await
        })
        .expect("serve the fake portal")
    }

    fn screen_reader(reader: &Bus) -> Option<bool> {
        reader.read_bool_property(
            "org.a11y.Bus",
            "/org/a11y/bus",
            "org.a11y.Status",
            "ScreenReaderEnabled",
        )
    }

    #[test]
    fn reads_a_current_portal_with_read_one() {
        if !enabled() {
            return;
        }
        let bus = PrivateBus::start();
        let _server = serve(&bus, CurrentPortal);
        let reader = Bus::at(bus.address.clone());

        assert_eq!(reader.read_portal_u32(APPEARANCE, "color-scheme"), Some(1));
        assert_eq!(reader.read_portal_u32(APPEARANCE, "contrast"), Some(0));
        assert_eq!(
            reader.read_portal_u32(APPEARANCE, "reduced-motion"),
            Some(1)
        );
        assert_eq!(
            reader.read_portal_rgb(APPEARANCE, "accent-color"),
            Some((0.25, 0.5, 0.75))
        );
        // An error reply and a reply of the wrong type or range are unknown,
        // and none of them falls back to `Read`.
        assert_eq!(reader.read_portal_u32(APPEARANCE, "no-such-key"), None);
        assert_eq!(reader.read_portal_u32("test.wrong", "type"), None);
        assert_eq!(reader.read_portal_rgb(APPEARANCE, "color-scheme"), None);
        assert_eq!(
            reader.read_portal_rgb("test.out-of-range", "accent-color"),
            None
        );

        assert_eq!(screen_reader(&reader), Some(true));
        // A property of another type.
        assert_eq!(
            reader.read_bool_property(
                "org.a11y.Bus",
                "/org/a11y/bus",
                "org.a11y.Status",
                "IsEnabled"
            ),
            None
        );
        // A service nobody owns on this bus.
        assert_eq!(
            reader.read_bool_property("org.example.Absent", "/", "org.example.Absent", "Flag"),
            None
        );

        // A portal that does not answer costs one timeout, and the reads made
        // during the quiet period after it do not wait at all.
        let started = Instant::now();
        assert_eq!(reader.read_portal_u32(HANG, "anything"), None);
        let waited = started.elapsed();
        assert!(
            waited >= CALL_TIMEOUT && waited < CALL_TIMEOUT * 4,
            "the hung read took {waited:?}"
        );
        let started = Instant::now();
        assert_eq!(reader.read_portal_u32(APPEARANCE, "color-scheme"), None);
        assert!(started.elapsed() < CALL_TIMEOUT / 2);
        // Another destination on the same bus is still read.
        assert_eq!(screen_reader(&reader), Some(true));
    }

    #[test]
    fn falls_back_to_read_on_a_legacy_portal() {
        if !enabled() {
            return;
        }
        let bus = PrivateBus::start();
        let _server = serve(&bus, LegacyPortal);
        let reader = Bus::at(bus.address.clone());

        assert_eq!(reader.read_portal_u32(APPEARANCE, "color-scheme"), Some(1));
        assert_eq!(reader.read_portal_u32(APPEARANCE, "contrast"), Some(0));
        assert_eq!(
            reader.read_portal_rgb(APPEARANCE, "accent-color"),
            Some((0.25, 0.5, 0.75))
        );
        assert_eq!(reader.read_portal_u32(APPEARANCE, "no-such-key"), None);
        assert_eq!(reader.read_portal_u32("test.wrong", "type"), None);
    }

    #[test]
    fn a_bus_that_is_not_there_reads_nothing_promptly() {
        if !enabled() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("teksilo-dbus-absent-{}", std::process::id()));
        let reader = Bus::at(format!("unix:path={}", dir.join("no-bus").display()));
        let started = Instant::now();
        assert_eq!(reader.read_portal_u32(APPEARANCE, "color-scheme"), None);
        assert_eq!(screen_reader(&reader), None);
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
