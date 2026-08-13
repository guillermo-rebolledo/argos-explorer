use std::env;

fn main() {
    println!("cargo:rerun-if-env-changed=ARGOS_EXPLORER_BUILD_TAG");
    let build_tag = env::var("ARGOS_EXPLORER_BUILD_TAG").unwrap_or_else(|_| "dev".to_owned());
    println!("cargo:rustc-env=ARGOS_EXPLORER_BUILD_TAG={build_tag}");
}
