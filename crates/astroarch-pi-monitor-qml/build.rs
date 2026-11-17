use cxx_qt_build::{CxxQtBuilder, PluginType, QmlModule};

fn main() {
    // A dynamic plugin (not the static module of the other crates): plasmashell loads it
    // from the QML import path. The module has no .qml file, only the Rust object
    CxxQtBuilder::new_qml_module(
        QmlModule::new("org.astroarch.pimonitor").plugin_type(PluginType::Dynamic),
    )
    .qt_module("Qml")
    .files(["src/backend.rs"])
    .build();

    // A Rust cdylib exports only the Rust symbols, and drops the C++ ones nobody calls: the
    // two functions Qt looks for in a plugin would be missing. lld accepts a second version
    // script, which exports them (GNU ld and gold refuse two)
    let has_lld = std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join("ld.lld").is_file()));
    if has_lld {
        println!("cargo:rustc-cdylib-link-arg=-fuse-ld=lld");
        println!("cargo:rustc-cdylib-link-arg=-Wl,--undefined=qt_plugin_instance");
        println!("cargo:rustc-cdylib-link-arg=-Wl,--undefined=qt_plugin_query_metadata_v2");
        let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        println!("cargo:rustc-cdylib-link-arg=-Wl,--version-script={manifest}/plugin-exports.map");
    } else {
        println!("cargo:warning=ld.lld not found (package `lld`): the QML plugin will not load, the widget keeps using the command");
    }
    println!("cargo:rerun-if-changed=plugin-exports.map");
}
