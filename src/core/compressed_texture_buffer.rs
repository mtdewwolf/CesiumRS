//! CesiumJS `CompressedTextureBuffer`. Clones share the compressed bytes.

use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompressedTextureBuffer {
    pub internal_format: u32,
    pub pixel_datatype: Option<u32>,
    pub width: u32,
    pub height: u32,
    buffer: Arc<[u8]>,
}

pub struct CompressedTextureBufferOptions {
    pub internal_format: u32,
    pub pixel_datatype: Option<u32>,
    pub width: u32,
    pub height: u32,
    pub buffer: Arc<[u8]>,
}

impl CompressedTextureBuffer {
    pub fn new(options: CompressedTextureBufferOptions) -> Self {
        Self {
            internal_format: options.internal_format,
            pixel_datatype: options.pixel_datatype,
            width: options.width,
            height: options.height,
            buffer: options.buffer,
        }
    }

    pub fn buffer_view(&self) -> &[u8] {
        &self.buffer
    }
    pub fn array_buffer_view(&self) -> &[u8] {
        &self.buffer
    }
    pub fn shared_buffer(&self) -> &Arc<[u8]> {
        &self.buffer
    }
}
