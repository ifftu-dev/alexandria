// Copyright 2025 N0, INC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::av::Decoders;

pub use self::{audio::*, ext::ffmpeg_log_init, video::*};

#[derive(Debug, Clone, Copy)]
pub struct FfmpegDecoders;

impl Decoders for FfmpegDecoders {
    type Audio = FfmpegAudioDecoder;
    type Video = FfmpegVideoDecoder;
}

mod audio {
    mod decoder;
    mod encoder;
    pub use decoder::*;
    pub use encoder::*;
}

pub mod video {
    mod decoder;
    mod encoder;
    pub(crate) mod util;
    pub use decoder::*;
    pub use encoder::*;
}

pub(crate) mod ext {
    use buf_list::BufList;
    use bytes::Buf;
    use ffmpeg_next as ffmpeg;
    pub fn ffmpeg_log_init() {
        use ffmpeg::util::log::Level::*;
        let level = if let Ok(val) = std::env::var("FFMPEG_LOG") {
            match val.as_str() {
                "quiet" => Quiet,
                "panic" => Panic,
                "fatal" => Fatal,
                "error" => Error,
                "warn" | "warning" => Warning,
                "info" => Info,
                "verbose" => Verbose,
                "debug" => Debug,
                "trace" => Trace,
                _ => Warning,
            }
        } else {
            Warning
        };
        ffmpeg::util::log::set_level(level);
    }

    pub trait PacketExt {
        fn to_ffmpeg_packet(self) -> ffmpeg::Packet;
    }

    impl PacketExt for BufList {
        fn to_ffmpeg_packet(mut self) -> ffmpeg_next::Packet {
            let mut packet = ffmpeg::Packet::new(self.num_bytes());
            let dst = packet.data_mut().unwrap();
            self.copy_to_slice(dst);
            packet
        }
    }

    // moq-mux 0.19 frames carry their payload as `bytes::Bytes` (hang 0.19
    // dropped the old `BufList` payload), so decoders hand us `Bytes` directly.
    impl PacketExt for bytes::Bytes {
        fn to_ffmpeg_packet(self) -> ffmpeg_next::Packet {
            let mut packet = ffmpeg::Packet::new(self.len());
            packet.data_mut().unwrap().copy_from_slice(&self);
            packet
        }
    }

    pub trait CodecContextExt {
        fn extradata(&self) -> Option<&[u8]>;
        fn set_extradata(&mut self, extradata: &[u8]) -> Result<(), ffmpeg::Error>;
    }

    impl CodecContextExt for ffmpeg::codec::Context {
        fn extradata(&self) -> Option<&[u8]> {
            // SAFETY: `Context` owns a live `AVCodecContext` for the duration
            // of `&self`. FFmpeg maintains `extradata` as either null/empty or
            // a readable allocation of `extradata_size` bytes. The returned
            // slice cannot outlive that shared borrow, so mutation or context
            // destruction cannot invalidate it while it is in use.
            unsafe {
                let ctx = self.as_ptr();
                if (*ctx).extradata.is_null() || (*ctx).extradata_size <= 0 {
                    return None;
                }
                Some(std::slice::from_raw_parts(
                    (*ctx).extradata as *const u8,
                    (*ctx).extradata_size as usize,
                ))
            }
        }

        fn set_extradata(&mut self, extradata: &[u8]) -> Result<(), ffmpeg::Error> {
            let data_size =
                i32::try_from(extradata.len()).map_err(|_| ffmpeg::Error::InvalidData)?;
            let padding = ffmpeg::ffi::AV_INPUT_BUFFER_PADDING_SIZE as usize;
            let allocation_size = extradata
                .len()
                .checked_add(padding)
                .ok_or(ffmpeg::Error::InvalidData)?;

            // SAFETY: `&mut self` provides exclusive access to the live
            // `AVCodecContext`. New storage comes from FFmpeg's zeroing
            // allocator with its required input padding. We allocate before
            // releasing the old FFmpeg-owned buffer, copy exactly the source
            // length into the checked allocation, and leave ownership with the
            // context so `avcodec_free_context` can release it normally.
            unsafe {
                let ctx = self.as_mut_ptr();
                let new_data = if extradata.is_empty() {
                    std::ptr::null_mut()
                } else {
                    ffmpeg::ffi::av_mallocz(allocation_size).cast::<u8>()
                };
                if !extradata.is_empty() && new_data.is_null() {
                    return Err(ffmpeg::Error::Bug);
                }
                if !new_data.is_null() {
                    std::ptr::copy_nonoverlapping(extradata.as_ptr(), new_data, extradata.len());
                }

                ffmpeg::ffi::av_freep(std::ptr::addr_of_mut!((*ctx).extradata).cast());
                (*ctx).extradata = new_data;
                (*ctx).extradata_size = data_size;
            }
            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::CodecContextExt;
        use ffmpeg_next as ffmpeg;

        #[test]
        fn codec_extradata_can_be_replaced_and_cleared() {
            let mut context = ffmpeg::codec::Context::new();
            assert_eq!(context.extradata(), None);

            context.set_extradata(&[1, 2, 3]).expect("initial data");
            assert_eq!(context.extradata(), Some([1, 2, 3].as_slice()));

            context.set_extradata(&[4, 5]).expect("replacement data");
            assert_eq!(context.extradata(), Some([4, 5].as_slice()));

            context.set_extradata(&[]).expect("clear data");
            assert_eq!(context.extradata(), None);
        }
    }
}
