{
  description = "A Nix-flake-based Rust development environment";

  inputs = {
    nixpkgs.url = "https://flakehub.com/f/NixOS/nixpkgs/0.1.*.tar.gz";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
    advisory-db = {
      url = "github:rustsec/advisory-db";
      flake = false;
    };
    nix-github-actions = {
      url = "github:nix-community/nix-github-actions";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, rust-overlay, crane, advisory-db, nix-github-actions }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forEachSupportedSystem = f: nixpkgs.lib.genAttrs supportedSystems (system: f {
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default self.overlays.default ];
        };
        inherit system;
      });
    in
    {
      overlays.default = final: prev: {
        rustToolchain = final.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
      };

      packages = forEachSupportedSystem ({ pkgs, ... }:
        let
          craneLib = (crane.mkLib pkgs).overrideToolchain pkgs.rustToolchain;
          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter = path: type:
              (craneLib.filterCargoSources path type) ||
              (builtins.match ".*templates/.*" path != null) ||
              (builtins.match ".*static/.*" path != null) ||
              (builtins.match ".*schema/.*" path != null);
          };
          commonArgs = {
            inherit src;
            buildInputs = with pkgs; [ openssl ];
            nativeBuildInputs = with pkgs; [ pkg-config ];
            preConfigure = ''
              sed -i '/^target-dir/d' .cargo/config.toml
            '';
          };
          cargoArtifacts = craneLib.buildDepsOnly commonArgs;
          bin = craneLib.buildPackage (commonArgs // {
            inherit cargoArtifacts;
          });
        in
        { default = bin; }
        // pkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
          docker = pkgs.dockerTools.buildLayeredImage {
            name = "mybriefcase-bookmarks";
            tag = "latest";
            contents = [ bin pkgs.cacert pkgs.busybox pkgs.tzdata ];
            config = {
              Cmd = [ "${bin}/bin/mybriefcase-bookmarks" ];
              Env = [
                "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
                "TZDIR=${pkgs.tzdata}/share/zoneinfo"
              ];
              ExposedPorts = { "3000/tcp" = {}; };
              Healthcheck = {
                Test = [ "CMD" "wget" "--spider" "-q" "http://localhost:3000/healthz" ];
                Interval = 10000000000;
                Timeout = 3000000000;
                StartPeriod = 5000000000;
                Retries = 3;
              };
            };
          };
        }
      );

      checks = forEachSupportedSystem ({ pkgs, system, ... }:
        let
          craneLib = (crane.mkLib pkgs).overrideToolchain pkgs.rustToolchain;
          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter = path: type:
              (craneLib.filterCargoSources path type) ||
              (builtins.match ".*templates/.*" path != null) ||
              (builtins.match ".*static/.*" path != null) ||
              (builtins.match ".*schema/.*" path != null);
          };
          commonArgs = {
            inherit src;
            buildInputs = with pkgs; [ openssl ];
            nativeBuildInputs = with pkgs; [ pkg-config ];
            preConfigure = ''
              sed -i '/^target-dir/d' .cargo/config.toml
            '';
          };
          cargoArtifacts = craneLib.buildDepsOnly commonArgs;

          frontendSrc = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter = path: type:
              (builtins.match ".*package\\.json$" path != null) ||
              (builtins.match ".*package-lock\\.json$" path != null) ||
              (builtins.match ".*templates/.*" path != null) ||
              (builtins.match ".*static/.*" path != null) ||
              (builtins.match ".*/\\.stylelintrc\\.json$" path != null) ||
              (builtins.match ".*/\\.htmlvalidate\\.json$" path != null) ||
              (builtins.match ".*/eslint\\.config\\.js$" path != null) ||
              (type == "directory");
          };

        in
        {
          fmt = craneLib.cargoFmt { inherit src; };

          clippy = craneLib.cargoClippy (commonArgs // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets --all-features -- -D warnings";
          });

          test = craneLib.cargoTest (commonArgs // {
            inherit cargoArtifacts;
            cargoTestExtraArgs = "--all-features";
          });

          deny = craneLib.cargoDeny { inherit src; };

          audit = craneLib.cargoAudit {
            inherit src;
            inherit advisory-db;
          };

          doc = craneLib.cargoDoc (commonArgs // {
            inherit cargoArtifacts;
            RUSTDOCFLAGS = "-D warnings";
            cargoDocExtraArgs = "--no-deps --all-features";
          });

          lint-frontend = pkgs.buildNpmPackage {
            pname = "mybriefcase-lint-frontend";
            version = "0.0.1";
            src = frontendSrc;
            npmDepsHash = "sha256-sI5vVEDRCs5lVE0gAE4CgJjrZMjRnEa7u6AYaj/gGDI=";
            dontNpmBuild = true;
            installPhase = ''
              export PATH="$PWD/node_modules/.bin:$PATH"
              stylelint "static/**/*.css"
              html-validate "templates/**/*.html"
              eslint "static/**/*.js" --no-error-on-unmatched-pattern
              touch $out
            '';
          };
        }
      );

      githubActions = nix-github-actions.lib.mkGithubMatrix {
        checks = nixpkgs.lib.getAttrs [ "x86_64-linux" ] self.checks;
      };

      devShells = forEachSupportedSystem ({ pkgs, ... }: {
        default = pkgs.mkShell {
          packages = with pkgs; [
            rustToolchain
            openssl
            pkg-config
            cargo-audit
            cargo-deny
            cargo-edit
            cargo-watch
            just
            rust-analyzer
            nodejs_22
          ];

          env = {
            RUST_SRC_PATH = "${pkgs.rustToolchain}/lib/rustlib/src/rust/library";
          };
        };
      });
    };
}
