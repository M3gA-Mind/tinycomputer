fn main() {
    println!("cargo:rustc-check-cfg=cfg(live_module_browser_egress_tests)");
    println!("cargo:rerun-if-env-changed=TINYCOMPUTER_LIVE_BROWSER_EGRESS_TESTS");
    if std::env::var_os("TINYCOMPUTER_LIVE_BROWSER_EGRESS_TESTS").is_some() {
        println!("cargo:rustc-cfg=live_module_browser_egress_tests");
    }
}
