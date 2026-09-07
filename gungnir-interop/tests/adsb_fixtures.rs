//! The ADS-B decoder against the two open-source decoders, over the two vendored
//! captures (GAP-010, `testdata/adsb/SOURCE.md`).
//!
//! # What a green run here means, and what it does not
//!
//! It means `gungnir_interop::adsb` reads these frames the same way `rs1090` 0.6.0 and
//! `adsb_deku` 0.7.1 read them. It **does not** mean the decoder conforms to ICAO Doc
//! 9871, and it cannot: no ADS-B specification is both free to obtain and permissively
//! licensed, so this project pins none (`docs/design/external-standards.md` §4). The two
//! oracles are only partly independent — `adsb_deku` cites the same ICAO section
//! numbering and `rs1090` takes inspiration from `pyModeS` — so a misreading shared by
//! both would pass here in silence. The verification-capability-table row's oracle column
//! says "open-source consensus, not normative" for that reason, and the two parts of the
//! build that *are* checkable by arithmetic are gated in `adsb_crc.rs` and `adsb_cpr.rs`
//! instead of here.
//!
//! # The disagreements this file records rather than hides
//!
//! Two of them are the oracles' and one is a difference of policy:
//!
//! - **`adsb_deku` reads seven callsign characters where the field holds eight**
//!   (`aircraft_identification_read`, `for _ in 0..=6`). Its callsign is therefore a
//!   prefix, and this file compares it as one.
//! - **`adsb_deku` carries the altitude as `u16`**, so it cannot represent the negative
//!   altitudes the Q-bit encoding allows for airports below sea level. Comparison against
//!   it is skipped where ours is negative, and counted.
//! - **Both oracles drop interior spaces from a callsign**; this build removes trailing
//!   spaces only, because dropping an interior one silently changes what the aircraft
//!   transmitted. The comparison strips spaces from both sides.

use std::collections::BTreeMap;
use std::path::PathBuf;

use adsb_deku::adsb::ME as DekuMe;
use adsb_deku::deku::DekuContainerRead;
use adsb_deku::{Frame as DekuFrame, DF as DekuDf};
use gungnir_interop::adsb::{
    demod, messages::MeMessage, parse_avr, AdsbCodec, AltitudeSource, Downlink,
};
use rs1090::decode::adsb::ME as RsMe;
use rs1090::decode::{Message as RsMessage, DF as RsDf};

/// A knot of tolerance is far more than either decoder's arithmetic needs; the values
/// are exact integers scaled by fixed factors and agreement is to the last bit.
const SPEED_TOLERANCE_KT: f64 = 1e-9;
const ANGLE_TOLERANCE_DEG: f64 = 1e-9;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../testdata/adsb")
        .join(name)
}

/// What was compared, so a passing run cannot be a run that compared nothing.
#[derive(Debug, Default)]
struct Tally {
    frames: usize,
    addresses: usize,
    identifications: usize,
    airborne_positions: usize,
    surface_positions: usize,
    velocities: usize,
    carried: BTreeMap<u8, usize>,
    /// Comparisons skipped because an oracle cannot represent the value, with the reason.
    skipped: BTreeMap<&'static str, usize>,
}

impl Tally {
    fn skip(&mut self, reason: &'static str) {
        *self.skipped.entry(reason).or_default() += 1;
    }
}

/// Spaces removed, which is what both oracles do to a callsign and what makes their
/// strings comparable with this build's.
fn without_spaces(callsign: &str) -> String {
    callsign.chars().filter(|c| *c != ' ').collect()
}

fn close(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() <= tolerance
}

