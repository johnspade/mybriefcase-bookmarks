default:
    @just --list

validate: fmt clippy test deny audit doc lint-frontend
validate-all: validate e2e

fmt:
    cargo fmt --all --check

clippy:
    cargo clippy --all-targets --all-features

test:
    cargo test --all-features

deny:
    cargo deny check

audit:
    cargo audit

doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features

e2e: (build)
    cd e2e && npx playwright test

lint-frontend:
    npx stylelint "static/**/*.css"
    npx html-validate "templates/**/*.html"
    npx eslint "static/**/*.js" --no-error-on-unmatched-pattern

generate-schema:
    cargo run --bin generate_schema > schema/bookmarks.schema.json

build:
    cargo build --release

nix-check:
    nix flake check
