use vergen::{Config, vergen};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if vergen(Config::default()).is_err() {
        println!("cargo:rustc-env=VERGEN_BUILD_SEMVER={}", env!("CARGO_PKG_VERSION"));
        println!("cargo:rustc-env=VERGEN_CARGO_PROFILE=unknown");
        println!("cargo:rustc-env=VERGEN_GIT_SHA=unknown");
        println!("cargo:rustc-env=VERGEN_GIT_COMMIT_TIMESTAMP=unknown");
    }

    Ok(())
}
