//! The AIS decoder against gpsd's regression captures (`testdata/ais/SOURCE.md`, D-32):
//! every sentence gpsd decoded to one of the eight message types in scope must decode
//! here to the same raw field values. gpsd's `.chk` files are the oracle; the sentences
//! are real transmitters, so a field that disagrees is a decoder bug, not a fixture one.

use std::path::PathBuf;

use gungnir_interop::ais::{AisCodec, AisError, AisMessage, StaticDataReportPart, StaticExtent};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../testdata/ais")
        .join(name)
}

/// Walk one `.chk`: sentences feed the codec; each `{"class":"AIS"...}` line is compared
/// with the message the preceding sentences produced.
fn check(name: &str) -> (usize, usize) {
    let text = std::fs::read_to_string(fixture(name)).expect("fixture present");
    let mut codec = AisCodec::default();
    let mut pending: Option<Result<AisMessage, AisError>> = None;
    // gpsd repeats the last AIS decode after an interleaved GPS sentence; a repeat is
    // skipped only when it is byte-identical to the line already compared.
    let mut last_oracle: Option<String> = None;
    let mut compared = 0;
    let mut skipped = 0;
    for (n, line) in text.lines().enumerate() {
        if line.starts_with('!') {
            match codec.decode_line(line) {
                Ok(None) | Err(AisError::NotAisSentence) => {}
                Ok(Some(m)) => pending = Some(Ok(m)),
                Err(e) => pending = Some(Err(e)),
            }
            continue;
        }
        if !line.starts_with("{\"class\":\"AIS\"") {
            continue;
        }
        let oracle: serde_json::Value = serde_json::from_str(line).expect("oracle is JSON");
        let t = oracle["type"].as_u64().expect("type");
        let Some(decoded) = pending.take() else {
            if last_oracle.as_deref() == Some(line) {
                continue;
            }
            panic!("{name}:{}: oracle line with no sentence before it", n + 1);
        };
        last_oracle = Some(line.to_owned());
        if ![1, 2, 3, 5, 18, 19, 21, 24].contains(&t) {
            skipped += 1;
            continue;
        }
        let m = match decoded {
            Ok(m) => m,
            Err(e) => panic!("{name}:{}: type {t} failed to decode: {e}", n + 1),
        };
        compare(name, n + 1, &oracle, &m);
        compared += 1;
    }
    (compared, skipped)
}

fn u(o: &serde_json::Value, key: &str) -> u64 {
    o[key].as_u64().unwrap_or_else(|| panic!("oracle {key}"))
}

fn i(o: &serde_json::Value, key: &str) -> i64 {
    o[key].as_i64().unwrap_or_else(|| panic!("oracle {key}"))
}

fn b(o: &serde_json::Value, key: &str) -> bool {
    o[key].as_bool().unwrap_or_else(|| panic!("oracle {key}"))
}

fn s(o: &serde_json::Value, key: &str) -> String {
    o[key]
        .as_str()
        .unwrap_or_else(|| panic!("oracle {key}"))
        .to_owned()
}

