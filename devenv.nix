{ config, lib, pkgs, ... }:
let
  verus = pkgs.callPackage ./nix/verus.nix {};
  linuxLibraries = with pkgs; lib.optionals stdenv.isLinux [
    fontconfig
    libx11
    libxcb
    libxkbcommon
    vulkan-loader
    wayland
  ];
in {
  packages = (with pkgs; [
    ast-grep
    clang
    git
    lld
    openssl
    pkg-config
    pre-commit
    jetbrains-mono
    rustup
  ]) ++ [ verus ] ++ linuxLibraries;

  languages.rust = {
    enable = true;
    channel = "stable";
    version = (builtins.fromTOML (builtins.readFile ./rust-toolchain.toml)).toolchain.channel;
  };

  env = {
    HELIX_DISABLE_AUTO_GRAMMAR_BUILD = "1";
    HELIX_RUNTIME = "${config.devenv.root}/runtime";
    FONTCONFIG_FILE = pkgs.makeFontsConf { fontDirectories = [ pkgs.jetbrains-mono ]; };
    LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";
    LD_LIBRARY_PATH = lib.makeLibraryPath linuxLibraries;
    RUST_BACKTRACE = "1";
  };

  tasks."project:check".exec = "cargo check --workspace --all-targets --locked";
  tasks."project:test".exec = "cargo test --workspace --locked";
  tasks."project:run".exec = "cargo run --bin hxt";
  tasks."verus:setup".exec = "rustup toolchain install ${verus.requiredRustToolchain} --profile minimal";
  tasks."project:verify".exec = ''
    cargo verus focus -p helix-term --locked --lib -- --verify-module typst_preview::core::context --verify-module typst_preview::core::state --verify-module typst_preview::core::layout --verify-module typst_preview::ports::renderer --verify-module typst_preview::ports::svg
  '';
}
