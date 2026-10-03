//! # worldforge
//!
//! A from-scratch Rust reimplementation of a WorldBox-style god simulator: hex
//! world generation, four civilized races, villages, kingdoms, diplomacy, wars,
//! ages, disasters and a toolbox of god powers.
//!
//! Everything is deterministic. `World::state_hash()` folds the whole world into a
//! `u64`; the same seed plus the same script always produce the same hash, which
//! is the oracle the test suite and the CLI lean on.
//!
//! ```no_run
//! use worldforge::powers::Power;
//! use worldforge::world::World;
//! use worldforge::worldgen::{GenParams, WorldType};
//!
//! let mut world = World::generate(GenParams::new(64, 48, 42, WorldType::Continents));
//! world.step_n(2000);
//! let somewhere = world.random_land_tile().unwrap();
//! world.cast(Power::Nuke, somewhere);
//! world.step_n(500);
//! println!("{}", world.summary());
//! ```
//!
//! No dependencies: the RNG, the save codec, the ASCII renderer and the PNG writer
//! are all in this crate, so it builds offline and behaves the same everywhere.

pub mod ages;
pub mod disaster;
pub mod hex;
pub mod kingdom;
pub mod names;
pub mod path;
pub mod png;
pub mod powers;
pub mod races;
pub mod render;
pub mod rng;
pub mod save;
pub mod script;
pub mod sim;
pub mod terrain;
pub mod units;
pub mod village;
pub mod world;
pub mod worldgen;

pub use hex::Hex;
pub use races::Race;
pub use rng::Rng;
pub use world::{Event, EventKind, World, WorldStats};
pub use worldgen::{GenParams, WorldType};
