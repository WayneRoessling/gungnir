// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! What a LAS/LAZ/COPC file says its own coordinates are, and the conversion from that
//! into a deployment's local ENU frame (GAP-102, D-41).
//!
//! **Reading the declaration needs no projection library and is always compiled.** The
//! `crs` feature gates only [`to_local_enu`], the conversion itself, because that is the
//! part that links `libproj`. A default build therefore still knows when a file is not
//! in the frame its baseline claims, and refuses it by name rather than drawing it in
//! the wrong place -- the same discipline `gungnir-app/src/terrain.rs`'s
//! `placement_refusal` already applies to a DEM.
//!
//! **Where the split of work is, and why.** `gungnir-data` depends on no other crate in
//! this workspace (`ARCHITECTURE.md` §7.1), so it cannot reach `gungnir_model::
//! LocalFrame` or `gungnir-coord`'s WGS-84 math. It therefore does the half only it can
//! do -- the file's own CRS to geographic WGS-84 -- and takes the geographic-to-ENU half
//! as a closure from the caller, which is the same shape
//! `gungnir_analytics::coverage_from_registry` and `gungnir_assessment::anchor_list`
//! already use for exactly this reason.

use crate::geospatial::GridCrs;
use crate::DataError;

/// `GTModelTypeGeoKey` value for a two-dimensional projected system.
const MODEL_TYPE_PROJECTED: u16 = 1;
/// `GTModelTypeGeoKey` value for a geographic two-dimensional system.
const MODEL_TYPE_GEOGRAPHIC: u16 = 2;

/// The coordinate reference system a LAS file declares for itself, in whichever of the
/// two forms the LAS specification allows.
///
/// Deliberately not `GridCrs`, and not an extension of it: a LAS 1.4 file declares its
/// CRS as WKT, which carries a compound (horizontal + vertical) system that a
/// `GeoTIFF`
/// geokey directory reduced to one EPSG code cannot express. The geokey form *is*
/// `GridCrs`, reused rather than redefined, because there it is the same set of keys
/// read from the same spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PointCloudCrs {
    /// The LAS 1.4 WKT VLR: user id `LASF_Projection`, record 2112. The LAS 1.4
    /// specification names OGC WKT 1 here, which is what the parsing below assumes.
    Wkt(String),
    /// The LAS 1.0-1.3 `GeoTIFF` geokey VLRs (records 34735/34736/34737), reduced to
    /// EPSG code they name.
    Geokeys(GridCrs),
}

impl PointCloudCrs {
    /// The CRS definition to hand PROJ as the source of a conversion.
    ///
    /// The file's own WKT is preferred over any code a baseline names, because it is
    /// richer: a compound WKT states the vertical system, and a bare horizontal EPSG
    /// code does not.
    #[must_use]
    pub fn proj_definition(&self) -> Option<String> {
        match self {
            PointCloudCrs::Wkt(wkt) => Some(wkt.clone()),
            PointCloudCrs::Geokeys(
                GridCrs::Projected { epsg: Some(code) } | GridCrs::Geographic { epsg: Some(code) },
            ) => Some(format!("EPSG:{code}")),
            PointCloudCrs::Geokeys(_) => None,
        }
    }

    /// How many metres one unit of the file's **vertical** axis is, when the file says.
    ///
    /// `Some(1.0)` for a file whose heights are already metres. See [`to_local_enu`] for
    /// why this is read here at all rather than left to PROJ.
    ///
    /// **Two places are consulted, in order, and the second is not a guess.** A WKT that
    /// declares a `VERT_CS` states the vertical unit outright and that answer is final.
    /// A WKT that declares none -- which is the ordinary case for a file in a metric
    /// projected system such as a UTM zone -- leaves the height in the same linear unit
    /// as the easting and northing, so the projected system's own `UNIT` is what applies,
    /// and that is read instead. Without this second step every plain UTM file would be
    /// refused, and a file in a projected system measured in feet would be off by a
    /// factor of 3.28 if the first step's absence were read as "metres".
    #[must_use]
    pub fn vertical_unit_metres(&self) -> Option<f64> {
        match self {
            PointCloudCrs::Wkt(wkt) => wkt1_vertical_unit_metres(wkt),
            // **Deliberately unread for a geokey file, rather than guessed.** A geokey
            // directory states its vertical unit as `VerticalUnitsGeoKey` (4099), an
            // EPSG unit code. Reading it would be a handful of lines, and this crate
            // holds no LAS 1.0-1.3 fixture that carries one -- the five-point fixture
            // it generates declares no CRS at all -- so the code would ship untested
            // against any real file. `None` means the loader refuses such a file by
            // name, which is recoverable and honest; a wrong factor would be a silent
            // three-fold error in every height. The next fixture with a geokey vertical
            // system is what should turn this on.
            PointCloudCrs::Geokeys(_) => None,
        }
    }

