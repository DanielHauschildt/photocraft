//! `photocraft-codecs`: symmetric, depth- and metadata-preserving codecs for
//! flat raster image formats.
//!
//! * Every format we **write** we can also **read** (see
//!   [`ASYMMETRIC_EXCEPTIONS`] for the documented, feature-gated exception).
//! * Bit depth (8/16-bit integer, 16/32-bit float), ICC profiles and basic
//!   metadata (EXIF, XMP, DPI, text) are preserved where the format allows.
//! * [`caps`] declares per-format capabilities and [`fidelity_warnings`]
//!   tells the UI exactly what an export will lose.
//! * The API is I/O free (`&[u8]` in, `Vec<u8>` out) and builds for
//!   `wasm32-unknown-unknown`. Decoders enforce [`Limits`] before allocating.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod codecs;
mod error;
mod fidelity;
mod format;
mod image;
mod options;
pub mod orientation;
pub mod web;

pub use crate::codecs::png::encode_indexed as encode_png_indexed;
pub use crate::codecs::tiff::{PhotoshopTags as TiffPhotoshopTags, photoshop_tags as tiff_photoshop_tags, writes_little_endian as tiff_writes_little_endian};
pub use crate::codecs::tiff_ifd::{TiffInfo, TiffPage, TiffPageKind};
pub use crate::error::CodecError;
pub use crate::fidelity::{FidelityWarning, fidelity_warnings, fidelity_warnings_with};
pub use crate::format::{ASYMMETRIC_EXCEPTIONS, Format, FormatCaps, caps, detect, from_extension};
pub use crate::image::{ChannelLayout, DecodeWarning, DeepChannel, DeepImage, Image, Metadata, SampleType};
pub use crate::options::{DecodeOptions, EncodeOptions, ExrCompression, Limits, PngCompression, TiffCompression};
pub use crate::orientation::{exif_orientation, upright_exif, upright_xmp};
pub use half::f16;

use crate::codecs::{exr, heif, jpeg, png, pnm, tiff, via_image, webp};

/// Detect the format and decode with default [`Limits`].
pub fn decode(bytes: &[u8]) -> Result<Image, CodecError> {
    decode_with(bytes, &DecodeOptions::default())
}

/// Detect the format and decode.
pub fn decode_with(bytes: &[u8], opts: &DecodeOptions) -> Result<Image, CodecError> {
    let format = detect(bytes).ok_or(CodecError::UnknownFormat)?;
    decode_as_with(format, bytes, opts)
}

/// Decode as a specific format with default [`Limits`].
pub fn decode_as(format: Format, bytes: &[u8]) -> Result<Image, CodecError> {
    decode_as_with(format, bytes, &DecodeOptions::default())
}

/// Decode as a specific format.
///
/// Dispatches on `format` at run time, so it links every decoder. A caller that knows its format
/// at compile time can call that format's `decode_<format>_with` instead and link only its decoder.
pub fn decode_as_with(format: Format, bytes: &[u8], opts: &DecodeOptions) -> Result<Image, CodecError> {
    match format {
        Format::Png => decode_png_with(bytes, opts),
        Format::Jpeg => decode_jpeg_with(bytes, opts),
        Format::Tiff => decode_tiff_with(bytes, opts),
        Format::WebP => decode_webp_with(bytes, opts),
        Format::Pnm => decode_pnm_with(bytes, opts),
        Format::OpenExr => decode_exr_with(bytes, opts),
        Format::Heif => decode_heif_with(bytes, opts),
        Format::Gif => decode_gif_with(bytes, opts),
        Format::Bmp => decode_bmp_with(bytes, opts),
        Format::Tga => decode_tga_with(bytes, opts),
        Format::Ico => decode_ico_with(bytes, opts),
        Format::Qoi => decode_qoi_with(bytes, opts),
        Format::Hdr => decode_hdr_with(bytes, opts),
        Format::Avif => decode_one(Format::Avif, bytes, opts, |b, l| via_image::decode(Format::Avif, b, l), exif_or_1),
    }
}

