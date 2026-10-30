{ lib
, stdenv
, fetchurl
, unzip
, autoPatchelfHook
, makeWrapper
}:

# Official weekly Verus binary. Keep this separate from Gulix's Rust toolchain:
# the verifier invokes the exact toolchain recorded in its own version.json.
stdenv.mkDerivation rec {
  pname = "verus";
  version = "0.2026.10.18.7d82994";

  src = fetchurl {
    url = "https://github.com/verus-lang/verus/releases/download/release/${version}/verus-${version}-x86-linux.zip";
    hash = "sha256-6a8Dcy1L3TK5Oo/PRxcGc5wkwUc/pQhmGNlsRyN4Rd0=";
  };

  nativeBuildInputs = [ unzip autoPatchelfHook makeWrapper ];
  buildInputs = [ stdenv.cc.cc.lib ];
  # rust_verify receives rustc_driver from Verus's rustup toolchain at runtime.
  # OpenSSL is needed only by bundled Verus developer utilities, not the verifier.
  autoPatchelfIgnoreMissingDeps = [
    "librustc_driver-7ed0375ae352bd75.so"
    "libssl.so.3"
    "libcrypto.so.3"
  ];
  dontBuild = true;

  installPhase = ''
    runHook preInstall
    mkdir -p $out/lib/verus $out/bin
    cp -a . $out/lib/verus/
    makeWrapper $out/lib/verus/verus $out/bin/verus
    makeWrapper $out/lib/verus/cargo-verus $out/bin/cargo-verus
    runHook postInstall
  '';

  passthru.requiredRustToolchain = "1.99.1";

  meta = {
    description = "Verifier for Rust code";
    homepage = "https://github.com/verus-lang/verus";
    license = lib.licenses.mit;
    platforms = [ "x86_64-linux" ];
  };
}
