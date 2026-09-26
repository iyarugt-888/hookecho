//! Desktop chrome: everything drawn around the map. Split out of app.rs so each surface is its
//! own file. `overlay` owns the floating map-first surfaces: the search pill, the right-edge
//! control column and the panels that slide over the map.

mod broadcast;
mod chips;
mod dock;
pub(crate) use dock::DockState;
mod overlay;
mod permalink;
mod phone_rail;
pub(crate) use overlay::{compact, phone_top};
pub(crate) use phone_rail::MODE_BAR_H;
mod registry;
mod ribbon;
mod scrubber;
mod state;
pub(crate) mod window_frame;
mod windows;

use super::*;