    /// Whether this declaration is consistent with a baseline that claims `epsg`.
    ///
    /// **The geokey check is exact and the WKT check is deliberately weak, and the
    /// difference is stated rather than smoothed over.** A geokey directory names one
    /// horizontal code, so equality is the whole question. A WKT names an authority on
    /// every node it has -- the projected system, the geographic system beneath it, the
    /// datum, the spheroid, the prime meridian, the units -- and picking the horizontal
    /// one out needs a WKT parser this crate does not have and PROJ already is. So the
    /// WKT case asks only whether the claimed code appears in the file's WKT at all.
    /// That catches an operator who named the wrong file or the wrong code, which is
    /// what this check is for; it does not certify the code is the *horizontal* one,
    /// and it is not what makes the conversion correct. What makes the conversion
    /// correct is that [`to_local_enu`] converts from the file's own declaration and
    /// never from the baseline's claim.
    #[must_use]
    pub fn agrees_with_epsg(&self, epsg: u32) -> bool {
        match self {
            PointCloudCrs::Wkt(wkt) => wkt.contains(&format!("\"EPSG\",\"{epsg}\"")),
            PointCloudCrs::Geokeys(
                GridCrs::Projected { epsg: Some(code) } | GridCrs::Geographic { epsg: Some(code) },
            ) => u32::from(*code) == epsg,
            // A geokey directory that names no code contradicts nothing.
            PointCloudCrs::Geokeys(_) => true,
        }
    }
}

/// The metres-per-unit factor of a WKT 1 `VERT_CS` node's `UNIT`, or `None` when the
/// WKT declares no vertical system.
///
/// A deliberately small scanner rather than a WKT parser: it finds the `VERT_CS[` node,
/// walks to its matching bracket so a `UNIT` belonging to the horizontal system is
/// never mistaken for the vertical one, takes the first `UNIT[` inside it, and reads
/// the second comma-separated field as the conversion factor. WKT 1 is what the LAS 1.4
/// specification names for this VLR; a WKT 2 file (`VERTCRS`/`LENGTHUNIT`) reads as
/// `None` and is refused by name rather than mis-scaled.
fn wkt1_vertical_unit_metres(wkt: &str) -> Option<f64> {
    if let Some(start) = wkt.find("VERT_CS[") {
        // A declared vertical system is the answer, whatever it says -- including
        // `None` when its `UNIT` is malformed. Falling through to the horizontal unit
        // there would silently substitute a different unit for the one the file named.
        let node = balanced_node(&wkt[start..])?;
        let unit = node.find("UNIT[")?;
        return unit_factor(balanced_node(&node[unit..])?);
    }
    // No vertical system: the height is in the projected system's own linear unit. The
    // `UNIT` wanted is the one that is a *direct* child of `PROJCS`, never the angular
    // `UNIT["degree", ...]` nested inside the `GEOGCS` beneath it.
    let start = wkt.find("PROJCS[")?;
    let node = balanced_node(&wkt[start..])?;
    direct_child_unit(node).and_then(unit_factor)
}