/// Compare one extended squitter against both oracles. Panics with the frame and the
/// field on any disagreement: a differential test that logged and continued would be a
/// test that passes while the decoder is wrong.
#[allow(clippy::too_many_lines)]
fn compare(octets: &[u8], ours: &Downlink, tally: &mut Tally) {
    let at = {
        use std::fmt::Write as _;
        let mut line = String::with_capacity(2 + octets.len() * 2);
        line.push('*');
        for octet in octets {
            // Writing into a `String` cannot fail; the result is discarded rather than
            // unwrapped, which keeps the no-`unwrap` rule even in a test.
            let _ = write!(line, "{octet:02X}");
        }
        line.push(';');
        line
    };

    let rs = RsMessage::try_from(octets).unwrap_or_else(|e| panic!("{at}: rs1090 refused: {e}"));
    let (_, deku) = DekuFrame::from_bytes((octets, 0))
        .unwrap_or_else(|e| panic!("{at}: adsb_deku refused: {e}"));

    let (rs_address, rs_me) = match &rs.df {
        RsDf::ExtendedSquitterADSB(adsb) => (adsb.icao24.0, &adsb.message),
        RsDf::ExtendedSquitterTisB { cf, .. } => (cf.aa.0, &cf.me),
        other => panic!("{at}: rs1090 read a downlink format this test did not offer: {other:?}"),
    };
    let (deku_address, deku_me) = match &deku.df {
        DekuDf::ADSB(adsb) => (adsb.icao, &adsb.me),
        DekuDf::TisB { cf, .. } => (cf.aa, &cf.me),
        other => panic!("{at}: adsb_deku read another downlink format: {other:?}"),
    };
    let deku_address =
        u32::from_be_bytes([0, deku_address.0[0], deku_address.0[1], deku_address.0[2]]);

    let ours_address = ours
        .announced_address()
        .unwrap_or_else(|| panic!("{at}: an extended squitter announces an address"))
        .0;
    assert_eq!(ours_address, rs_address, "{at}: address against rs1090");
    assert_eq!(
        ours_address, deku_address,
        "{at}: address against adsb_deku"
    );
    tally.addresses += 1;

    let Some(message) = ours.message() else {
        panic!("{at}: this build read no message from an extended squitter");
    };

    match message {
        MeMessage::Identification(m) => {
            let RsMe::BDS08 { inner, .. } = rs_me else {
                panic!("{at}: rs1090 did not read an identification");
            };
            let DekuMe::AircraftIdentification(d) = deku_me else {
                panic!("{at}: adsb_deku did not read an identification");
            };
            assert_eq!(
                without_spaces(&m.callsign),
                without_spaces(&inner.callsign),
                "{at}: callsign against rs1090"
            );
            // adsb_deku reads seven of the eight characters, so its answer is a prefix.
            let seven: String = m.callsign.chars().take(7).collect();
            assert_eq!(
                without_spaces(&seven),
                without_spaces(&d.cn),
                "{at}: callsign prefix against adsb_deku"
            );
            assert_eq!(m.category_code, inner.ca, "{at}: category against rs1090");
            assert_eq!(m.category_code, d.ca, "{at}: category against adsb_deku");
            assert_eq!(
                m.category_set().map(|c| c.to_string()),
                Some(format!("{:?}", d.tc)),
                "{at}: category set against adsb_deku"
            );
            tally.identifications += 1;
        }
        MeMessage::AirbornePosition(m) => {
            let RsMe::BDS05 { inner, .. } = rs_me else {
                panic!("{at}: rs1090 did not read an airborne position");
            };
            let (DekuMe::AirbornePositionBaroAltitude(d) | DekuMe::AirbornePositionGNSSAltitude(d)) =
                deku_me
            else {
                panic!("{at}: adsb_deku did not read an airborne position");
            };
            assert_eq!(m.type_code, d.tc, "{at}: type code against adsb_deku");
            assert_eq!(
                m.cpr.odd,
                inner.parity == rs1090::decode::cpr::CPRFormat::Odd
            );
            assert_eq!(
                m.cpr.odd,
                d.odd_flag == adsb_deku::CPRFormat::Odd,
                "{at}: CPR parity against adsb_deku"
            );
            assert_eq!(m.cpr.lat_cpr, inner.lat_cpr, "{at}: lat CPR against rs1090");
            assert_eq!(m.cpr.lon_cpr, inner.lon_cpr, "{at}: lon CPR against rs1090");
            assert_eq!(m.cpr.lat_cpr, d.lat_cpr, "{at}: lat CPR against adsb_deku");
            assert_eq!(m.cpr.lon_cpr, d.lon_cpr, "{at}: lon CPR against adsb_deku");
            assert_eq!(m.altitude_ft(), inner.alt, "{at}: altitude against rs1090");
            match (m.altitude_ft(), d.alt) {
                (Some(ours), Some(theirs)) if ours >= 0 => {
                    assert_eq!(ours, i32::from(theirs), "{at}: altitude against adsb_deku");
                }
                // adsb_deku carries the altitude as u16 and cannot hold a negative one.
                (Some(_), _) => tally.skip("negative altitude, adsb_deku carries u16"),
                (None, None) => {}
                (None, Some(theirs)) => {
                    panic!("{at}: this build read no altitude where adsb_deku read {theirs}")
                }
            }
            assert_eq!(
                m.altitude_source == AltitudeSource::GnssHeight,
                m.type_code >= 20,
                "{at}: the altitude source follows the type code"
            );
            tally.airborne_positions += 1;
        }
        MeMessage::SurfacePosition(m) => {
            let RsMe::BDS06 { inner, .. } = rs_me else {
                panic!("{at}: rs1090 did not read a surface position");
            };
            let DekuMe::SurfacePosition(d) = deku_me else {
                panic!("{at}: adsb_deku did not read a surface position");
            };
            assert_eq!(m.movement_code, d.mov, "{at}: movement against adsb_deku");
            assert_eq!(m.track_code, d.trk, "{at}: track against adsb_deku");
            assert_eq!(m.cpr.lat_cpr, inner.lat_cpr, "{at}: lat CPR against rs1090");
            assert_eq!(m.cpr.lon_cpr, inner.lon_cpr, "{at}: lon CPR against rs1090");
            assert_eq!(m.cpr.lat_cpr, d.lat_cpr, "{at}: lat CPR against adsb_deku");
            assert_eq!(m.cpr.lon_cpr, d.lon_cpr, "{at}: lon CPR against adsb_deku");
            assert_eq!(
                m.track_valid, inner.track_status,
                "{at}: track status against rs1090"
            );
            match (m.ground_speed_kt(), inner.groundspeed) {
                (Some(ours), Some(theirs)) => assert!(
                    close(ours, theirs, SPEED_TOLERANCE_KT),
                    "{at}: ground speed {ours} against rs1090's {theirs}"
                ),
                (None, None) => {}
                (ours, theirs) => panic!("{at}: ground speed {ours:?} against rs1090's {theirs:?}"),
            }
            match (m.ground_track_deg(), inner.track) {
                (Some(ours), Some(theirs)) => assert!(
                    close(ours, theirs, ANGLE_TOLERANCE_DEG),
                    "{at}: ground track {ours} against rs1090's {theirs}"
                ),
                (None, None) => {}
                (ours, theirs) => panic!("{at}: ground track {ours:?} against rs1090's {theirs:?}"),
            }
            tally.surface_positions += 1;
        }
        MeMessage::AirborneVelocity(m) => {
            let RsMe::BDS09(inner) = rs_me else {
                panic!("{at}: rs1090 did not read a velocity");
            };
            let DekuMe::AirborneVelocity(d) = deku_me else {
                panic!("{at}: adsb_deku did not read a velocity");
            };
            assert_eq!(m.subtype, inner.subtype, "{at}: subtype against rs1090");
            assert_eq!(m.subtype, d.st, "{at}: subtype against adsb_deku");
            assert_eq!(
                m.vertical_rate_ft_min()
                    .map(|v| i16::try_from(v).unwrap_or(i16::MAX)),
                inner.vertical_rate,
                "{at}: vertical rate against rs1090"
            );
            if let rs1090::decode::bds::bds09::AirborneVelocitySubType::GroundSpeedDecoding(g) =
                &inner.velocity
            {
                let ours_speed = m
                    .ground_speed_kt()
                    .unwrap_or_else(|| panic!("{at}: rs1090 read a ground speed and this did not"));
                let ours_track = m
                    .ground_track_deg()
                    .unwrap_or_else(|| panic!("{at}: rs1090 read a track and this did not"));
                assert!(
                    close(ours_speed, g.groundspeed, SPEED_TOLERANCE_KT),
                    "{at}: ground speed {ours_speed} against rs1090's {}",
                    g.groundspeed
                );
                assert!(
                    close(ours_track, g.track, ANGLE_TOLERANCE_DEG),
                    "{at}: track {ours_track} against rs1090's {}",
                    g.track
                );
                // `adsb_deku::AirborneVelocity::calculate` returns the speed, the track
                // and the vertical rate together, and returns `None` for all three when
                // the vertical rate alone is unavailable. So a frame whose vertical rate
                // is absent yields no comparison of the speed either; the skip is
                // counted, and this build's own answer is checked to be `None` for the
                // one field that is genuinely missing.
                #[allow(clippy::single_match_else)]
                match d.calculate() {
                    Some((deku_track, deku_speed, deku_vrate)) => {
                        assert!(
                            close(ours_speed, deku_speed, SPEED_TOLERANCE_KT),
                            "{at}: ground speed {ours_speed} against adsb_deku's {deku_speed}"
                        );
                        assert!(
                            close(ours_track, f64::from(deku_track), 1e-4),
                            "{at}: track {ours_track} against adsb_deku's {deku_track}"
                        );
                        assert_eq!(
                            m.vertical_rate_ft_min(),
                            Some(i32::from(deku_vrate)),
                            "{at}: vertical rate against adsb_deku"
                        );
                    }
                    None => {
                        assert_eq!(
                            m.vertical_rate_ft_min(),
                            None,
                            "{at}: adsb_deku returns nothing only when the vertical rate \
                             is unavailable, and this build read one"
                        );
                        tally.skip("vertical rate unavailable, adsb_deku returns no velocity");
                    }
                }
            }
            tally.velocities += 1;
        }
        MeMessage::Carried { type_code, .. } => {
            *tally.carried.entry(*type_code).or_default() += 1;
        }
    }
}

