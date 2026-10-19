mod common;
use cesium_rs::core::ktx2_transcoder::*;
use cesium_rs::core::CompressedTextureBuffer;
use common::*;
use serde_json::{json, Value};
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};
fn ready<T>(f: impl Future<Output = T>) -> T {
    let mut f = pin!(f);
    match f.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("synchronous codec yielded"),
    }
}
fn buffer(b: CompressedTextureBuffer, face: &str) -> Value {
    json!([face, b.internal_format, b.pixel_datatype, b.width, b.height, b.buffer_view()])
}
fn texture(t: Ktx2Texture) -> Value {
    match t {
        Ktx2Texture::Image(b) => json!([[buffer(b, "positiveX")]]),
        Ktx2Texture::Mipmaps(m) => json!(m.into_iter().map(|b| vec![buffer(b, "positiveX")]).collect::<Vec<_>>()),
        Ktx2Texture::Cubemap(m) => json!(m
            .into_iter()
            .map(|faces| faces
                .into_iter()
                .map(|(f, b)| buffer(
                    b,
                    match f {
                        CubeFace::PositiveX => "positiveX",
                        CubeFace::NegativeX => "negativeX",
                        CubeFace::PositiveY => "positiveY",
                        CubeFace::NegativeY => "negativeY",
                        CubeFace::PositiveZ => "positiveZ",
                        CubeFace::NegativeZ => "negativeZ",
                    }
                ))
                .collect::<Vec<_>>())
            .collect::<Vec<_>>()),
    }
}
#[test]
fn ktx2_reference() {
    run(
        "core",
        "KTX2Transcoder",
        |_| EXACT,
        |_, a| {
            let data = if a[0].is_null() {
                a[2].as_array().unwrap().iter().map(|v| u(v) as u8).collect()
            } else {
                std::fs::read(format!("{}/tests/data/ktx2/{}", env!("CARGO_MANIFEST_DIR"), s(&a[0]))).unwrap()
            };
            let has = |key: &str| a[1][key].as_bool().unwrap_or(false);
            let codec = Ktx2Transcoder {
                basis: RustBasisTranscoder,
                supported_formats: SupportedTextureFormats {
                    etc: has("etc"),
                    etc1: has("etc1"),
                    s3tc: has("s3tc"),
                    pvrtc: has("pvrtc"),
                    astc: has("astc"),
                    bc7: has("bc7"),
                },
            };
            match ready(codec.transcode(&data)) {
                Ok(t) => ok(texture(t)),
                Err(e) => Some(Out::Err(e.to_string())),
            }
        },
    );
}
#[test]
fn corrupt_ktx2_is_rejected() {
    for len in 0..80 {
        assert!(Ktx2Header::parse(&vec![0; len]).is_err());
    }
    let original = include_bytes!("data/ktx2/Green4x4.ktx2");
    for len in 80..original.len() {
        assert!(
            ready(Ktx2Transcoder::default().transcode(&original[..len])).is_err(),
            "accepted truncated length {len}"
        );
    }
}