/// The `UNIT[...]` node that is a direct child of `node`, skipping any nested inside a
/// sub-node such as the `GEOGCS` within a `PROJCS`.
fn direct_child_unit(node: &str) -> Option<&str> {
    let mut depth = 0usize;
    let bytes = node.as_bytes();
    for (i, c) in node.char_indices() {
        match c {
            '[' => {
                // Depth is counted *before* this bracket is opened, so a `UNIT[` whose
                // name begins at depth 1 is a direct child of the outer node and one at
                // depth 2 or more belongs to something nested inside it.
                if depth == 1 && i >= 4 && &bytes[i - 4..i] == b"UNIT" {
                    return balanced_node(&node[i - 4..]);
                }
                depth += 1;
            }
            ']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    None
}

/// The conversion factor of a `UNIT["name",factor,...]` node: the field after the
/// quoted name. `None` for anything that does not parse to a finite positive number.
fn unit_factor(unit_node: &str) -> Option<f64> {
    let inside = unit_node.strip_prefix("UNIT[")?.strip_suffix(']')?;
    let after_name = inside.split_once(',')?.1;
    let factor = after_name.split(',').next()?.trim();
    factor
        .parse::<f64>()
        .ok()
        .filter(|f| f.is_finite() && *f > 0.0)
}

/// The text from the start of `s` (which must begin `NAME[`) through the `]` that
/// closes it, brackets balanced. `None` if the brackets never balance.
fn balanced_node(s: &str) -> Option<&str> {
    let open = s.find('[')?;
    let mut depth = 0usize;
    for (i, c) in s[open..].char_indices() {
        match c {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[..=(open + i)]);
                }
            }
            _ => {}
        }
    }
    None
}

/// What a LAS header declares its coordinates to be, read from its CRS VLRs.
///
/// `Ok(None)` for a file that declares nothing, which is not an error: the five-point
/// fixture this crate generates declares nothing, and so does any LAS file written by a
/// tool that did not know its own frame.
///
/// # Errors
///
/// `DataError::Parse` when a geokey directory is present but malformed -- a truncated
/// key directory, or one whose version header the `GeoTIFF` spec forbids. A file that
/// declares no CRS at all is `Ok(None)`, never an error.
pub fn declared(
    header: &las::Header,
    path: &std::path::Path,
) -> Result<Option<PointCloudCrs>, DataError> {
    if let Some(bytes) = header.get_wkt_crs_bytes() {
        // The VLR is a null-terminated, null-padded string. Lossy rather than strict:
        // a stray byte in a CRS description must not fail a load that is otherwise
        // fine, and PROJ is what decides whether the text is a usable definition.
        let text = String::from_utf8_lossy(bytes);
        let text = text.trim_end_matches('\0').trim();
        if !text.is_empty() {
            return Ok(Some(PointCloudCrs::Wkt(text.to_string())));
        }
    }
    let geo = header.get_geotiff_crs().map_err(|e| {
        DataError::Parse(format!(
            "{}: its GeoTIFF CRS keys are malformed: {e}",
            path.display()
        ))
    })?;
    let Some(geo) = geo else {
        return Ok(None);
    };
    // The same three keys `gungnir-data`'s `GeoTIFF` DEM reader already reduces to a
    // `GridCrs` (`geospatial::georeference`), read here through the `las` crate's own
    // accessors rather than re-walking the key directory.
    let crs = match geo.get_gt_model_type_geo_key_value() {
        Some(MODEL_TYPE_PROJECTED) => GridCrs::Projected {
            epsg: geo.get_projected_crs_geo_key_value(),
        },
        Some(MODEL_TYPE_GEOGRAPHIC) => GridCrs::Geographic {
            epsg: geo.get_geodetic_crs_geo_key_value(),
        },
        _ => GridCrs::Unstated,
    };
    Ok(Some(PointCloudCrs::Geokeys(crs)))
}

