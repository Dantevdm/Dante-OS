//! First Light: Dante OS's colours. The sea at dawn, drawn like a technical drawing.

use crate::screen::{Gradient, Rgb};

pub const DAWN: Rgb = Rgb::hex(0xF4E7DC); //       pale dawn peach (top of the sky)
pub const MIST: Rgb = Rgb::hex(0xECEDEA); //       sea mist
pub const SEA_MIST: Rgb = Rgb::hex(0xE3E9EA); //   mist over the water
pub const PAPER: Rgb = Rgb::hex(0xF7F8F6); //      window surfaces
pub const INK: Rgb = Rgb::hex(0x1E2A30); //        deep sea slate, for text
pub const MUTED: Rgb = Rgb::hex(0x5E6B70); //      secondary text
pub const RULE: Rgb = Rgb::hex(0x7F969D); //       hairlines and tick marks
pub const HAIRLINE: Rgb = Rgb::hex(0xC9D1D2); //   window borders
pub const SEA: Rgb = Rgb::hex(0x3F6E7C); //        the one accent
pub const TERRACOTTA: Rgb = Rgb::hex(0xC7794E); // Windows apps, warnings

pub const SKY: Gradient = Gradient { top: DAWN, mid: MIST, mid_at: 0.55, bottom: SEA_MIST };
