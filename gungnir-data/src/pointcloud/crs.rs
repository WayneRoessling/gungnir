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
//! as a closure from the caller, which is the same shape `gungnir_assessment::anchor_list`
//! already uses for exactly this reason. (`gungnir_analytics::coverage_from_registry`
//! used it too until GAP-118, when it took the `LocalFrame` itself because a sensor's
//! sector has to be turned by the frame as well as placed in it; `gungnir-analytics` can
//! reach the model, and this crate cannot.)

use crate::geospatial::GridCrs;
use crate::DataError;

/// `GTModelTypeGeoKey` value for a two-dimensional projected system.
const MODEL_TYPE_PROJECTED: u16 = 1;
/// `GTModelTypeGeoKey` value for a geographic two-dimensional system.
const MODEL_TYPE_GEOGRAPHIC: u16 = 2;
/// `ProjLinearUnitsGeoKey` (OGC `GeoTIFF` 1.1 §7.4.7; `ProjLinearUnitsGeoKey` in 1.0):
/// the linear unit of a projected system's easting and northing.
const PROJ_LINEAR_UNITS_GEO_KEY: u16 = 3076;
/// `VerticalGeoKey` (`VerticalCSTypeGeoKey` in `GeoTIFF` 1.0): the vertical CRS code.
const VERTICAL_GEO_KEY: u16 = 4096;
/// `VerticalUnitsGeoKey`: the vertical axis's linear unit code.
const VERTICAL_UNITS_GEO_KEY: u16 = 4099;

/// What a LAS 1.0-1.3 `GeoTIFF` key directory states: the horizontal system, reduced to
/// the EPSG code it names (the same [`GridCrs`] the DEM reader reduces a `GeoTIFF` to),
/// and the three keys that say what the heights are (GAP-102 item (2), GAP-197).
///
/// Every field is the key's raw value, `None` where the directory omits the key, so what
/// the file said and what this crate concluded from it stay apart:
/// [`PointCloudCrs::vertical_unit`] and [`PointCloudCrs::vertical_datum`] draw the
/// conclusions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GeokeyCrs {
    /// `GTModelTypeGeoKey` with `ProjectedCRSGeoKey` or `GeodeticCRSGeoKey`.
    pub horizontal: GridCrs,
    /// `VerticalGeoKey` (4096): an EPSG vertical CRS code, `32767` for user-defined.
    pub vertical: Option<u16>,
    /// `VerticalUnitsGeoKey` (4099): an EPSG linear unit code.
    pub vertical_units: Option<u16>,
    /// `ProjLinearUnitsGeoKey` (3076): an EPSG linear unit code.
    pub linear_units: Option<u16>,
}

impl From<GridCrs> for GeokeyCrs {
    /// A directory stating a horizontal system and nothing about its heights.
    fn from(horizontal: GridCrs) -> Self {
        GeokeyCrs {
            horizontal,
            ..GeokeyCrs::default()
        }
    }
}