/// Convert a buffer's points from the coordinate reference system `source` names into
/// the local ENU frame `to_enu` anchors, through `libproj` (D-41).
///
/// `source` is any definition PROJ accepts -- an `EPSG:<code>` string, or the file's own
/// WKT, which is what [`PointCloudCrs::proj_definition`] hands over. `vertical_metres`
/// is how many metres one unit of the source's vertical axis is. `to_enu` takes
/// `[lat_rad, lon_rad, alt_m]` and returns `[east_m, north_m, up_m]`; on the desktop
/// that is `gungnir_model::LocalFrame::to_enu`.
///
/// # What this converts, and what it deliberately does not
///
/// **The horizontal conversion is PROJ's and is complete.** Easting and northing go
/// through `proj_trans` from `source` to `EPSG:4979`, so a projected system's inverse
/// projection and whatever datum step PROJ selects for it are both applied.
///
/// **The vertical conversion is a unit scale and nothing more, and that is a limitation
/// of the pinned crate rather than a choice.** `proj` 0.31's high-level `Proj::convert`
/// sets the `z` of every coordinate it hands `proj_trans` to `0.0`, so a height cannot
/// be routed through PROJ at all from this API. What is applied instead is the metres-
/// per-unit factor the file itself declares -- arithmetic on a number in the file, not
/// an invented datum shift. **No vertical datum shift is applied**: a height above a
/// gravity-related datum such as NAVD88 stays that height, expressed in metres, and is
/// not converted to a height above the WGS-84 ellipsoid. In the Pacific Northwest that
/// separation is of the order of -22 m. This is named here, in the register, and in
/// `testdata/pointcloud/SOURCE.md` rather than left for a reader to discover. It is
/// also, for what it is worth, exactly what PROJ itself does when its optional
/// vertical-datum grids are absent, which is the state a deployment that never fetches
/// grids over the network is always in: the independent `pyproj` check recorded in
/// `gungnir-data/tests/pointcloud_crs.rs` returns the same unit-only height.
///
/// # Performance
///
/// One `proj_trans` call per point, rather than `Proj::convert_array`'s one call for the
/// whole slice. Deliberate at this size and worth revisiting at a larger one: a bounded
/// COPC query returns thousands of points, where the difference is not measurable, and
/// the per-point call is what lets a refusal name the coordinate that failed instead of
/// only the file. A caller that ever converts a whole ten-million-point file should move
/// to `convert_array` and give up that message.
///
/// # Errors
///
/// `DataError::Parse` when PROJ cannot build a transformation from `source` to
/// `EPSG:4979` (an unknown EPSG code, or a WKT it will not parse), or when a point does
/// not convert. Never a panic.
#[cfg(feature = "crs")]
pub fn to_local_enu(
    buffer: &super::PointBuffer,
    source: &str,
    vertical_metres: f64,
    to_enu: &dyn Fn([f64; 3]) -> [f64; 3],
) -> Result<super::PointBuffer, DataError> {
    // EPSG:4979 is WGS 84 three-dimensional geographic. `Proj::new_known_crs`
    // normalizes the axis order for visualisation, so the pair that comes back is
    // (longitude, latitude) in degrees rather than the authority's own (lat, lon).
    let transform = proj::Proj::new_known_crs(source, "EPSG:4979", None).map_err(|e| {
        DataError::Parse(format!(
            "cannot build a transformation from {source:?} to EPSG:4979: {e}"
        ))
    })?;
    let mut geodetic = Vec::with_capacity(buffer.positions.len());
    for p in &buffer.positions {
        let x = f64::from(p[0]) + buffer.origin[0];
        let y = f64::from(p[1]) + buffer.origin[1];
        let z = f64::from(p[2]) + buffer.origin[2];
        let (lon_deg, lat_deg) = transform.convert((x, y)).map_err(|e| {
            DataError::Parse(format!(
                "{source:?}: point ({x}, {y}) does not convert: {e}"
            ))
        })?;
        geodetic.push([
            lat_deg.to_radians(),
            lon_deg.to_radians(),
            z * vertical_metres,
        ]);
    }
    Ok(place_geodetic(buffer, &geodetic, to_enu))
}

