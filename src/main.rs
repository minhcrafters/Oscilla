//! Standalone entry point for Oscilla.
//!
//! When the `standalone` feature is enabled, this binary provides a
//! desktop application that hosts the plugin using nice-plug's built-in
//! standalone wrapper (cpal + baseview).

use nice_plug::prelude::*;

fn main() {
    nice_export_standalone::<oscilla::Oscilla>();
}
