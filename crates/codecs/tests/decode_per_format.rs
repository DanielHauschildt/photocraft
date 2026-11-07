//! Every `decode_<format>_with` gives what `decode_as_with` gives for that format: the same
//! image, or the same error, for a valid file of each writable format and for garbage.

mod common;
use common::*;
use photocraft_codecs::*;

type DecodeFn = fn(&[u8], &DecodeOptions) -> Result<Image, CodecError>;

fn per_format(format: Format) -> Option<DecodeFn> {
    Some(match format {
        Format::Png => decode_png_with,
        Format::Jpeg => decode_jpeg_with,
        Format::Tiff => decode_tiff_with,
        Format::WebP => decode_webp_with,
        Format::Pnm => decode_pnm_with,
        Format::OpenExr => decode_exr_with,
        Format::Heif => decode_heif_with,
        Format::Gif => decode_gif_with,
        Format::Bmp => decode_bmp_with,
        Format::Tga => decode_tga_with,
        Format::Ico => decode_ico_with,
        Format::Qoi => decode_qoi_with,
        Format::Hdr => decode_hdr_with,
        Format::Avif => return None,
    })
}

fn same(format: Format, bytes: &[u8], opts: &DecodeOptions) {
    let Some(f) = per_format(format) else { return };
    let a = decode_as_with(format, bytes, opts);
    let b = f(bytes, opts);
    match (&a, &b) {
        (Ok(a), Ok(b)) => assert_eq!(a, b, "{format:?}: images differ"),
        (Err(a), Err(b)) => assert_eq!(a.to_string(), b.to_string(), "{format:?}: errors differ"),
        _ => panic!("{format:?}: decode_as_with gave {a:?}, the per-format function {b:?}"),
    }
}

#[test]
fn per_format_functions_match_decode_as_with() {
    let img = synth(9, 7, ChannelLayout::Rgba, SampleType::U8, 3, 0.0);
    for format in Format::ALL {
        for keep_orientation in [false, true] {
            let opts = DecodeOptions { keep_orientation, ..DecodeOptions::default() };
            if caps(format).write {
                let bytes = encode(&img, format, &EncodeOptions::default()).expect("encodes");
                same(format, &bytes, &opts);
                same(format, &bytes[..bytes.len() / 2], &opts);
            }
            same(format, b"not an image at all", &opts);
            same(format, &[], &opts);
        }
    }
}

#[test]
fn per_format_functions_apply_the_limits() {
    let img = synth(64, 64, ChannelLayout::Rgb, SampleType::U8, 3, 0.0);
    let mut opts = DecodeOptions::default();
    opts.limits.max_width = 16;
    for format in Format::ALL {
        if !caps(format).write || per_format(format).is_none() {
            continue;
        }
        let bytes = encode(&img, format, &EncodeOptions::default()).expect("encodes");
        let r = per_format(format).expect("checked")(&bytes, &opts);
        assert!(r.is_err(), "{format:?}: a 64-pixel-wide image passed a 16-pixel limit");
        same(format, &bytes, &opts);
    }
}