#[allow(clippy::too_many_lines)]
fn compare(name: &str, line: usize, o: &serde_json::Value, m: &AisMessage) {
    let at = format!("{name}:{line}");
    let h = m.header();
    assert_eq!(u64::from(h.message_type), u(o, "type"), "{at} type");
    assert_eq!(u64::from(h.repeat_indicator), u(o, "repeat"), "{at} repeat");
    assert_eq!(u64::from(h.mmsi), u(o, "mmsi"), "{at} mmsi");
    match m {
        AisMessage::Position(p) => {
            assert_eq!(
                u64::from(p.navigational_status),
                u(o, "status"),
                "{at} status"
            );
            assert_eq!(i64::from(p.rate_of_turn), i(o, "turn"), "{at} turn");
            assert_eq!(u64::from(p.speed_over_ground), u(o, "speed"), "{at} speed");
            assert_eq!(p.position.accuracy_high, b(o, "accuracy"), "{at} accuracy");
            assert_eq!(p.position.longitude, i(o, "lon"), "{at} lon");
            assert_eq!(p.position.latitude, i(o, "lat"), "{at} lat");
            assert_eq!(
                u64::from(p.course_over_ground),
                u(o, "course"),
                "{at} course"
            );
            assert_eq!(u64::from(p.true_heading), u(o, "heading"), "{at} heading");
            assert_eq!(u64::from(p.time_stamp), u(o, "second"), "{at} second");
            assert_eq!(
                u64::from(p.special_manoeuvre),
                u(o, "maneuver"),
                "{at} maneuver"
            );
            assert_eq!(p.raim, b(o, "raim"), "{at} raim");
            assert_eq!(u64::from(p.radio_status), u(o, "radio"), "{at} radio");
        }
        AisMessage::StaticVoyage(v) => {
            assert_eq!(u64::from(v.imo_number), u(o, "imo"), "{at} imo");
            assert_eq!(
                u64::from(v.ais_version),
                u(o, "ais_version"),
                "{at} ais_version"
            );
            assert_eq!(v.call_sign, s(o, "callsign"), "{at} callsign");
            assert_eq!(v.name, s(o, "shipname"), "{at} shipname");
            assert_eq!(u64::from(v.ship_type), u(o, "shiptype"), "{at} shiptype");
            assert_eq!(
                u64::from(v.dimensions.to_bow),
                u(o, "to_bow"),
                "{at} to_bow"
            );
            assert_eq!(
                u64::from(v.dimensions.to_stern),
                u(o, "to_stern"),
                "{at} to_stern"
            );
            assert_eq!(
                u64::from(v.dimensions.to_port),
                u(o, "to_port"),
                "{at} to_port"
            );
            assert_eq!(
                u64::from(v.dimensions.to_starboard),
                u(o, "to_starboard"),
                "{at} to_starboard"
            );
            assert_eq!(
                u64::from(v.position_fixing_device),
                u(o, "epfd"),
                "{at} epfd"
            );
            let (month, day, hour, minute) = v.eta;
            assert_eq!(
                format!("{month:02}-{day:02}T{hour:02}:{minute:02}Z"),
                s(o, "eta"),
                "{at} eta"
            );
            assert_eq!(u64::from(v.draught), u(o, "draught"), "{at} draught");
            assert_eq!(v.destination, s(o, "destination"), "{at} destination");
            assert_eq!(u64::from(v.dte_not_ready), u(o, "dte"), "{at} dte");
        }
        AisMessage::ClassBPosition(p) => {
            assert_eq!(u64::from(p.speed_over_ground), u(o, "speed"), "{at} speed");
            assert_eq!(p.position.accuracy_high, b(o, "accuracy"), "{at} accuracy");
            assert_eq!(p.position.longitude, i(o, "lon"), "{at} lon");
            assert_eq!(p.position.latitude, i(o, "lat"), "{at} lat");
            assert_eq!(
                u64::from(p.course_over_ground),
                u(o, "course"),
                "{at} course"
            );
            assert_eq!(u64::from(p.true_heading), u(o, "heading"), "{at} heading");
            assert_eq!(u64::from(p.time_stamp), u(o, "second"), "{at} second");
            assert_eq!(p.class_b_unit_cs, b(o, "cs"), "{at} cs");
            assert_eq!(p.class_b_display, b(o, "display"), "{at} display");
            assert_eq!(p.class_b_dsc, b(o, "dsc"), "{at} dsc");
            assert_eq!(p.class_b_band, b(o, "band"), "{at} band");
            assert_eq!(p.class_b_message_22, b(o, "msg22"), "{at} msg22");
            assert_eq!(p.raim, b(o, "raim"), "{at} raim");
            assert_eq!(u64::from(p.radio_status), u(o, "radio"), "{at} radio");
        }
        AisMessage::ClassBExtended(p) => {
            assert_eq!(u64::from(p.speed_over_ground), u(o, "speed"), "{at} speed");
            assert_eq!(p.position.accuracy_high, b(o, "accuracy"), "{at} accuracy");
            assert_eq!(p.position.longitude, i(o, "lon"), "{at} lon");
            assert_eq!(p.position.latitude, i(o, "lat"), "{at} lat");
            assert_eq!(
                u64::from(p.course_over_ground),
                u(o, "course"),
                "{at} course"
            );
            assert_eq!(u64::from(p.true_heading), u(o, "heading"), "{at} heading");
            assert_eq!(u64::from(p.time_stamp), u(o, "second"), "{at} second");
            assert_eq!(p.name, s(o, "shipname"), "{at} shipname");
            assert_eq!(u64::from(p.ship_type), u(o, "shiptype"), "{at} shiptype");
            assert_eq!(
                u64::from(p.dimensions.to_bow),
                u(o, "to_bow"),
                "{at} to_bow"
            );
            assert_eq!(
                u64::from(p.dimensions.to_stern),
                u(o, "to_stern"),
                "{at} to_stern"
            );
            assert_eq!(
                u64::from(p.dimensions.to_port),
                u(o, "to_port"),
                "{at} to_port"
            );
            assert_eq!(
                u64::from(p.dimensions.to_starboard),
                u(o, "to_starboard"),
                "{at} to_starboard"
            );
            assert_eq!(
                u64::from(p.position_fixing_device),
                u(o, "epfd"),
                "{at} epfd"
            );
            assert_eq!(p.raim, b(o, "raim"), "{at} raim");
            assert_eq!(u64::from(p.dte_not_ready), u(o, "dte"), "{at} dte");
            assert_eq!(p.assigned_mode, b(o, "assigned"), "{at} assigned");
        }
        AisMessage::AidToNavigation(a) => {
            assert_eq!(u64::from(a.aid_type), u(o, "aid_type"), "{at} aid_type");
            assert_eq!(a.full_name(), s(o, "name"), "{at} name");
            assert_eq!(a.position.accuracy_high, b(o, "accuracy"), "{at} accuracy");
            assert_eq!(a.position.longitude, i(o, "lon"), "{at} lon");
            assert_eq!(a.position.latitude, i(o, "lat"), "{at} lat");
            assert_eq!(
                u64::from(a.dimensions.to_bow),
                u(o, "to_bow"),
                "{at} to_bow"
            );
            assert_eq!(
                u64::from(a.dimensions.to_stern),
                u(o, "to_stern"),
                "{at} to_stern"
            );
            assert_eq!(
                u64::from(a.dimensions.to_port),
                u(o, "to_port"),
                "{at} to_port"
            );
            assert_eq!(
                u64::from(a.dimensions.to_starboard),
                u(o, "to_starboard"),
                "{at} to_starboard"
            );
            assert_eq!(
                u64::from(a.position_fixing_device),
                u(o, "epfd"),
                "{at} epfd"
            );
            assert_eq!(u64::from(a.time_stamp), u(o, "second"), "{at} second");
            assert_eq!(u64::from(a.regional), u(o, "regional"), "{at} regional");
            assert_eq!(a.off_position, b(o, "off_position"), "{at} off_position");
            assert_eq!(a.raim, b(o, "raim"), "{at} raim");
            assert_eq!(a.virtual_aid, b(o, "virtual_aid"), "{at} virtual_aid");
        }
        AisMessage::StaticData(d) => match &d.part {
            StaticDataReportPart::A { name } => {
                assert_eq!(*name, s(o, "shipname"), "{at} shipname");
            }
            StaticDataReportPart::B {
                ship_type,
                vendor_id,
                unit_model,
                unit_serial,
                call_sign,
                extent,
                ..
            } => {
                assert_eq!(u64::from(*ship_type), u(o, "shiptype"), "{at} shiptype");
                // gpsd still reads the vendor id over the pre-2014 42-bit field, so its
                // string runs on into the model and serial bits; the -6 table's three
                // characters are its prefix.
                assert!(
                    vendor_id.len() <= 3 && s(o, "vendorid").starts_with(vendor_id.as_str()),
                    "{at} vendorid {vendor_id:?} vs {:?}",
                    s(o, "vendorid")
                );
                assert_eq!(u64::from(*unit_model), u(o, "model"), "{at} model");
                assert_eq!(u64::from(*unit_serial), u(o, "serial"), "{at} serial");
                assert_eq!(*call_sign, s(o, "callsign"), "{at} callsign");
                match extent {
                    StaticExtent::Dimensions(dim) => {
                        assert_eq!(u64::from(dim.to_bow), u(o, "to_bow"), "{at} to_bow");
                        assert_eq!(u64::from(dim.to_stern), u(o, "to_stern"), "{at} to_stern");
                        assert_eq!(u64::from(dim.to_port), u(o, "to_port"), "{at} to_port");
                        assert_eq!(
                            u64::from(dim.to_starboard),
                            u(o, "to_starboard"),
                            "{at} to_starboard"
                        );
                    }
                    StaticExtent::MotherShip { mmsi } => {
                        assert_eq!(u64::from(*mmsi), u(o, "mothership_mmsi"), "{at} mothership");
                    }
                }
            }
        },
        AisMessage::Unsupported { .. } => panic!("{at}: in-scope type decoded as unsupported"),
    }
}