/// The ENU half: every point placed through `to_enu`, then made relative to the cloud's
/// own minimum corner again.
///
/// **The relative-to-origin trick survives the conversion, and it has to.** The loader
/// makes positions relative to the file's minimum bound so a six-figure UTM easting
/// still resolves to well under a millimetre in an `f32`. Converting to ENU does not
/// remove that need, it moves it: a deployment whose declared origin is tens of
/// kilometres from the cloud -- or, in a system of systems, hundreds -- has ENU
/// coordinates just as large as the projected ones were. So the converted cloud is
/// re-based on its own new minimum corner, `origin` carries that corner in `f64` in
/// **ENU metres from the deployment origin**, and `positions` stay small. What changes
/// is the meaning of `origin`, not its role: it was a corner in the file's own frame
/// and it is now a corner in the deployment's.
///
/// Split out from [`to_local_enu`], and public rather than private, for two reasons:
/// the arithmetic that makes an `f32` position safe is not PROJ's and should not be
/// gated behind PROJ's build, and a caller that has geodetic coordinates from some other
/// source needs exactly this and nothing else of this module.
///
/// `geodetic` is `[lat_rad, lon_rad, alt_m]` per point, in the same order as
/// `buffer.positions`; a shorter slice places only as many points as it has.
#[must_use]
pub fn place_geodetic(
    buffer: &super::PointBuffer,
    geodetic: &[[f64; 3]],
    to_enu: &dyn Fn([f64; 3]) -> [f64; 3],
) -> super::PointBuffer {
    let enu: Vec<[f64; 3]> = geodetic.iter().map(|g| to_enu(*g)).collect();
    let mut origin = [f64::INFINITY; 3];
    for p in &enu {
        for i in 0..3 {
            origin[i] = origin[i].min(p[i]);
        }
    }
    if !origin.iter().all(|v| v.is_finite()) {
        // An empty cloud has no corner. Zero is the only honest answer and the
        // positions vector it goes with is empty, so nothing is placed against it.
        origin = [0.0; 3];
    }
    #[allow(clippy::cast_possible_truncation)]
    let positions = enu
        .iter()
        .map(|p| {
            [
                (p[0] - origin[0]) as f32,
                (p[1] - origin[1]) as f32,
                (p[2] - origin[2]) as f32,
            ]
        })
        .collect();
    super::PointBuffer {
        positions,
        intensity: buffer.intensity.clone(),
        classification: buffer.classification.clone(),
        // A normal is a direction in the frame its positions are in, and this changed
        // frames. Nothing here rotates one, and carrying the old one over would be a
        // claim about the new frame that nothing computed -- so it is dropped, the same
        // refusal `PointBuffer::normals`' own documentation already makes for inventing
        // one at load.
        normals: None,
        origin,
        // The cloud is in the deployment's frame now; it no longer carries the file's
        // own declaration, because that declaration is no longer true of these numbers.
        crs: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real WKT the vendored Autzen COPC fixture carries, transcribed from the
    /// file's own record-2112 VLR (`testdata/pointcloud/SOURCE.md`).
    const AUTZEN_WKT: &str = r#"COMPD_CS["NAD83 / Oregon GIC Lambert (ft) + NAVD88 height (ftUS)",PROJCS["NAD83 / Oregon GIC Lambert (ft)",GEOGCS["NAD83",DATUM["North_American_Datum_1983",SPHEROID["GRS 1980",6378137,298.257222101,AUTHORITY["EPSG","7019"]],AUTHORITY["EPSG","6269"]],PRIMEM["Greenwich",0,AUTHORITY["EPSG","8901"]],UNIT["degree",0.0174532925199433,AUTHORITY["EPSG","9122"]],AUTHORITY["EPSG","4269"]],PROJECTION["Lambert_Conformal_Conic_2SP"],PARAMETER["latitude_of_origin",41.75],PARAMETER["central_meridian",-120.5],PARAMETER["standard_parallel_1",43],PARAMETER["standard_parallel_2",45.5],PARAMETER["false_easting",1312335.958],PARAMETER["false_northing",0],UNIT["foot",0.3048,AUTHORITY["EPSG","9002"]],AXIS["Easting",EAST],AXIS["Northing",NORTH],AUTHORITY["EPSG","2992"]],VERT_CS["NAVD88 height (ftUS)",VERT_DATUM["North American Vertical Datum 1988",2005,AUTHORITY["EPSG","5103"]],UNIT["US survey foot",0.304800609601219,AUTHORITY["EPSG","9003"]],AXIS["Gravity-related height",UP],AUTHORITY["EPSG","6360"]]]"#;

    /// The vertical unit comes from `VERT_CS`, not from the horizontal `UNIT` that
    /// appears earlier in the same string -- which is the whole reason the scanner
    /// walks to the `VERT_CS` node first instead of taking the first `UNIT[` it sees.
    /// Autzen's two differ: international feet horizontally, US survey feet vertically.
    #[test]
    fn the_vertical_unit_is_read_from_vert_cs_and_not_from_the_horizontal_unit() {
        let crs = PointCloudCrs::Wkt(AUTZEN_WKT.to_string());
        let factor = crs
            .vertical_unit_metres()
            .expect("Autzen declares a VERT_CS");
        assert!(
            (factor - 0.304_800_609_601_219).abs() < 1e-15,
            "expected the US survey foot, got {factor}"
        );
        assert!(
            (factor - 0.3048).abs() > 1e-9,
            "the international foot is the horizontal unit and must not be picked up"
        );
    }

    /// The ordinary UTM case: no `VERT_CS`, so the height is in the projected system's
    /// own linear unit. Without this fallback every plain UTM file would be refused.
    #[test]
    fn a_wkt_with_no_vertical_system_falls_back_to_the_projected_linear_unit() {
        let metric = PointCloudCrs::Wkt(
            r#"PROJCS["WGS 84 / UTM zone 10N",GEOGCS["WGS 84",UNIT["degree",0.0174532925199433]],UNIT["metre",1],AUTHORITY["EPSG","32610"]]"#
                .to_string(),
        );
        assert_eq!(
            metric.vertical_unit_metres(),
            Some(1.0),
            "the linear UNIT that is a direct child of PROJCS, not the angular one              nested in the GEOGCS beneath it"
        );

        // The case the fallback exists to get right: a projected system in feet, where
        // reading the missing VERT_CS as "metres" would put every height out by 3.28x.
        let feet = PointCloudCrs::Wkt(
            r#"PROJCS["NAD83 / Oregon GIC Lambert (ft)",GEOGCS["NAD83",UNIT["degree",0.0174532925199433]],UNIT["foot",0.3048],AUTHORITY["EPSG","2992"]]"#
                .to_string(),
        );
        assert_eq!(feet.vertical_unit_metres(), Some(0.3048));
    }

    /// A geographic-only WKT has no linear unit to fall back to, and is refused rather
    /// than assumed to be metres.
    #[test]
    fn a_geographic_wkt_states_no_vertical_unit() {
        let crs = PointCloudCrs::Wkt(
            r#"GEOGCS["WGS 84",UNIT["degree",0.0174532925199433],AUTHORITY["EPSG","4326"]]"#
                .to_string(),
        );
        assert_eq!(crs.vertical_unit_metres(), None);
    }

    #[test]
    fn a_metric_vertical_system_reads_as_one_metre_per_unit() {
        let crs = PointCloudCrs::Wkt(
            r#"COMPD_CS["x",PROJCS["y",UNIT["foot",0.3048]],VERT_CS["EGM2008 height",UNIT["metre",1,AUTHORITY["EPSG","9001"]]]]"#
                .to_string(),
        );
        assert_eq!(crs.vertical_unit_metres(), Some(1.0));
    }

    /// Malformed input is `None` -- refused by the caller -- and never a panic, which is
    /// the rule every other parser in this crate follows. A declared `VERT_CS` whose
    /// `UNIT` does not parse stays `None` rather than falling through to the horizontal
    /// unit: substituting a different unit for the one the file named would be worse
    /// than refusing.
    #[test]
    fn malformed_wkt_yields_no_vertical_unit_rather_than_panicking() {
        for text in [
            "VERT_CS[",
            "VERT_CS[\"x\"",
            r#"VERT_CS["x",UNIT["m"]]"#,
            r#"VERT_CS["x",UNIT["m",not-a-number]]"#,
            r#"VERT_CS["x",UNIT["m",0]]"#,
            r#"VERT_CS["x",UNIT["m",-1]]"#,
            "",
        ] {
            assert_eq!(
                PointCloudCrs::Wkt(text.to_string()).vertical_unit_metres(),
                None,
                "{text:?}"
            );
        }
    }

    /// The file's own WKT is what PROJ is given, in full: a bare `EPSG:2992` would
    /// silently drop the vertical system the compound WKT states.
    #[test]
    fn the_proj_definition_of_a_wkt_file_is_the_whole_wkt() {
        let crs = PointCloudCrs::Wkt(AUTZEN_WKT.to_string());
        assert_eq!(crs.proj_definition().as_deref(), Some(AUTZEN_WKT));
    }

    #[test]
    fn a_geokey_file_is_offered_to_proj_as_an_epsg_code() {
        let crs = PointCloudCrs::Geokeys(GridCrs::Projected { epsg: Some(32610) });
        assert_eq!(crs.proj_definition().as_deref(), Some("EPSG:32610"));
        let geographic = PointCloudCrs::Geokeys(GridCrs::Geographic { epsg: Some(4326) });
        assert_eq!(geographic.proj_definition().as_deref(), Some("EPSG:4326"));
        // A directory that names no code has nothing to offer, and the loader refuses
        // it by name rather than falling back to the baseline's claim.
        let unstated = PointCloudCrs::Geokeys(GridCrs::Unstated);
        assert_eq!(unstated.proj_definition(), None);
    }

    #[test]
    fn a_geokey_code_must_match_the_baseline_exactly() {
        let crs = PointCloudCrs::Geokeys(GridCrs::Projected { epsg: Some(32610) });
        assert!(crs.agrees_with_epsg(32610));
        assert!(!crs.agrees_with_epsg(32611));
        // Nothing declared contradicts nothing.
        assert!(PointCloudCrs::Geokeys(GridCrs::Unstated).agrees_with_epsg(32610));
    }

    /// The weak half of the check, pinned as weak on purpose: 2992 is Autzen's
    /// horizontal code and passes, 32610 is unrelated and fails, and 6269 -- the
    /// *datum's* authority, not a horizontal CRS code -- also passes, which is exactly
    /// the limitation `agrees_with_epsg`'s own documentation states.
    #[test]
    fn a_wkt_is_checked_only_for_containment_of_the_claimed_code() {
        let crs = PointCloudCrs::Wkt(AUTZEN_WKT.to_string());
        assert!(crs.agrees_with_epsg(2992));
        assert!(!crs.agrees_with_epsg(32610));
        assert!(
            crs.agrees_with_epsg(6269),
            "the containment check cannot tell a datum authority from a horizontal one, \
             and its documentation says so"
        );
    }

    /// The re-basing arithmetic, without PROJ: three points placed by an identity-ish
    /// ENU closure end up relative to their own minimum corner, with `origin` carrying
    /// that corner. This is what keeps an `f32` position honest after the frame change.
    #[test]
    // Every value compared below is an exact sum or difference of the small decimals in
    // this test's own input, so equality is the assertion that means something here; an
    // epsilon would only hide a re-basing that had drifted.
    #[allow(clippy::float_cmp)]
    fn a_converted_cloud_is_re_based_on_its_own_corner_so_f32_stays_exact() {
        let buffer = super::super::PointBuffer {
            positions: vec![[0.0; 3]; 3],
            intensity: Some(vec![1.0, 2.0, 3.0]),
            classification: Some(vec![2, 2, 5]),
            normals: Some(vec![[0.0, 0.0, 1.0]; 3]),
            origin: [0.0; 3],
            crs: Some(PointCloudCrs::Wkt(AUTZEN_WKT.to_string())),
        };
        // A closure standing in for `LocalFrame::to_enu`: it just reinterprets the
        // triple, so the expected output is arithmetic a reader can check by eye.
        let far = [400_000.0_f64, 6_000_000.0, 100.0];
        let enu = |g: [f64; 3]| [far[0] + g[0], far[1] + g[1], far[2] + g[2]];
        let out = place_geodetic(
            &buffer,
            &[[0.0, 0.0, 0.0], [1.5, 2.5, 3.5], [-1.0, 4.0, 0.5]],
            &enu,
        );

        assert_eq!(out.origin, [far[0] - 1.0, far[1], far[2]]);
        assert_eq!(out.positions[0], [1.0, 0.0, 0.0]);
        assert_eq!(out.positions[1], [2.5, 2.5, 3.5]);
        assert_eq!(out.positions[2], [0.0, 4.0, 0.5]);
        assert_eq!(out.intensity, buffer.intensity, "attributes ride along");
        assert_eq!(out.classification, buffer.classification);
        assert_eq!(
            out.normals, None,
            "a normal does not survive a frame change"
        );
        assert_eq!(out.crs, None, "the file's declaration is no longer true");
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn an_empty_cloud_places_to_an_origin_of_zero_rather_than_infinity() {
        let buffer = super::super::PointBuffer::default();
        let out = place_geodetic(&buffer, &[], &|g| g);
        assert_eq!(out.origin, [0.0; 3]);
        assert!(out.positions.is_empty());
    }
}
