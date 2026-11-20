# Releasing

Releases are automated with [release-plz](https://release-plz.dev) and
[crates.io trusted publishing](https://crates.io/docs/trusted-publishing)
(GitHub OIDC). The first release is also driven by GitHub Actions; no local
`cargo publish` is required.

On every push to the default branch:

- **Release check** runs the test suite and checks whether all publishable
  crates exist on crates.io.
- **Release-plz bootstrap** runs only while a crate is missing; it uses a
  temporary token to create the crates.
- **Release-plz release** runs once all crates exist; it publishes through OIDC
  trusted publishing.
- **Release-plz PR** computes the next versions, updates `Cargo.toml` and
  changelogs, and opens or updates a release PR.

## First release (one-time token bootstrap)

crates.io cannot create a **new** crate through OIDC. The workflow handles this
automatically: while any of `koanf-core`, `koanf-derive`, or `koanf-rs` is
missing from the registry, it uses a temporary `CARGO_REGISTRY_TOKEN`; after all
crates exist it switches to trusted publishing.

1. Create the GitHub repository `hushink/koanf-rs`. If the owner or default
   branch is different, update the `github.repository_owner` guard and the
   `branches` filter in `.github/workflows/release-plz.yml`. The committed
   workflow targets `main`.
2. In repository **Settings → Actions → General → Workflow permissions**,
   enable **Allow GitHub Actions to create and approve pull requests** for the
   release PR job.
3. Add a **temporary** repository secret named `CARGO_REGISTRY_TOKEN` with a
   crates.io token scoped to creating/publishing `koanf-core`, `koanf-derive`,
   and `koanf-rs`. This secret is only needed for the initial crate creation; the
   bootstrap job fails with a clear error if it is missing.
4. Push the repository:
   ```bash
   git remote add origin https://github.com/hushink/koanf-rs.git
   git push -u origin main   # use your default branch if it differs
   ```
   The check job detects that the crates do not exist and the bootstrap job
   publishes `koanf-core`, then `koanf-derive`, then `koanf-rs`, creating the
   `koanf-core-v0.1.0` / `koanf-derive-v0.1.0` / `koanf-rs-v0.1.0` tags and
   GitHub releases. No local `cargo publish` is involved.
5. After `0.1.0` is visible on crates.io, configure trusted publishing for
   **all three** crates:
   - GitHub owner: `hushink`
   - Repository: `koanf-rs`
   - Workflow filename: `release-plz.yml`
     (the file is `.github/workflows/release-plz.yml`)
   - Environment: leave empty; the workflow does not use a GitHub environment.
6. Delete the `CARGO_REGISTRY_TOKEN` secret. From now on the existence check
   finds all crates and the release job uses OIDC only.

If the first run fails (for example, the secret was not added yet), add it and
re-run **Actions → Release-plz → Run workflow** (`workflow_dispatch`). If a
future release adds a new publishable crate, extend the crates.io existence
check in the workflow or temporarily re-add the token for that crate's first
publish.

## Release flow

1. Push conventional commits to the default branch.
2. The PR job opens/updates `release-plz-*` with the next versions and
   per-package changelogs.
3. Merge the release PR.
4. The check job tests and finds all crates on crates.io; the release job
   publishes through OIDC, creates tags (`koanf-core-vX.Y.Z`,
   `koanf-derive-vX.Y.Z`, `koanf-rs-vX.Y.Z`), and creates GitHub releases.

`version_group = "koanf"` keeps all crates on the same version. The facade's
dependencies on `koanf-core` and `koanf-derive` are updated by release-plz.

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
  `CARGO_REGISTRY_TOKEN` on the normal path. The release-plz action performs the
  OIDC exchange.
- The temporary `CARGO_REGISTRY_TOKEN` is used only while a crate is missing
  from crates.io. Delete it after the first release.
- The PR job has no registry token.
- Release PRs can be squashed; `release_always = false` would require a merge
  commit associated with the release PR.
- The default `GITHUB_TOKEN` cannot trigger other workflows. Add a PAT or
  GitHub App token if CI must run on release PRs or releases.
- The bootstrap and trusted-publishing release jobs share a non-canceling
  `release-plz-publish` concurrency group, so overlapping workflow runs cannot
  publish concurrently. Do not change it to cancel in progress.
- Do not add a canceling concurrency group to the release job.