/// Run every extended squitter of a set of frames through this build and both oracles.
fn compare_all(frames: &[Vec<u8>]) -> (Tally, AdsbCodec) {
    let mut tally = Tally::default();
    let mut codec = AdsbCodec::default();
    for octets in frames {
        let Ok(decoded) = codec.decode_frame(octets) else {
            continue;
        };
        if !matches!(
            decoded,
            Downlink::ExtendedSquitter(_) | Downlink::Supplementary(_)
        ) {
            continue;
        }
        tally.frames += 1;
        compare(octets, &decoded, &mut tally);
    }
    (tally, codec)
}

fn avr_frames() -> Vec<Vec<u8>> {
    let text = std::fs::read_to_string(fixture("lax-messages-first40000.txt"))
        .expect("the vendored AVR corpus is present");
    text.lines()
        .filter_map(|line| parse_avr(line).ok())
        .collect()
}

/// The message-layer capture: 13 324 extended squitters recorded off air over Los
/// Angeles, every one of them read the same way by three decoders.
#[test]
fn every_extended_squitter_in_the_avr_capture_agrees_with_both_oracles() {
    let frames = avr_frames();
    assert_eq!(frames.len(), 40_000, "the vendored corpus is 40 000 frames");
    let (tally, _codec) = compare_all(&frames);
    eprintln!("AVR capture: {tally:#?}");
    assert_eq!(tally.frames, 13_324);
    assert_eq!(tally.addresses, 13_324);
    // The exact counts of the vendored prefix. Exact rather than a floor, so a
    // fixture that quietly changed is a failure rather than a smaller number nobody
    // reads: 488 identifications, 4 921 airborne positions, 4 901 velocities, and
    // 3 014 extended squitters carried under a type code this build does not interpret.
    assert_eq!(tally.identifications, 488);
    assert_eq!(tally.airborne_positions, 4_921);
    assert_eq!(tally.velocities, 4_901);
    assert_eq!(tally.surface_positions, 0);
    assert_eq!(
        tally.carried,
        [(24u8, 122usize), (28, 500), (29, 1405), (31, 987)]
            .into_iter()
            .collect()
    );
}

