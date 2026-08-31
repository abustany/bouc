
{
  description = "bouc";

  inputs = {
    nixpkgs.url  = "github:NixOS/nixpkgs/nixos-26.05";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs = {
        nixpkgs.follows = "nixpkgs";
      };
    };
    flake-utils.url  = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils, crane, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        rust = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        craneLib = (crane.mkLib pkgs).overrideToolchain rust;

        # the nixpkgs package installs no bin/ entry point, only the npm tree
        rolldown = pkgs.writeShellScriptBin "rolldown" ''
          exec ${pkgs.nodejs}/bin/node ${pkgs.rolldown}/lib/node_modules/rolldown/bin/cli.mjs "$@"
        '';

        src = pkgs.lib.cleanSourceWith {
          src = ./.;
          filter = path: type:
            (craneLib.filterCargoSources path type)
            || (pkgs.lib.hasInfix "/assets/" path)
            || (pkgs.lib.hasSuffix ".sql" path)
            || (pkgs.lib.hasSuffix ".css" path)
            || (pkgs.lib.hasSuffix ".ts" path);
        };

        craneCommonArgs = {
          inherit src;
          strictDeps = true;
          # build.rs shells out to both
          nativeBuildInputs = [ rolldown pkgs.tailwindcss_4 ];
          buildInputs = [] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [ pkgs.libiconv ];
        };

        cargoArtifacts = craneLib.buildDepsOnly craneCommonArgs;

        bouc = craneLib.buildPackage(
          craneCommonArgs // { inherit cargoArtifacts; }
        );
      in
      with pkgs;
      {
        checks = {
          # Make sure it compiles
          inherit bouc;

          bouc-clippy = craneLib.cargoClippy ( craneCommonArgs // { inherit cargoArtifacts; } );
          bouc-fmt = craneLib.cargoFmt { inherit src; };
        }
        # nixpkgs has no chromium on darwin: build checks.<linux system>.bouc-e2e
        # from there, nix hands it to a linux builder
        // lib.optionalAttrs stdenv.hostPlatform.isLinux {
          bouc-e2e = craneLib.cargoTest ( craneCommonArgs // {
            inherit cargoArtifacts;
            cargoTestExtraArgs = "--test e2e";
            nativeBuildInputs = craneCommonArgs.nativeBuildInputs ++ [ chromedriver chromium mailpit ];
            CHROME_BINARY = "${chromium}/bin/chromium";
            # the UI has emoji in it, and skia aborts the renderer rather than
            # fall back to a font it cannot find
            FONTCONFIG_FILE = makeFontsConf {
              fontDirectories = [ dejavu_fonts noto-fonts-color-emoji ];
            };
            # chrome refuses to start without a writable home
            preCheck = "export HOME=$(mktemp -d)";
          } );
        };
        packages.default = bouc;
        apps.default = flake-utils.lib.mkApp { drv = bouc; };
        devShells.default = mkShell {
          buildInputs = [
            chromedriver
            mailpit
            nixfmt
            oxfmt
            rolldown
            rust
            tailwindcss_4
            treefmt
            typescript
            watchexec
          ] ++ lib.optionals stdenv.isDarwin [ libiconv ];
        };
      }
    );
}
