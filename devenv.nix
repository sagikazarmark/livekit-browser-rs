{ pkgs, ... }:

{
  dotenv.enable = true;

  dagger.enable = true;
  env.DAGGER_X_RELEASE = "v1.0.0-beta.7";

  # Required by arborium
  env.CC_wasm32_unknown_unknown = "${pkgs.llvmPackages.clang-unwrapped}/bin/clang";

  packages = with pkgs; [
    lld

    cargo-audit
    cargo-deny
    cargo-dist
    cargo-release
    cargo-watch

    dioxus-cli
    wasm-pack
    wasm-bindgen-cli_0_2_126

    # Builds the demo's Cloudflare Worker (`demo/wrangler.toml`). nixpkgs does
    # not bundle worker-build's wasm-bindgen/wasm-opt dependencies, so both have
    # to be on PATH or it downloads its own.
    worker-build
    binaryen
  ];

  languages = {
    rust = {
      enable = true;
      channel = "stable";
      targets = [ "wasm32-unknown-unknown" ];
    };
    javascript = {
      enable = true;
      npm.enable = true;
    };
  };
}
