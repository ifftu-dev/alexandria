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

// Vendored media crate (n0 iroh-live). A couple of upstream style choices trip
// these lints; allow them crate-wide rather than diverge cosmetically from the
// reference source we track.
#![allow(clippy::needless_update, clippy::manual_clamp, clippy::never_loop)]

pub mod audio;
pub mod av;
#[cfg(feature = "video")]
pub mod capture;
#[cfg(feature = "video")]
pub mod ffmpeg;
pub mod opus;
pub mod publish;
pub mod subscribe;
mod util;
#[cfg(feature = "video-ios")]
pub mod videotoolbox;

pub use audio::AudioBackend;