#[test]
fn every_in_scope_sentence_in_the_gpsd_captures_decodes_as_gpsd_did() {
    let mut total = 0;
    for name in [
        "ais-nmea.log.chk",
        "ais-raw-messages.log.chk",
        "ais-18-27.log.chk",
        "ais-nmea-type6-fid55.log.chk",
        "ais_unpack_sixbit.log.chk",
    ] {
        let (compared, skipped) = check(name);
        eprintln!("{name}: {compared} compared, {skipped} out of scope");
        total += compared;
    }
    assert!(
        total >= 500,
        "the captures hold hundreds of in-scope messages; only {total} were compared"
    );
}

/// The oracle covers every one of the eight types, so a decoder that read only the
/// common ones could not pass by accident.
#[test]
fn the_captures_exercise_every_message_type_in_scope() {
    let mut seen = std::collections::BTreeSet::new();
    for name in [
        "ais-nmea.log.chk",
        "ais-18-27.log.chk",
        "ais-raw-messages.log.chk",
        "ais-nmea-type6-fid55.log.chk",
        "ais_unpack_sixbit.log.chk",
    ] {
        let text = std::fs::read_to_string(fixture(name)).expect("fixture");
        for line in text.lines().filter(|l| l.starts_with("{\"class\":\"AIS\"")) {
            let o: serde_json::Value = serde_json::from_str(line).expect("json");
            seen.insert(o["type"].as_u64().unwrap_or_default());
        }
    }
    for t in [1, 2, 3, 5, 18, 19, 21, 24] {
        assert!(seen.contains(&t), "no type {t} in the captures");
    }
}
