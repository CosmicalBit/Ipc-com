# Releasing

The [publish workflow](.github/workflows/publish.yml) runs when a push to `main` changes `Cargo.toml` or the publish workflow itself. If the version is not already on crates.io, it checks formatting, Clippy, tests, and the committed lockfile, then publishes the crate. GitHub updates the repository README on push; crates.io updates the crate README from the new release, and docs.rs builds its documentation after publication.

## One-time setup

As an owner of `ipc-com` on crates.io, add a [trusted publisher](https://crates.io/crates/ipc-com/settings/new-trusted-publisher) with these values:

- Provider: GitHub Actions
- Repository owner: `CosmicalBit`
- Repository name: `Ipc-com`
- Workflow filename: `publish.yml`
- Environment: leave empty

The workflow uses a temporary crates.io token, so no GitHub secret is needed.

## Publish a new version

Increase the version in `Cargo.toml` and `Cargo.lock` in the same commit, then push to `main`. A version already present on crates.io is skipped. Each new version is immutable, so README and package metadata changes intended for the crates.io page also need a new version.
