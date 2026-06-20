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
            doCheck = false;
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
                "MBB_HOST=0.0.0.0"
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

          coverage = craneLib.cargoLlvmCov (commonArgs // {
            inherit cargoArtifacts;
            cargoLlvmCovExtraArgs = "--workspace --all-features --fail-under-lines 75";
            installPhase = "touch $out";
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

      apps = forEachSupportedSystem ({ pkgs, ... }:
        let
          nightlyToolchain = pkgs.rust-bin.selectLatestNightlyWith (toolchain:
            toolchain.default.override {
              extensions = [ "miri" "rust-src" ];
            }
          );
        in {
        miri = {
          type = "app";
          program = toString (pkgs.writeShellScript "miri" ''
            set -euo pipefail
            export PATH="${nightlyToolchain}/bin:$PATH"
            cargo miri test
          '');
        };

        e2e = {
          type = "app";
          program = toString (pkgs.writeShellScript "e2e" ''
            set -euo pipefail
            nix build .#default
            cd e2e
            npm ci --silent
            npx playwright install --with-deps chromium
            MBB_BINARY="$(cd .. && pwd)/result/bin/mybriefcase-bookmarks" npx playwright test
          '');
        };

        docker-test = {
          type = "app";
          program = toString (pkgs.writeShellScript "docker-test" ''
            set -euo pipefail
            DOCKER_ARCH=$(docker info --format '{{.Architecture}}')
            if [ "$DOCKER_ARCH" = "aarch64" ]; then
              DOCKER_SYSTEM="aarch64-linux"
            else
              DOCKER_SYSTEM="x86_64-linux"
            fi
            echo "Building Docker image ($DOCKER_SYSTEM)..."
            nix build ".#packages.$DOCKER_SYSTEM.docker"
            echo "Running smoke test..."
            docker load < result
            docker run -d --name smoke-test -p 3000:3000 mybriefcase-bookmarks:latest
            timeout 30 sh -c 'until curl -sf http://localhost:3000/ | grep -q "MyBriefcase Bookmarks"; do sleep 1; done'
            docker stop smoke-test && docker rm smoke-test
            echo "Docker smoke test passed!"
          '');
        };

        validate = {
          type = "app";
          program = toString (pkgs.writeShellScript "validate" ''
            exec nix develop --command validate
          '');
        };

        validate-all = {
          type = "app";
          program = toString (pkgs.writeShellScript "validate-all" ''
            exec nix develop --command validate-all
          '');
        };
      });

      devShells = forEachSupportedSystem ({ pkgs, ... }:
        let
          validate = pkgs.writeShellScriptBin "validate" ''
            set -euo pipefail
            echo "==> Running Nix flake checks..."
            nix flake check --keep-going
            echo "==> Running Miri..."
            nix run .#miri
            echo "==> Running E2E tests..."
            nix run .#e2e
            echo "==> All validations passed!"
          '';
          validate-all = pkgs.writeShellScriptBin "validate-all" ''
            set -euo pipefail
            validate
            echo "==> Running mutation testing (diff vs main)..."
            git diff origin/main...HEAD > /tmp/mutants-diff.patch
            if [ -s /tmp/mutants-diff.patch ]; then
              cargo mutants --in-diff /tmp/mutants-diff.patch --in-place -vV --timeout 300
            else
              echo "    No diff vs main, skipping"
            fi
            echo "==> Running Docker build + smoke test..."
            nix run .#docker-test
            echo "==> All validations passed!"
          '';
          cargo-mutants-diff = pkgs.writeShellScriptBin "cargo-mutants-diff" ''
            set -euo pipefail
            git diff origin/main...HEAD > /tmp/mutants-diff.patch
            if [ -s /tmp/mutants-diff.patch ]; then
              cargo mutants --in-diff /tmp/mutants-diff.patch --in-place -vV --timeout 300
            else
              echo "No diff vs main, nothing to test"
            fi
          '';
        in {
        default = pkgs.mkShell {
          packages = with pkgs; [
            rustToolchain
            openssl
            pkg-config
            cargo-audit
            cargo-deny
            cargo-edit
            cargo-llvm-cov
            cargo-mutants
            cargo-nextest
            cargo-watch
            rust-analyzer
            nodejs_22
            validate
            validate-all
            cargo-mutants-diff
          ];

          env = {
            RUST_SRC_PATH = "${pkgs.rustToolchain}/lib/rustlib/src/rust/library";
          };
        };
      });
    };
}
