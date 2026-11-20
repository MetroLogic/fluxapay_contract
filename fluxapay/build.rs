// Build script for fluxapay.
//
// Enforces that exactly one deployable contract-* feature is selected when
// building for WASM targets (Issue #1269). Accidental multi-contract WASM
// deployment artifacts or zero-contract WASM artifacts fail with a clear diagnostic.
// On non-WASM targets (native tests, clippy, doc tests), multiple features
// (e.g. `--all-features`) are permitted.

const KNOWN_CONTRACT_FEATURES: &[&str] = &[
    "contract-payment-processor",
    "contract-payment-streaming",
    "contract-refund-manager",
    "contract-gas-estimator",
    "contract-fx-oracle",
    "contract-merchant-registry",
    "contract-payment-link",
    "contract-dex-router",
    "contract-htlc",
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // Track changes in any known contract feature environment variable.
    for feature in KNOWN_CONTRACT_FEATURES {
        let env_var = format!("CARGO_FEATURE_{}", feature.to_uppercase().replace('-', "_"));
        println!("cargo:rerun-if-env-changed={env_var}");
    }

    let is_wasm = std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32")
        || std::env::var("CARGO_CFG_TARGET_FAMILY").as_deref() == Ok("wasm")
        || std::env::var("TARGET").map_or(false, |t| t.starts_with("wasm32"));

    if !is_wasm {
        return;
    }

    // Collect all selected contract-* features
    let mut selected: Vec<String> = std::env::vars()
        .filter_map(|(key, _)| {
            if key.starts_with("CARGO_FEATURE_CONTRACT_") {
                let name = key
                    .strip_prefix("CARGO_FEATURE_")
                    .unwrap()
                    .to_lowercase()
                    .replace('_', "-");
                Some(name)
            } else {
                None
            }
        })
        .collect();

    selected.sort();

    if selected.is_empty() {
        let feature_list = KNOWN_CONTRACT_FEATURES.join("\n  - ");
        let msg = format!(
            "No deployable contract feature selected for WASM build.\n\
             Soroban WASM deployment artifacts require exactly one contract feature to be enabled.\n\
             Available contract features:\n  - {}\n\n\
             To build a contract WASM, enable exactly one feature, for example:\n  \
             cargo build --target wasm32-unknown-unknown --no-default-features --features contract-payment-processor",
            feature_list
        );
        println!("cargo::error={msg}");
        eprintln!("\n[error] {msg}\n");
        panic!(
            "No deployable contract feature selected for WASM build. Exactly one contract-* feature must be enabled."
        );
    }

    if selected.len() > 1 {
        let msg = format!(
            "Multiple deployable contract features selected for WASM build: {}.\n\
             Soroban WASM deployment artifacts require exactly one contract feature, but {} were selected.\n\
             Accidental multi-contract deployment artifacts will exceed size limits and produce symbol conflicts.\n\n\
             To fix this, disable default features if necessary and select only one contract feature, for example:\n  \
             cargo build --target wasm32-unknown-unknown --no-default-features --features {}",
            selected.join(", "),
            selected.len(),
            selected[0]
        );
        println!("cargo::error={msg}");
        eprintln!("\n[error] {msg}\n");
        panic!(
            "Multiple deployable contract features selected for WASM build: {}. Exactly one contract-* feature must be enabled.",
            selected.join(", ")
        );
    }
}