/// The steps every decode shares: the read capability check, the decoder, the limits check and
/// the orientation. `decoder` and `orientation` are concrete per format, so a caller links only them.
#[inline(always)]
fn decode_one(
    format: Format,
    bytes: &[u8],
    opts: &DecodeOptions,
    decoder: impl FnOnce(&[u8], &Limits) -> Result<Image, CodecError>,
    orientation: fn(&[u8], &Image) -> u16,
) -> Result<Image, CodecError> {
    // HEIF without the `heif` feature: its decoder stub says what is missing from this build.
    if !caps(format).read && format != Format::Heif {
        return Err(CodecError::unsupported(format, "decoding is not available for this format"));
    }
    let img = decoder(bytes, &opts.limits)?;
    // Turn the pixels upright, like Photoshop: a TIFF records it in the decoded page's own
    // directory, the others in their EXIF block. The metadata is rewritten to Orientation = 1
    // on the way. HEIF keeps it in its container, and its decoder has already applied it.
    let o = orientation(bytes, &img);
    finish_decode(img, o, opts)
}

/// The orientation in the decoded image's EXIF block, 1 without one.
fn exif_or_1(_bytes: &[u8], img: &Image) -> u16 {
    img.meta.exif.as_deref().map_or(1, exif_orientation)
}

