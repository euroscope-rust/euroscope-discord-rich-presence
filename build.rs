fn main() {
    println!("cargo::rerun-if-env-changed=EUROSCOPE_PLUGIN_DELAYLOAD");

    if std::env::var_os("EUROSCOPE_PLUGIN_DELAYLOAD").is_some() {
        // Delay-loading the DLL defers resolving its imports until the first
        // call into EuroScope. The tests never call into EuroScope, so the DLL
        // is never loaded and the harness runs normally.
        // `delayimp.lib` provides the delay-load helper (`__delayLoadHelper2`);
        // `/DELAYLOAD` marks the EuroScope import so it resolves lazily.
        println!("cargo::rustc-link-arg=/DELAYLOAD:EuroScopePlugInDll.dll");
        println!("cargo::rustc-link-arg=delayimp.lib");
    }
}
