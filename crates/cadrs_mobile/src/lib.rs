//! cadrs on a phone: a small web page (no Bevy, no kernel) for what a phone is good for: your
//! card to show, joining your account, contacts, the shops' answers to your requests for quotes,
//! accepting a quote and messaging the shop.
//!
//! - [`state`]: the phone's state and what it does with posts (native, tested).
//! - `page` (in a browser): the screens, and the network through `cadrs_social_net`.

#[cfg(target_arch = "wasm32")]
mod page;
pub mod scan;
pub mod shop;
pub mod state;
