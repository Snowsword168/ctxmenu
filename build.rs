use embed_manifest::{embed_manifest, manifest::ExecutionLevel, new_manifest};

fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some()
        && std::env::var_os("CARGO_FEATURE_ELEVATED").is_some()
    {
        embed_manifest(
            new_manifest("CtxMenu.Manager")
                .requested_execution_level(ExecutionLevel::RequireAdministrator),
        )
        .expect("unable to embed manifest");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