/// The coordinate reference system a LAS file declares for itself, in whichever of the
/// two forms the LAS specification allows.
///
/// Deliberately not `GridCrs`, and not an extension of it: a LAS 1.4 file declares its
/// CRS as WKT, which carries a compound (horizontal + vertical) system that a
/// `GeoTIFF`
/// geokey directory reduced to one EPSG code cannot express. The geokey form carries
/// `GridCrs` for its horizontal half, reused rather than redefined, because there it is
/// the same set of keys read from the same spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PointCloudCrs {
    /// The LAS 1.4 WKT VLR: user id `LASF_Projection`, record 2112. The LAS 1.4
    /// specification names OGC WKT 1 here, which is what the parsing below assumes.
    Wkt(String),
    /// The LAS 1.0-1.3 `GeoTIFF` geokey VLRs (records 34735/34736/34737).
    Geokeys(GeokeyCrs),
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
            PointCloudCrs::Geokeys(GeokeyCrs {
                horizontal:
                    GridCrs::Projected { epsg: Some(code) } | GridCrs::Geographic { epsg: Some(code) },
                ..
            }) => Some(format!("EPSG:{code}")),
            PointCloudCrs::Geokeys(_) => None,
        }
    }

    /// How many metres one unit of the file's **vertical** axis is, when the file says;
    /// `None` when it does not, which the loader refuses. [`Self::vertical_unit`] says
    /// why.
    #[must_use]
    pub fn vertical_unit_metres(&self) -> Option<f64> {
        self.vertical_unit().ok()
    }

    /// How many metres one unit of the file's **vertical** axis is, or why the file does
    /// not say in a form this build reads.
    ///
    /// `Ok(1.0)` for a file whose heights are already metres. See [`to_local_enu`] for
    /// why this is read here at all rather than left to PROJ.
    ///
    /// **A WKT: two places, in order, and the second is not a guess.** A WKT that
    /// declares a `VERT_CS` states the vertical unit outright and that answer is final.
    /// A WKT that declares none -- which is the ordinary case for a file in a metric
    /// projected system such as a UTM zone -- leaves the height in the same linear unit
    /// as the easting and northing, so the projected system's own `UNIT` is what applies,
    /// and that is read instead. Without this second step every plain UTM file would be
    /// refused, and a file in a projected system measured in feet would be off by a
    /// factor of 3.28 if the first step's absence were read as "metres".
    ///
    /// **A geokey directory (GAP-102 item (2), GAP-197): the same two places, read from
    /// its keys.** The vertical system's unit first: `VerticalUnitsGeoKey` (4099) where
    /// the directory has it, which is what `LAStools` writes; otherwise the unit the
    /// `VerticalGeoKey` (4096) code itself fixes, which is what GDAL writes -- for
    /// EPSG:2992+6360 it writes 4096 = 6360 and no 4099, the US survey foot being part of
    /// what 6360 means ([`crate::geoid::vertical_crs_unit_metres`]). Where the directory
    /// states both and they disagree, the file contradicts itself and is refused. With no
    /// vertical key at all, the height is in the projected system's linear unit,
    /// `ProjLinearUnitsGeoKey` (3076), which GDAL and libLAS both write for a projected
    /// system; a geographic directory has no linear unit to fall back to. Only the metre
    /// and the two feet are read ([`crate::geoid::linear_unit_metres`]).
    ///
    /// # Errors
    ///
    /// The reason in words, for the loader's refusal.
    pub fn vertical_unit(&self) -> Result<f64, String> {
        use crate::geoid::{linear_unit_metres, vertical_crs_unit_metres};
        match self {
            PointCloudCrs::Wkt(wkt) => wkt1_vertical_unit_metres(wkt).ok_or_else(|| {
                "its WKT states no unit this build reads for its heights (a VERT_CS with a \
                 UNIT, or a PROJCS whose own linear UNIT applies)"
                    .to_string()
            }),
            PointCloudCrs::Geokeys(keys) => {
                let unit_key = keys
                    .vertical_units
                    .map(|code| {
                        linear_unit_metres(code).ok_or_else(|| {
                            format!(
                                "its VerticalUnitsGeoKey names unit EPSG:{code}, which is \
                                 not the metre or a foot"
                            )
                        })
                    })
                    .transpose()?;
                let implied = keys
                    .vertical
                    .and_then(|code| vertical_crs_unit_metres(u32::from(code)));
                match (keys.vertical, unit_key, implied) {
                    (Some(code), Some(stated), Some(implied))
                        if (stated - implied).abs() > 1e-12 =>
                    {
                        Err(format!(
                            "its VerticalGeoKey EPSG:{code} is in units of {implied} m and \
                             its VerticalUnitsGeoKey says {stated} m, so the file \
                             contradicts itself about its heights"
                        ))
                    }
                    (_, Some(stated), _) => Ok(stated),
                    (Some(_), None, Some(implied)) => Ok(implied),
                    (Some(code), None, None) => Err(format!(
                        "its VerticalGeoKey is EPSG:{code} with no VerticalUnitsGeoKey, and \
                         this build does not know the unit that code fixes"
                    )),
                    (None, None, _) => match (keys.horizontal, keys.linear_units) {
                        (GridCrs::Projected { .. }, Some(code)) => linear_unit_metres(code)
                            .ok_or_else(|| {
                                format!(
                                    "its ProjLinearUnitsGeoKey names unit EPSG:{code}, which \
                                     is not the metre or a foot"
                                )
                            }),
                        _ => Err("its geokeys state no unit for its heights (no \
                                  VerticalGeoKey or VerticalUnitsGeoKey, and no \
                                  ProjLinearUnitsGeoKey of a projected system)"
                            .to_string()),
                    },
                }
            }
        }
    }

    /// The vertical datum the file states for its heights, or `None` when it states none
    /// (GAP-108 and D-121; GAP-197 and D-125).
    ///
    /// **A geokey directory** states it as its `VerticalGeoKey` code, read through
    /// [`VerticalDatum::from_epsg`](crate::geoid::VerticalDatum::from_epsg).
    ///
    /// **A WKT** states it in a WKT 1 `VERT_CS` node, read in this order: a `VERT_CS`
    /// whose axis points **down** is a depth, not a height, and is refused by its name
    /// whatever its datum; a `VERT_DATUM` of type 2002 (OGC 01-009's ellipsoidal datum
    /// type) is a WGS-84 ellipsoidal height; a `VERT_DATUM` whose own authority is one
    /// of the geoid datums a grid is pinned for -- EPSG:1027 (EGM2008), 5171 (EGM96),
    /// 5103 (NAVD88) -- names that datum whatever its unit, which
    /// [`Self::vertical_unit`] reads separately; otherwise the `VERT_CS`'s own EPSG
    /// authority decides; and a `VERT_CS` with no authority at all is kept by its name,
    /// which a refusal quotes.
    ///
    /// **A WKT with no `VERT_CS`, or a directory with no `VerticalGeoKey`, states
    /// nothing**: a plain projected system says nothing about what its heights are
    /// measured from, and reading that silence as "ellipsoidal" or "above the geoid"
    /// would be the guess D-121 refuses. The baseline's `point_cloud.vertical` is where
    /// such a file's datum is declared.
    #[must_use]
    pub fn vertical_datum(&self) -> Option<crate::geoid::VerticalDatum> {
        use crate::geoid::VerticalDatum;
        let wkt = match self {
            PointCloudCrs::Wkt(wkt) => wkt,
            PointCloudCrs::Geokeys(keys) => {
                return keys
                    .vertical
                    .map(|code| VerticalDatum::from_epsg(u32::from(code)));
            }
        };
        let start = wkt.find("VERT_CS[")?;
        let node = balanced_node(&wkt[start..])?;
        let name = node
            .split_once('"')
            .and_then(|(_, rest)| rest.split_once('"'))
            .map_or("an unnamed vertical system", |(name, _)| name)
            .to_string();
        if direct_child(node, "AXIS").is_some_and(|axis| axis.contains("DOWN")) {
            return Some(VerticalDatum::Other {
                epsg: direct_child(node, "AUTHORITY").and_then(epsg_authority),
                name: format!("{name}, a depth axis rather than a height"),
            });
        }
        if let Some(datum_at) = node.find("VERT_DATUM[") {
            if let Some(datum) = balanced_node(&node[datum_at..]) {
                // VERT_DATUM["name", type, AUTHORITY[...]]: the type is the field after
                // the quoted name.
                let datum_type = datum
                    .strip_prefix("VERT_DATUM[")
                    .and_then(|inside| inside.split_once(',').map(|(_, rest)| rest))
                    .and_then(|rest| rest.split([',', ']']).next())
                    .map(str::trim);
                if datum_type == Some("2002") {
                    return Some(VerticalDatum::Ellipsoidal);
                }
                if let Some(known) = direct_child(datum, "AUTHORITY")
                    .and_then(epsg_authority)
                    .and_then(VerticalDatum::from_datum_epsg)
                {
                    return Some(known);
                }
            }
        }
        Some(
            match direct_child(node, "AUTHORITY").and_then(epsg_authority) {
                Some(code) => match VerticalDatum::from_epsg(code) {
                    VerticalDatum::Other { epsg, .. } => VerticalDatum::Other { epsg, name },
                    known => known,
                },
                None => VerticalDatum::Other { epsg: None, name },
            },
        )
    }

    /// Whether this declaration is consistent with a baseline that claims `epsg`.
    ///
    /// **Both checks are exact since GAP-102 closed.** A geokey directory names one
    /// horizontal code, so equality is the whole question. A WKT names an authority on
    /// every node it has -- the projected system, the geographic system beneath it, the
    /// datum, the spheroid, the prime meridian, the units -- and this used to ask only
    /// whether the claimed code appeared anywhere in it, which a datum's code (Autzen's
    /// NAD83 datum, EPSG:6269) passed as readily as the system's own (GAP-102 item (4)).
    /// It now reads the horizontal system's own code: the `AUTHORITY` that is a direct
    /// child of the top `PROJCS` or `GEOGCS`, or of the one that is a direct child of a
    /// `COMPD_CS` ([`wkt1_horizontal_epsg`]). A WKT whose horizontal system carries no
    /// EPSG authority contradicts nothing, the same as a geokey directory naming no
    /// code. This catches an operator who named the wrong file or the wrong code; what
    /// makes the conversion correct is still that [`to_local_enu`] converts from the
    /// file's own declaration and never from the baseline's claim.
    #[must_use]
    pub fn agrees_with_epsg(&self, epsg: u32) -> bool {
        match self {
            PointCloudCrs::Wkt(wkt) => wkt1_horizontal_epsg(wkt).is_none_or(|code| code == epsg),
            PointCloudCrs::Geokeys(GeokeyCrs {
                horizontal:
                    GridCrs::Projected { epsg: Some(code) } | GridCrs::Geographic { epsg: Some(code) },
                ..
            }) => u32::from(*code) == epsg,
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
    direct_child(node, "UNIT").and_then(unit_factor)
}

/// The EPSG code of a WKT 1 definition's **horizontal** system, or `None` when it has no
/// EPSG authority of its own: the top node when that is a `PROJCS` or `GEOGCS`, or the
/// `PROJCS` or `GEOGCS` that is a direct child of a top `COMPD_CS`, and then only the
/// `AUTHORITY` that is that node's direct child -- never a datum's, a spheroid's or a
/// unit's nested inside it.
fn wkt1_horizontal_epsg(wkt: &str) -> Option<u32> {
    let top = balanced_node(wkt.trim_start())?;
    let horizontal = if top.starts_with("COMPD_CS[") {
        direct_child(top, "PROJCS").or_else(|| direct_child(top, "GEOGCS"))?
    } else if top.starts_with("PROJCS[") || top.starts_with("GEOGCS[") {
        top
    } else {
        return None;
    };
    direct_child(horizontal, "AUTHORITY").and_then(epsg_authority)
}

/// The `NAME[...]` node that is a direct child of `node`, skipping any nested inside a
/// sub-node such as the `GEOGCS` within a `PROJCS` or the `VERT_DATUM` within a
/// `VERT_CS`.
fn direct_child<'a>(node: &'a str, name: &str) -> Option<&'a str> {
    let mut depth = 0usize;
    let bytes = node.as_bytes();
    let name = name.as_bytes();
    for (i, c) in node.char_indices() {
        match c {
            '[' => {
                // Depth is counted *before* this bracket is opened, so a `NAME[` whose
                // name begins at depth 1 is a direct child of the outer node and one at
                // depth 2 or more belongs to something nested inside it. The byte before
                // the name must end the previous field, so `UNIT` is never read out of
                // the tail of a longer keyword.
                if depth == 1
                    && i > name.len()
                    && &bytes[i - name.len()..i] == name
                    && matches!(bytes[i - name.len() - 1], b',' | b'[' | b' ')
                {
                    return balanced_node(&node[i - name.len()..]);
                }
                depth += 1;
            }
            ']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    None
}

/// The code of an `AUTHORITY["EPSG","<code>"]` node; `None` for another authority or a
/// code that is not a whole number.
fn epsg_authority(node: &str) -> Option<u32> {
    let inside = node.strip_prefix("AUTHORITY[")?.strip_suffix(']')?;
    let (authority, code) = inside.split_once(',')?;
    if authority.trim().trim_matches('"') != "EPSG" {
        return None;
    }
    code.trim().trim_matches('"').parse().ok()
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
    Ok(Some(PointCloudCrs::Geokeys(geokey_crs(&geo))))
}

/// A `GeoTIFF` key directory as [`GeokeyCrs`]: the same three horizontal keys
/// `gungnir-data`'s `GeoTIFF` DEM reader already reduces to a `GridCrs`
/// (`geospatial::georeference`), read through the `las` crate's own accessors, and the
/// three unit and vertical keys read from its entries directly (the crate has an accessor
/// for `VerticalGeoKey` and none for the two unit keys). A key stored anywhere but inline
/// as a `SHORT` is not a code, and reads as absent.
#[must_use]
pub fn geokey_crs(geo: &las::crs::GeoTiffCrs) -> GeokeyCrs {
    let key = |id: u16| {
        geo.entries
            .iter()
            .find(|entry| entry.id == id)
            .and_then(|entry| match entry.data {
                las::crs::GeoTiffData::U16(value) => Some(value),
                _ => None,
            })
    };
    let horizontal = match geo.get_gt_model_type_geo_key_value() {
        Some(MODEL_TYPE_PROJECTED) => GridCrs::Projected {
            epsg: geo.get_projected_crs_geo_key_value(),
        },
        Some(MODEL_TYPE_GEOGRAPHIC) => GridCrs::Geographic {
            epsg: geo.get_geodetic_crs_geo_key_value(),
        },
        _ => GridCrs::Unstated,
    };
    GeokeyCrs {
        horizontal,
        vertical: key(VERTICAL_GEO_KEY),
        vertical_units: key(VERTICAL_UNITS_GEO_KEY),
        linear_units: key(PROJ_LINEAR_UNITS_GEO_KEY),
    }
}

/// Convert a buffer's points from the coordinate reference system `source` names into
/// the local ENU frame `to_enu` anchors, through `libproj` (D-41).
///
/// `source` is any definition PROJ accepts -- an `EPSG:<code>` string, or the file's own
/// WKT, which is what [`PointCloudCrs::proj_definition`] hands over. `vertical_metres`
/// is how many metres one unit of the source's vertical axis is. `heights` is how a
/// height in metres becomes a WGS-84 ellipsoidal one, which
/// [`crate::geoid::height_reference`] decides from what the file and the baseline state.
/// `to_enu` takes `[lat_rad, lon_rad, alt_m]` and returns `[east_m, north_m, up_m]`; on
/// the desktop that is `gungnir_model::LocalFrame::to_enu`.
///
/// # What this converts
///
/// **The horizontal conversion is PROJ's and is complete.** Easting and northing go
/// through `proj_trans` from `source` to `EPSG:4979`, so a projected system's inverse
/// projection and whatever datum step PROJ selects for it are both applied.
///
/// **The vertical conversion is the file's unit, then the vertical datum (GAP-108 and
/// D-121; GAP-197 and D-125).** The height is first scaled by the metres-per-unit factor
/// the file declares. It is then made an ellipsoidal height under `heights`: unchanged
/// when it already is one, and with the undulation of its datum's pinned geoid grid at
/// the point's own longitude and latitude added when it is an EGM2008, EGM96 or NAVD88
/// height, read from the verified grid by PROJ ([`crate::geoid::undulations`]); a point
/// outside that grid -- NAVD88 outside the conterminous United States -- fails the whole
/// conversion by name. A height in any other datum never reaches this function:
/// `height_reference` refuses it by name. (Before GAP-108 no datum shift was applied at
/// all, so a NAVD88 height was used as though it were ellipsoidal -- about 23 m high at
/// the Autzen fixture. `proj` 0.31's `Proj::convert` still zeroes the `z` it hands
/// `proj_trans`, which is why the height does not ride the horizontal transform and the
/// geoid is applied as its own step.)
///
/// **A NAVD88 height lands at the metre level, not the centimetre.** GEOID18 makes it a
/// NAD83(2011) ellipsoidal height, and the horizontal half likewise converts a NAD83
/// latitude and longitude with PROJ's null NAD83-to-WGS-84 step; the metre or two
/// between the two frames is not modelled on either axis (`crate::geoid`'s module
/// documentation has the figures).
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
/// `EPSG:4979` (an unknown EPSG code, or a WKT it will not parse), when a point does
/// not convert, or when the geoid grid has no undulation for it. Never a panic.
#[cfg(feature = "crs")]
pub fn to_local_enu(
    buffer: &super::PointBuffer,
    source: &str,
    vertical_metres: f64,
    heights: &crate::geoid::HeightReference,
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
    let mut lon_lat_deg = Vec::with_capacity(buffer.positions.len());
    let mut heights_m = Vec::with_capacity(buffer.positions.len());
    for p in &buffer.positions {
        let x = f64::from(p[0]) + buffer.origin[0];
        let y = f64::from(p[1]) + buffer.origin[1];
        let z = f64::from(p[2]) + buffer.origin[2];
        let (lon_deg, lat_deg) = transform.convert((x, y)).map_err(|e| {
            DataError::Parse(format!(
                "{source:?}: point ({x}, {y}) does not convert: {e}"
            ))
        })?;
        lon_lat_deg.push([lon_deg, lat_deg]);
        heights_m.push(z * vertical_metres);
    }
    let ellipsoidal = crate::geoid::ellipsoidal_heights(heights, &lon_lat_deg, &heights_m)?;
    let geodetic: Vec<[f64; 3]> = lon_lat_deg
        .iter()
        .zip(ellipsoidal)
        .map(|(&[lon_deg, lat_deg], h)| [lat_deg.to_radians(), lon_deg.to_radians(), h])
        .collect();
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

    /// GAP-108, GAP-197: the vertical datum each form of WKT states. Autzen's is NAVD88
    /// (its `VERT_DATUM` authority is EPSG:5103), in US survey feet, which the unit reader
    /// takes separately; the UTM + EGM2008 WKT is the one GDAL writes for EPSG:32633+3855
    /// (`pyproj`'s `to_wkt("WKT1_GDAL")`, transcribed); a datum of type 2002 is OGC
    /// 01-009's ellipsoidal type.
    #[test]
    fn the_vertical_datum_is_read_from_vert_cs_and_silence_stays_silence() {
        use crate::geoid::VerticalDatum;
        assert_eq!(
            PointCloudCrs::Wkt(AUTZEN_WKT.to_string()).vertical_datum(),
            Some(VerticalDatum::Navd88)
        );
        // The EGM96 form GDAL writes for EPSG:32633+5773 (`pyproj`, transcribed): the
        // datum authority is EPSG:5171.
        let egm96 = r#"COMPD_CS["WGS 84 / UTM zone 33N + EGM96 height",PROJCS["WGS 84 / UTM zone 33N",UNIT["metre",1,AUTHORITY["EPSG","9001"]],AUTHORITY["EPSG","32633"]],VERT_CS["EGM96 height",VERT_DATUM["EGM96 geoid",2005,AUTHORITY["EPSG","5171"]],UNIT["metre",1,AUTHORITY["EPSG","9001"]],AXIS["Gravity-related height",UP],AUTHORITY["EPSG","5773"]]]"#;
        assert_eq!(
            PointCloudCrs::Wkt(egm96.to_string()).vertical_datum(),
            Some(VerticalDatum::Egm96)
        );
        // A depth below NAVD88 (EPSG:6357) is not a height, whatever its datum: it is
        // refused by name rather than converted upside down.
        let depth = r#"COMPD_CS["x",PROJCS["y",UNIT["metre",1]],VERT_CS["NAVD88 depth",VERT_DATUM["North American Vertical Datum 1988",2005,AUTHORITY["EPSG","5103"]],UNIT["metre",1],AXIS["Gravity-related depth",DOWN],AUTHORITY["EPSG","6357"]]]"#;
        let Some(VerticalDatum::Other { epsg, name }) =
            PointCloudCrs::Wkt(depth.to_string()).vertical_datum()
        else {
            panic!("a depth is refused");
        };
        assert_eq!(epsg, Some(6357));
        assert!(name.contains("depth"), "{name}");
        let egm2008 = r#"COMPD_CS["WGS 84 / UTM zone 33N + EGM2008 height",PROJCS["WGS 84 / UTM zone 33N",GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563,AUTHORITY["EPSG","7030"]],AUTHORITY["EPSG","6326"]],PRIMEM["Greenwich",0,AUTHORITY["EPSG","8901"]],UNIT["degree",0.0174532925199433,AUTHORITY["EPSG","9122"]],AUTHORITY["EPSG","4326"]],PROJECTION["Transverse_Mercator"],PARAMETER["latitude_of_origin",0],PARAMETER["central_meridian",15],PARAMETER["scale_factor",0.9996],PARAMETER["false_easting",500000],PARAMETER["false_northing",0],UNIT["metre",1,AUTHORITY["EPSG","9001"]],AXIS["Easting",EAST],AXIS["Northing",NORTH],AUTHORITY["EPSG","32633"]],VERT_CS["EGM2008 height",VERT_DATUM["EGM2008 geoid",2005,AUTHORITY["EPSG","1027"]],UNIT["metre",1,AUTHORITY["EPSG","9001"]],AXIS["Gravity-related height",UP],AUTHORITY["EPSG","3855"]]]"#;
        assert_eq!(
            PointCloudCrs::Wkt(egm2008.to_string()).vertical_datum(),
            Some(VerticalDatum::Egm2008)
        );
        let ellipsoidal = r#"COMPD_CS["x",PROJCS["y",UNIT["metre",1]],VERT_CS["ellipsoidal height",VERT_DATUM["Ellipsoid",2002],UNIT["metre",1]]]"#;
        assert_eq!(
            PointCloudCrs::Wkt(ellipsoidal.to_string()).vertical_datum(),
            Some(VerticalDatum::Ellipsoidal)
        );
        let unnamed_authority = r#"COMPD_CS["x",PROJCS["y",UNIT["metre",1]],VERT_CS["Local datum",VERT_DATUM["Local",2005],UNIT["metre",1]]]"#;
        assert_eq!(
            PointCloudCrs::Wkt(unnamed_authority.to_string()).vertical_datum(),
            Some(VerticalDatum::Other {
                epsg: None,
                name: "Local datum".to_string()
            })
        );
        // No VERT_CS, and a geokey directory: nothing stated, which is not the same as
        // ellipsoidal and is not read as it.
        let plain_utm = r#"PROJCS["WGS 84 / UTM zone 10N",GEOGCS["WGS 84",UNIT["degree",0.0174532925199433]],UNIT["metre",1],AUTHORITY["EPSG","32610"]]"#;
        assert_eq!(
            PointCloudCrs::Wkt(plain_utm.to_string()).vertical_datum(),
            None
        );
        assert_eq!(
            PointCloudCrs::Geokeys(GridCrs::Projected { epsg: Some(32610) }.into())
                .vertical_datum(),
            None
        );
        // A geokey directory states its datum by its VerticalGeoKey code.
        assert_eq!(
            PointCloudCrs::Geokeys(GeokeyCrs {
                vertical: Some(6360),
                ..GridCrs::Projected { epsg: Some(2992) }.into()
            })
            .vertical_datum(),
            Some(VerticalDatum::Navd88)
        );
    }

    /// The key directory GDAL 3.11.3 writes for `-a_srs EPSG:2992+6360` (a `GeoTIFF` 1.1
    /// directory, transcribed from the file `gdal_create` wrote): 4096 = 6360 and no unit
    /// key at all. The same directory is what `testdata/pointcloud/autzen-geokeys.las`
    /// carries.
    fn gdal_2992_6360() -> las::crs::GeoTiffCrs {
        use las::crs::{GeoTiffCrs, GeoTiffData, GeoTiffKeyEntry};
        let short = |id, value| GeoTiffKeyEntry {
            id,
            data: GeoTiffData::U16(value),
        };
        GeoTiffCrs {
            entries: vec![
                short(1024, 1),
                short(1025, 1),
                GeoTiffKeyEntry {
                    id: 1026,
                    data: GeoTiffData::String(
                        "NAD83 / Oregon GIC Lambert (ft) + NAVD88 height (ftUS)|".to_string(),
                    ),
                },
                short(3072, 2992),
                short(4096, 6360),
            ],
        }
    }

    /// GAP-102 item (2): the geokey form's vertical unit, from each of the places a real
    /// writer puts it. The US survey foot comes from the code 6360 itself in GDAL's
    /// directory; the international foot from `ProjLinearUnitsGeoKey` in the one GDAL
    /// writes for a bare EPSG:2992 (3076 = 9002) and libLAS wrote for its own Autzen LAS
    /// 1.2 sample; the metre from `VerticalUnitsGeoKey` beside NN2000 (4096 = 5941,
    /// 4099 = 9001), as `LAStools` writes it.
    #[test]
    // Each factor is a defining constant passed through, so equality is the property.
    #[allow(clippy::float_cmp)]
    fn a_geokey_directory_states_its_vertical_unit_where_real_writers_put_it() {
        let gdal = geokey_crs(&gdal_2992_6360());
        assert_eq!(
            gdal,
            GeokeyCrs {
                horizontal: GridCrs::Projected { epsg: Some(2992) },
                vertical: Some(6360),
                vertical_units: None,
                linear_units: None,
            }
        );
        assert_eq!(
            PointCloudCrs::Geokeys(gdal).vertical_unit(),
            Ok(1200.0 / 3937.0)
        );

        let horizontal_only = GeokeyCrs {
            linear_units: Some(9002),
            ..GridCrs::Projected { epsg: Some(2992) }.into()
        };
        assert_eq!(
            PointCloudCrs::Geokeys(horizontal_only).vertical_unit(),
            Ok(0.3048)
        );

        let lastools = GeokeyCrs {
            horizontal: GridCrs::Projected { epsg: Some(25832) },
            vertical: Some(5941),
            vertical_units: Some(9001),
            linear_units: Some(9001),
        };
        assert_eq!(PointCloudCrs::Geokeys(lastools).vertical_unit(), Ok(1.0));
    }

    /// And where the directory does not say, or says something contradictory or unread,
    /// the file is refused with the reason rather than read as metres.
    #[test]
    fn a_geokey_directory_that_does_not_state_a_readable_unit_is_refused_with_the_reason() {
        for (keys, reason) in [
            // A horizontal system with no unit key of any kind.
            (
                GeokeyCrs::from(GridCrs::Projected { epsg: Some(32610) }),
                "state no unit",
            ),
            // A geographic system has no linear unit to fall back to.
            (
                GeokeyCrs {
                    linear_units: Some(9001),
                    ..GridCrs::Geographic { epsg: Some(4326) }.into()
                },
                "state no unit",
            ),
            // NN2000 alone: a code this build does not know the unit of.
            (
                GeokeyCrs {
                    vertical: Some(5941),
                    ..GridCrs::Projected { epsg: Some(25832) }.into()
                },
                "EPSG:5941",
            ),
            // NAVD88 in US survey feet, with a unit key saying metres.
            (
                GeokeyCrs {
                    vertical: Some(6360),
                    vertical_units: Some(9001),
                    ..GridCrs::Projected { epsg: Some(2992) }.into()
                },
                "contradicts itself",
            ),
            // A unit this build does not read: the kilometre.
            (
                GeokeyCrs {
                    vertical: Some(5703),
                    vertical_units: Some(9036),
                    ..GridCrs::Projected { epsg: Some(26910) }.into()
                },
                "EPSG:9036",
            ),
        ] {
            let err = PointCloudCrs::Geokeys(keys)
                .vertical_unit()
                .expect_err("no readable unit");
            assert!(err.contains(reason), "{keys:?}: {err}");
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
        let crs = PointCloudCrs::Geokeys(GridCrs::Projected { epsg: Some(32610) }.into());
        assert_eq!(crs.proj_definition().as_deref(), Some("EPSG:32610"));
        let geographic = PointCloudCrs::Geokeys(GridCrs::Geographic { epsg: Some(4326) }.into());
        assert_eq!(geographic.proj_definition().as_deref(), Some("EPSG:4326"));
        // A directory that names no code has nothing to offer, and the loader refuses
        // it by name rather than falling back to the baseline's claim.
        let unstated = PointCloudCrs::Geokeys(GridCrs::Unstated.into());
        assert_eq!(unstated.proj_definition(), None);
    }

    #[test]
    fn a_geokey_code_must_match_the_baseline_exactly() {
        let crs = PointCloudCrs::Geokeys(GridCrs::Projected { epsg: Some(32610) }.into());
        assert!(crs.agrees_with_epsg(32610));
        assert!(!crs.agrees_with_epsg(32611));
        // Nothing declared contradicts nothing.
        assert!(PointCloudCrs::Geokeys(GridCrs::Unstated.into()).agrees_with_epsg(32610));
    }

    /// GAP-102 item (4), closed: the WKT check reads the horizontal system's own code. 2992
    /// is Autzen's horizontal code and passes; 32610 is unrelated and fails; and 6269 (the
    /// NAD83 *datum*), 4269 (the geographic system beneath the projection), 9002 (its
    /// unit) and 6360 (the vertical system) all appear in the same WKT and all fail now,
    /// where the old containment check passed every one of them.
    #[test]
    fn a_wkt_is_checked_against_its_horizontal_systems_own_code() {
        let crs = PointCloudCrs::Wkt(AUTZEN_WKT.to_string());
        assert!(crs.agrees_with_epsg(2992));
        for other in [32610, 6269, 4269, 9002, 6360, 5103] {
            assert!(!crs.agrees_with_epsg(other), "EPSG:{other}");
        }
        // A plain PROJCS and a plain GEOGCS read their own direct AUTHORITY.
        let utm = PointCloudCrs::Wkt(
            r#"PROJCS["WGS 84 / UTM zone 10N",GEOGCS["WGS 84",AUTHORITY["EPSG","4326"]],UNIT["metre",1,AUTHORITY["EPSG","9001"]],AUTHORITY["EPSG","32610"]]"#
                .to_string(),
        );
        assert!(utm.agrees_with_epsg(32610));
        assert!(!utm.agrees_with_epsg(4326));
        let geographic = PointCloudCrs::Wkt(
            r#"GEOGCS["WGS 84",DATUM["WGS_1984",AUTHORITY["EPSG","6326"]],AUTHORITY["EPSG","4326"]]"#
                .to_string(),
        );
        assert!(geographic.agrees_with_epsg(4326));
        assert!(!geographic.agrees_with_epsg(6326));
        // A horizontal system with no EPSG authority of its own contradicts nothing.
        let unnamed = PointCloudCrs::Wkt(
            r#"PROJCS["local",GEOGCS["x",DATUM["d",AUTHORITY["EPSG","6326"]]],UNIT["metre",1]]"#
                .to_string(),
        );
        assert!(unnamed.agrees_with_epsg(32610));
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
