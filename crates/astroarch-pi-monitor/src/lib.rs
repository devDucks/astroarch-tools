//! The library behind `astroarch-pi-monitor` (the binary) and the QML plugin
//! (`astroarch-pi-monitor-qml`): sensors, alerts, settings, the D-Bus daemon and client.

pub mod alerts;
pub mod client;
pub mod config;
pub mod daemon;
pub mod i18n;
pub mod install;
pub mod sensors;
pub mod status;