macro_rules! decode_fns {
    ($($(#[$doc:meta])* $name:ident => $format:expr, $decoder:expr, $orientation:expr;)*) => {$(
        $(#[$doc])*
        ///
        /// The same as [`decode_as_with`] for this format, but links only this format's decoder.
        pub fn $name(bytes: &[u8], opts: &DecodeOptions) -> Result<Image, CodecError> {
            decode_one($format, bytes, opts, $decoder, $orientation)
        }
    )*};
}

/// Decode a HEIF/HEIC file (an "unsupported" error without the `heif` feature).
///
/// The same as [`decode_as_with`] for this format, but links only this format's decoder.
pub fn decode_heif_with(bytes: &[u8], opts: &DecodeOptions) -> Result<Image, CodecError> {
    let keep = opts.keep_orientation;
    decode_one(Format::Heif, bytes, opts, |b, l| heif::decode(b, l, keep), |_, _| 1)
}

decode_fns! {
    /// Decode a PNG.
    decode_png_with => Format::Png, png::decode, exif_or_1;
    /// Decode a JPEG.
    decode_jpeg_with => Format::Jpeg, jpeg::decode, exif_or_1;
    /// Decode the default page of a TIFF or BigTIFF file.
    decode_tiff_with => Format::Tiff, tiff::decode, |b, _| tiff::orientation(b, None);
    /// Decode a WebP.
    decode_webp_with => Format::WebP, webp::decode, exif_or_1;
    /// Decode a Netpbm file (PBM, PGM, PPM, PAM, PFM).
    decode_pnm_with => Format::Pnm, pnm::decode, exif_or_1;
    /// Decode an OpenEXR file.
    decode_exr_with => Format::OpenExr, exr::decode, exif_or_1;
    /// Decode a GIF (its first frame).
    decode_gif_with => Format::Gif, via_image::decode_gif, exif_or_1;
    /// Decode a BMP.
    decode_bmp_with => Format::Bmp, via_image::decode_bmp, exif_or_1;
    /// Decode a TGA.
    decode_tga_with => Format::Tga, via_image::decode_tga, exif_or_1;
    /// Decode an ICO (its largest entry).
    decode_ico_with => Format::Ico, via_image::decode_ico, exif_or_1;
    /// Decode a QOI.
    decode_qoi_with => Format::Qoi, via_image::decode_qoi, exif_or_1;
    /// Decode a Radiance HDR file.
    decode_hdr_with => Format::Hdr, via_image::decode_hdr, exif_or_1;
}

/// Lists the pages (image directories) of a TIFF or BigTIFF file without decoding pixels: the
/// main IFD chain and its SubIFDs, each with its size, kind (page, reduced-resolution copy or
/// mask) and storage. [`TiffInfo::default_page`] is the page [`decode`] opens.
pub fn tiff_info(bytes: &[u8]) -> Result<TiffInfo, CodecError> {
    crate::codecs::tiff_ifd::tiff_info(bytes)
}

/// Decodes page `page` (an index into [`tiff_info`]'s `pages`) of a TIFF or BigTIFF file,
/// applying that page's orientation unless `opts.keep_orientation`.
pub fn decode_tiff_page(bytes: &[u8], page: usize, opts: &DecodeOptions) -> Result<Image, CodecError> {
    let img = tiff::decode_page(bytes, Some(page), &opts.limits)?;
    finish_decode(img, tiff::orientation(bytes, Some(page)), opts)
}

/// The Orientation (1–8) of the page [`decode`] opens from a TIFF or BigTIFF file; 1 when it is
/// absent, malformed or out of range.
pub fn tiff_orientation(bytes: &[u8]) -> u16 {
    tiff::orientation(bytes, None)
}

/// The Orientation (1–8) of page `page` (an index into [`tiff_info`]'s `pages`); 1 when it is
/// absent, malformed or out of range, or the page does not exist.
pub fn tiff_page_orientation(bytes: &[u8], page: usize) -> u16 {
    tiff::orientation(bytes, Some(page))
}

/// The limits check and the orientation, shared by every decoder.
fn finish_decode(img: Image, o: u16, opts: &DecodeOptions) -> Result<Image, CodecError> {
    let l = &opts.limits;
    // Final guard for decoders whose header we could not pre-inspect.
    l.check(img.width(), img.height(), img.layout(), img.sample_type())?;
    if opts.keep_orientation {
        return Ok(img);
    }
    // Orientations 5–8 swap width and height: with asymmetric limits a stored landscape
    // that fit can turn into a portrait that does not, so check the upright size too.
    if (5..=8).contains(&o) {
        l.check(img.height(), img.width(), img.layout(), img.sample_type())?;
    }
    img.oriented(o)
}

/// Deep OpenEXR samples (`deepscanline`/`deeptile`): the structured per-pixel sample lists
/// (colour, alpha and Z per sample), for callers that composite deep data themselves —
/// [`decode_as_with`] instead flattens a deep file into a normal [`Image`] with a
/// [`DecodeWarning::DeepFlattened`]. `Err(Unsupported)` when the file has no deep part.
pub fn decode_deep_exr(bytes: &[u8], limits: &Limits) -> Result<DeepImage, CodecError> {
    let mut cur = std::io::Cursor::new(bytes);
    let meta = ::exr::meta::MetaData::read_from_buffered(&mut cur, false).map_err(|e| CodecError::malformed(Format::OpenExr, e))?;
    codecs::deep_exr::decode_deep(&meta, bytes, limits)
}

/// Encode `image` as `format`. Conversions follow the same plan that
/// [`fidelity_warnings_with`] reports; fatal conditions (unsupported
/// format, oversize image) return an error.
pub fn encode(image: &Image, format: Format, opts: &EncodeOptions) -> Result<Vec<u8>, CodecError> {
    if !caps(format).write {
        return Err(CodecError::unsupported(format, "encoding is not available in this build"));
    }
    if image.width() == 0 || image.height() == 0 {
        return Err(CodecError::InvalidImage("cannot encode an empty image".into()));
    }
    if let Some((mw, mh)) = format.max_dimensions()
        && (image.width() > mw || image.height() > mh)
    {
        return Err(CodecError::encode(format, format!("dimensions exceed {mw}x{mh}")));
    }
    let plan = fidelity::plan(image, format, opts);
    match format {
        Format::Png => png::encode(image, plan, opts),
        Format::Jpeg => jpeg::encode(image, plan, opts),
        Format::Tiff => tiff::encode(image, plan, opts),
        Format::WebP => webp::encode(image, plan, opts),
        Format::Pnm => pnm::encode(image, plan, opts),
        Format::OpenExr => exr::encode(image, plan, opts),
        Format::Heif => Err(CodecError::unsupported(format, "encoding is not available in this build")),
        Format::Gif | Format::Bmp | Format::Tga | Format::Ico | Format::Qoi | Format::Hdr | Format::Avif => via_image::encode(format, image, plan, opts),
    }
}
