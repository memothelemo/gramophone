#![forbid(unsafe_code)]
#![warn(
    clippy::pedantic,
    clippy::must_use_candidate,
    clippy::empty_enum,
    clippy::unwrap_used
)]
#![allow(
    clippy::new_without_default,
    clippy::empty_docs,
    clippy::missing_errors_doc,
    clippy::module_name_repetitions
)]

pub mod client;
pub mod crypto;
pub mod mixer;

/// This module provides composable voice-related objects and utilities
/// for building customizable behavior.
///
/// It enables developers to define their own abstractions tailored
/// to their specific needs.
pub mod net;
