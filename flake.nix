{
  description = "bouc";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs = {
        nixpkgs.follows = "nixpkgs";
      };
    };
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      flake-utils,
      crane,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
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
          filter =
            path: type:
            (craneLib.filterCargoSources path type)
            || (pkgs.lib.hasInfix "/assets/vendor/" path)
            || (pkgs.lib.hasSuffix ".sql" path)
            || (pkgs.lib.hasSuffix ".css" path)
            || (pkgs.lib.hasSuffix ".ts" path)
            || (pkgs.lib.hasInfix "tests/testing_library/testing-library-dom" path);
        };

        # tsconfig.json is deliberately not in `src`: it would invalidate the
        # cargo dependency build on every edit
        tsSrc = pkgs.lib.cleanSourceWith {
          src = ./.;
          filter =
            path: type:
            (type == "directory")
            || (pkgs.lib.hasSuffix ".ts" path)
            || (pkgs.lib.hasSuffix "/tsconfig.json" path);
        };

        craneCommonArgs = {
          inherit src;
          strictDeps = true;
          # build.rs shells out to both
          nativeBuildInputs = [
            rolldown
            pkgs.tailwindcss_4
            # lettre's native-tls pulls in openssl-sys, which needs pkg-config
            pkgs.pkg-config
          ];
          buildInputs = [
            pkgs.openssl
          ]
          ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [ pkgs.libiconv ];
        };

        cargoArtifacts = craneLib.buildDepsOnly craneCommonArgs;

        # the crate version in Cargo.toml is meaningless, we release from git
        version = self.shortRev or self.dirtyShortRev;

        bouc = craneLib.buildPackage (
          craneCommonArgs
          // {
            inherit cargoArtifacts version;
          }
        );

        boucImage = pkgs.dockerTools.buildLayeredImage {
          name = "bouc";
          tag = bouc.version;
          contents = [
            bouc
            # a non-root user needs an /etc/passwd entry, and glibc needs an
            # /etc/nsswitch.conf to resolve the SMTP server name
            (pkgs.dockerTools.fakeNss.override {
              extraPasswdLines = [ "bouc:x:1000:1000:bouc:/data:/noshell" ];
              extraGroupLines = [ "bouc:x:1000:" ];
            })
          ];
          fakeRootCommands = ''
            mkdir -p ./data
            chown 1000:1000 ./data
          '';
          config = {
            Entrypoint = [ "/bin/bouc" ];
            User = "1000:1000";
            WorkingDir = "/data";
            ExposedPorts = {
              "3000/tcp" = { };
            };
            # nothing sets up /etc/ssl or /etc/localtime in the image, so point
            # openssl and jiff straight at the store
            Env = [
              "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
              "TZDIR=${pkgs.tzdata}/share/zoneinfo"
            ];
          };
        };
      in
      with pkgs;
      {
        checks = {
          # Make sure it compiles
          inherit bouc;

          bouc-clippy = craneLib.cargoClippy (craneCommonArgs // { inherit cargoArtifacts; });
          bouc-fmt = craneLib.cargoFmt { inherit src; };

          bouc-ts =
            runCommand "bouc-ts"
              {
                nativeBuildInputs = [
                  typescript
                  oxlint
                ];
              }
              ''
                cd ${tsSrc}
                tsc --noEmit
                oxlint --deny-warnings
                touch $out
              '';
        }
        # nixpkgs has no chromium on darwin: build checks.<linux system>.bouc-e2e
        # from there, nix hands it to a linux builder
        // lib.optionalAttrs stdenv.hostPlatform.isLinux {
          bouc-e2e = craneLib.cargoTest (
            craneCommonArgs
            // {
              inherit cargoArtifacts;
              cargoTestExtraArgs = "--test e2e";
              nativeBuildInputs = craneCommonArgs.nativeBuildInputs ++ [
                chromedriver
                chromium
                mailpit
              ];
              CHROME_BINARY = "${chromium}/bin/chromium";
              # the UI has emoji in it, and skia aborts the renderer rather than
              # fall back to a font it cannot find
              FONTCONFIG_FILE = makeFontsConf {
                fontDirectories = [
                  dejavu_fonts
                  noto-fonts-color-emoji
                ];
              };
              # chrome refuses to start without a writable home
              preCheck = "export HOME=$(mktemp -d)";
            }
          );
        };
        packages = {
          default = bouc;
        }
        # dockerTools only builds linux images: build
        # packages.<linux system>.docker, nix hands it to a linux builder
        // lib.optionalAttrs stdenv.hostPlatform.isLinux {
          docker = boucImage;
        };
        apps.default = flake-utils.lib.mkApp { drv = bouc; };
        devShells.default = mkShell {
          buildInputs = [
            chromedriver
            mailpit
            nixfmt
            openssl
            oxfmt
            pkg-config
            oxlint
            rolldown
            rust
            tailwindcss_4
            treefmt
            typescript
            watchexec
          ]
          ++ lib.optionals stdenv.isDarwin [ libiconv ];
        };
      }
    );
}
