# Releasing

Releases are automated with [release-plz](https://release-plz.dev) and
[crates.io trusted publishing](https://crates.io/docs/trusted-publishing)
(GitHub OIDC). No long-lived `CARGO_REGISTRY_TOKEN` is stored in the repository.

Two jobs run on every push to the default branch:

- **Release-plz PR** computes the next versions, updates `Cargo.toml` and
  changelogs, and opens or updates a release PR.
- **Release-plz release** verifies dependencies, exchanges the job's OIDC token
  for a short-lived crates.io token, publishes any unpublished version, then
  creates tags and GitHub releases.

## One-time setup: bootstrap the first release

crates.io cannot create a **new** crate through trusted publishing. Publish
`0.1.0` once manually, then let trusted publishing take over.

1. Create the GitHub repository `hushink/koanf-rs` (an empty repository is
   enough for this step). If the owner is different, also update the
   `github.repository_owner` guards in `.github/workflows/release-plz.yml`.
2. Publish the first versions locally with a crates.io token:
   ```bash
   cargo login
   cargo publish -p koanf-core
   # wait until koanf-core 0.1.0 is visible on crates.io
   cargo publish -p koanf-rs
   ```
   The first publish creates the crates; it is the only step that needs a
   long-lived token. Do not add that token to GitHub.
3. On crates.io, open the settings of **each** crate and configure trusted
   publishing:
   - GitHub owner: `hushink`
   - Repository: `koanf-rs`
   - Workflow filename: `release-plz.yml`
     (the file is `.github/workflows/release-plz.yml`)
   - Environment: leave empty; the workflow does not use a GitHub environment.
4. Create the initial tags and push the repository:
   ```bash
   git tag koanf-core-v0.1.0
   git tag koanf-rs-v0.1.0
   git remote add origin https://github.com/hushink/koanf-rs.git
   git push -u origin master
   git push origin --tags
   ```
   Optionally create the GitHub releases from the per-package changelogs:
   ```bash
   gh release create koanf-core-v0.1.0 --notes-file crates/koanf-core/CHANGELOG.md
   gh release create koanf-rs-v0.1.0 --notes-file crates/koanf-rs/CHANGELOG.md
   ```
5. The first workflow run sees `0.1.0` already published. The release job does
   not need to publish it; subsequent versions use OIDC trusted publishing.

If the repository is pushed before steps 2–3, the first release job can fail
with a trusted-publishing/new-crate error. Finish the bootstrap and re-run the
workflow with **Actions → Release-plz → Run workflow** (`workflow_dispatch`).

## Release flow

1. Push conventional commits to the default branch.
2. The PR job opens/updates `release-plz-*` with the next versions and
   per-package changelogs.
3. Merge the release PR.
4. The release job tests, publishes `koanf-core` first, then `koanf-rs`, creates
   tags (`koanf-core-vX.Y.Z`, `koanf-rs-vX.Y.Z`), and creates GitHub releases.

`version_group = "koanf"` keeps both crates on the same version. The facade's
dependency on `koanf-core` is updated by release-plz.

Per-package changelogs are generated under `crates/koanf-core/CHANGELOG.md` and
`crates/koanf-rs/CHANGELOG.md`. The root `CHANGELOG.md` is the project-level
overview.

## Local checks

```bash
release-plz update --allow-dirty    # edit versions/changelogs locally
release-plz release --dry-run       # verify publishing, without uploading
```

`release --dry-run` still needs forge credentials to inspect release PRs.

## Notes

- `release_always = true` means any unpublished version already in `Cargo.toml`
  is published on a default-branch push. Merging the release PR is what moves a
  new version into `Cargo.toml`.
- Trusted publishing requires `id-token: write` on the release job and no
  `CARGO_REGISTRY_TOKEN`. The release-plz action performs the OIDC exchange.
- The PR job has no registry token; only the release job needs OIDC.
- Release PRs can be squashed; `release_always = false` would require a merge
  commit associated with the release PR.
- The default `GITHUB_TOKEN` cannot trigger other workflows. Add a PAT or
  GitHub App token if CI must run on release PRs or releases.
- Do not add a canceling concurrency group to the release job.
