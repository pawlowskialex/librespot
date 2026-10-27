use std::io::{Read, Seek};
use std::ops::Deref;

use thiserror::Error;

use crate::metadata::audio::AudioFileFormat;

#[cfg(feature = "passthrough-decoder")]
mod passthrough_decoder;
#[cfg(feature = "passthrough-decoder")]
pub use passthrough_decoder::PassthroughDecoder;

mod symphonia_decoder;
pub use symphonia_decoder::SymphoniaDecoder;

#[derive(Error, Debug)]
pub enum DecoderError {
    #[error("Passthrough Decoder Error: {0}")]
    PassthroughDecoder(String),
    #[error("Symphonia Decoder Error: {0}")]
    SymphoniaDecoder(String),
    #[error("Decoder Plugin Error: {0}")]
    Plugin(String),
}

pub type DecoderResult<T> = Result<T, DecoderError>;

#[derive(Error, Debug)]
pub enum AudioPacketError {
    #[error("Decoder Raw Error: Can't return Raw on Samples")]
    Raw,
    #[error("Decoder Samples Error: Can't return Samples on Raw")]
    Samples,
}

pub type AudioPacketResult<T> = Result<T, AudioPacketError>;

pub enum AudioPacket {
    Samples(Vec<f64>),
    Raw(Vec<u8>),
}

impl AudioPacket {
    #[inline]
    pub fn samples(&self) -> AudioPacketResult<&[f64]> {
        match self {
            AudioPacket::Samples(s) => Ok(s),
            AudioPacket::Raw(_) => Err(AudioPacketError::Raw),
        }
    }

    #[inline]
    pub fn raw(&self) -> AudioPacketResult<&[u8]> {
        match self {
            AudioPacket::Raw(d) => Ok(d),
            AudioPacket::Samples(_) => Err(AudioPacketError::Samples),
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        match self {
            AudioPacket::Samples(s) => s.is_empty(),
            AudioPacket::Raw(d) => d.is_empty(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AudioPacketPosition {
    pub position_ms: u32,
    pub skipped: bool,
}

impl Deref for AudioPacketPosition {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.position_ms
    }
}

pub trait AudioDecoder {
    fn seek(&mut self, position_ms: u32) -> Result<u32, DecoderError>;
    fn next_packet(&mut self) -> DecoderResult<Option<(AudioPacketPosition, AudioPacket)>>;
}

/// The input of a [`DecoderPlugin`]: the decrypted audio file.
pub trait ReadSeek: Read + Seek + Send {}
impl<T: Read + Seek + Send> ReadSeek for T {}

/// A decoder supplied by the application through `PlayerConfig::decoder_plugin`, for example one
/// suited to the platform: a fixed-point decoder on a CPU without an FPU.
pub trait DecoderPlugin: Send + Sync {
    /// Whether this decoder plays files in `format`. The player then prefers files in the formats
    /// it takes, in its usual order of preference, and opens them through [`DecoderPlugin::open`].
    fn takes(&self, format: AudioFileFormat) -> bool;

    /// A decoder for `file`, a file in `format`, positioned at the start of the audio data (for
    /// Ogg Vorbis, after Spotify's own header packet).
    fn open(
        &self,
        file: Box<dyn ReadSeek>,
        format: AudioFileFormat,
    ) -> DecoderResult<Box<dyn AudioDecoder + Send>>;
}

impl From<DecoderError> for librespot_core::error::Error {
    fn from(err: DecoderError) -> Self {
        librespot_core::error::Error::aborted(err)
    }
}

impl From<symphonia::core::errors::Error> for DecoderError {
    fn from(err: symphonia::core::errors::Error) -> Self {
        Self::SymphoniaDecoder(err.to_string())
    }
}