/// The radio-layer capture, demodulated here and decoded by all three: an independent
/// recording, by a different person, with a different receiver, at a different place.
/// The register asks for two recordings rather than one precisely so that a fixture that
/// happened to suit one decoder cannot carry the gate.
#[test]
fn every_extended_squitter_demodulated_from_the_iq_capture_agrees_with_both_oracles() {
    let raw = std::fs::read(fixture("modes1.bin")).expect("the vendored IQ capture is present");
    let (demodulated, stats) = demod::frames_from_iq(&raw);
    assert_eq!(stats.frames, 198);
    let frames: Vec<Vec<u8>> = demodulated.into_iter().map(|f| f.octets).collect();
    let (tally, _codec) = compare_all(&frames);
    eprintln!("IQ capture: {tally:#?}");
    assert_eq!(tally.frames, 141, "extended squitters in the IQ capture");
    assert_eq!(tally.identifications, 9);
    assert_eq!(tally.airborne_positions, 68);
    assert_eq!(tally.velocities, 64);
}

/// Nothing is dropped in silence: every frame the corpus holds is either decoded or
/// counted under a name, and the totals add up to the number of frames read.
#[test]
fn the_statistics_account_for_every_frame_in_the_capture() {
    let frames = avr_frames();
    let (_, codec) = compare_all(&frames);
    let stats = codec.stats();
    eprintln!("decode statistics over the capture: {stats:#?}");
    assert_eq!(stats.frames, 40_000);
    // Every downlink format in the capture that this build does not decode is named
    // and counted, and the counts are exact: an unfiltered receiver log is mostly not
    // extended squitters, and the point of the counters is that a reader can see that.
    assert_eq!(
        stats.other_downlink_formats,
        [
            (0u8, 12_938u64),
            (4, 4_265),
            (5, 81),
            (11, 8_307),
            (16, 838),
            (20, 185),
            (21, 62),
        ]
        .into_iter()
        .collect(),
        "the downlink formats this build does not decode, by name and count"
    );
    // The type codes this build carries rather than interprets are counted, and the
    // ones it reads are not in that list.
    for carried in [24u8, 28, 29, 31] {
        assert!(
            stats.carried_type_codes.contains_key(&carried),
            "type code {carried} is carried and must be counted"
        );
    }
    for read in [4u8, 11, 19] {
        assert!(
            !stats.carried_type_codes.contains_key(&read),
            "type code {read} is decoded and must not be counted as carried"
        );
    }
    let decoded: u64 = stats.decoded.values().sum();
    let counted = stats.not_decoded();
    assert_eq!(
        decoded + counted,
        u64::from(u32::try_from(frames.len()).expect("the corpus fits a u32")),
        "every frame is either decoded or counted"
    );
}

/// The corpus exercises each of the four message groups this build interprets, so a
/// decoder that read only the common ones could not pass by accident. Stated as a set
/// so the reason a group is missing is visible rather than inferred from a count.
#[test]
fn the_captures_exercise_every_message_group_this_build_interprets() {
    let frames = avr_frames();
    let (tally, _) = compare_all(&frames);
    assert!(tally.identifications > 0, "no identification messages");
    assert!(tally.airborne_positions > 0, "no airborne positions");
    assert!(tally.velocities > 0, "no airborne velocities");
    // **Surface position is not exercised by the vendored corpus.** The whole
    // `lax-messages.txt` holds one surface frame in 215 606, and it falls outside the
    // first 40 000 the repository carries (`testdata/adsb/SOURCE.md`). The decode path
    // is gated by the published CPR surface vectors in `adsb_cpr.rs` and by the unit
    // tests in `adsb::messages`, not by consensus, and this assertion records that
    // rather than leaving a reader to wonder why the count is zero.
    assert_eq!(
        tally.surface_positions, 0,
        "the vendored prefix holds no surface position; if one appears, the fixture \
         changed and testdata/adsb/SOURCE.md needs changing with it"
    );
}
