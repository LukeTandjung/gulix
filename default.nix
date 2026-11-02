{
  lib,
  rustPlatform,
  callPackage,
  runCommand,
  installShellFiles,
  fetchzip,
  git,
  gitRev ? null,
  grammarOverlays ? [],
  includeGrammarIf ? _: true,
}: let
  fs = lib.fileset;

  src = fs.difference (fs.gitTracked ./.) (fs.unions [
    ./.envrc
    ./rustfmt.toml
    ./screenshot.png
    ./book
    ./docs
    ./runtime
    ./flake.lock
    (fs.fileFilter (file: lib.strings.hasInfix ".git" file.name) ./.)
    (fs.fileFilter (file: file.hasExt "svg") ./.)
    (fs.fileFilter (file: file.hasExt "md") ./.)
    (fs.fileFilter (file: file.hasExt "nix") ./.)
  ]);

  # Next we actually need to build the grammars and the runtime directory
  # that they reside in. It is built by calling the derivation in the
  # grammars.nix file, then taking the runtime directory in the git repo
  # and hooking symlinks up to it.
  grammars = callPackage ./grammars.nix {inherit grammarOverlays includeGrammarIf;};
  runtimeDir = runCommand "helix-runtime" {} ''
    mkdir -p $out
    ln -s ${./runtime}/* $out
    rm -r $out/grammars
    ln -s ${grammars} $out/grammars
  '';
in
  rustPlatform.buildRustPackage (self: {
    # Cargo's Git dependencies share repositories. Fetch one pinned archive per
    # revision, not all Git refs/history during evaluation. These hashes cover
    # unpacked archives; they are not fetchgit (submodule-inclusive) hashes.
    cargoDeps = (rustPlatform.importCargoLock.override {
      fetchgit = {url, rev, sha256}: let
        github = builtins.match "https://github.com/([^/]+)/([^/]+)" url;
      in
        if github == null then
          throw "Gulix Cargo archive fetcher only supports GitHub repositories: ${url}"
        else fetchzip {
          url = "https://github.com/${builtins.elemAt github 0}/${lib.removeSuffix ".git" (builtins.elemAt github 1)}/archive/${rev}.tar.gz";
          inherit sha256;
        };
    }) {
      lockFile = ./Cargo.lock;
      outputHashes = {
        "gpui-0.2.2" = "19hf071brxhf1gij8dm9l6c0hb81haa6fd6fbs45jqqaq9858x6g";
        "xim-ctext-0.3.0" = "14cyxxdgjpbkf8ny4c46cfqjinavi2cyxzwfngldlls97m5zh555";
        "zed-font-kit-0.14.1-zed" = "07jxxm5g2ighb2f3z383ydnfvl4hfaanh7ngr2whhd959a5s0z19";
        "zed-scap-0.0.8-zed" = "0k7fvdxxj0nb1046gvcn82qrd96xi10pywjbna8zg5ssg5064a06";
      };
    };

    propagatedBuildInputs = [ runtimeDir ];
    
    nativeBuildInputs = [
      installShellFiles
      git
    ];

    buildType = "release";

    name = with builtins; (fromTOML (readFile ./helix-term/Cargo.toml)).package.name;
    src = fs.toSource {
      root = ./.;
      fileset = src;
    };

    # Helix attempts to reach out to the network and get the grammars. Nix doesn't allow this.
    HELIX_DISABLE_AUTO_GRAMMAR_BUILD = "1";

    # So Helix knows what rev it is.
    HELIX_NIX_BUILD_REV = gitRev;

    doCheck = false;
    strictDeps = true;

    # Sets the Helix runtime dir to the grammars
    env.HELIX_DEFAULT_RUNTIME = "${runtimeDir}";

    # Get all the application stuff in the output directory.
    postInstall = ''
      mkdir -p $out/lib
      installShellCompletion ${./contrib/completion}/hx.{bash,fish,zsh}
      mkdir -p $out/share/{applications,icons/hicolor/{256x256,scalable}/apps}
      cp ${./contrib/Helix.desktop} $out/share/applications/Helix.desktop
      cp ${./logo.svg} $out/share/icons/hicolor/scalable/apps/helix.svg
      cp ${./contrib/helix.png} $out/share/icons/hicolor/256x256/apps/helix.png
    '';

    meta.mainProgram = "hx";
  })
