# Releasing

Releases are automated with [release-plz](https://release-plz.dev). Two jobs run
on every push to the default branch:

- **Release-plz PR** computes the next versions, updates `Cargo.toml` and
  changelogs, and opens or updates a release PR.
- **Release-plz release** publishes any package version present in
  `Cargo.toml` that is not yet on crates.io, then creates tags and GitHub
  releases.

## One-time setup

1. Create the GitHub repository and push `master` (or rename it to `main`).
   The workflow triggers on both branches.
2. Add a repository secret named `CARGO_REGISTRY_TOKEN` containing a crates.io
   API token with publish permissions.
3. If the repository owner is not `hushink`, update the
   `github.repository_owner` guards in `.github/workflows/release-plz.yml`.
4. The first release of a crate is allowed with a registry token. If you switch
   to crates.io trusted publishing, remove `CARGO_REGISTRY_TOKEN`, add
   `id-token: write` to the release job, and publish the first version of each
   crate manually.

## Release flow

1. Push conventional commits to the default branch.
2. The PR job opens/updates `release-plz-*` with the next versions and
   per-package changelogs.
3. Merge the release PR.
4. The release job runs the test suite, publishes `koanf-core` first, then
   `koanf-rs`, creates tags (`koanf-core-vX.Y.Z`, `koanf-rs-vX.Y.Z`), and creates
   GitHub releases.

The initial `0.1.0` already in `Cargo.toml` may be published directly by the
release job on the first default-branch push; the PR job then has nothing to
bump until new commits land.

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
- Release PRs can be squashed; `release_always = false` would require a merge
  commit associated with the release PR.
- The default `GITHUB_TOKEN` cannot trigger other workflows. Add a PAT or
  GitHub App token if CI must run on release PRs or releases.
- Do not add a canceling concurrency group to the release job.
