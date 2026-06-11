{
  description = "A Nix-flake-based Rust development environment";

  inputs = {
    nixpkgs.url = "https://flakehub.com/f/NixOS/nixpkgs/0.1.*.tar.gz";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
  };

  outputs = { self, nixpkgs, rust-overlay, crane }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forEachSupportedSystem = f: nixpkgs.lib.genAttrs supportedSystems (system: f {
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default self.overlays.default ];
        };
      });
    in
    {
      overlays.default = final: prev: {
        rustToolchain = final.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
      };

      packages = forEachSupportedSystem ({ pkgs }:
        let
          craneLib = (crane.mkLib pkgs).overrideToolchain pkgs.rustToolchain;
          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter = path: type:
              (craneLib.filterCargoSources path type) ||
              (builtins.match ".*templates/.*" path != null) ||
              (builtins.match ".*static/.*" path != null);
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
            name = "automerge-playground";
            tag = "latest";
            contents = [ bin pkgs.cacert pkgs.busybox pkgs.tzdata ];
            config = {
              Cmd = [ "${bin}/bin/automerge-playground" ];
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

      devShells = forEachSupportedSystem ({ pkgs }: {
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
