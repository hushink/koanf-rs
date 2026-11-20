## Summary

<!-- What changed, and why? Keep it to a few sentences. -->

## Related issue

<!-- Use "Closes #123" to close the issue on merge, or "Refs #123" for related work. -->

Closes #

## Type of change

- [ ] Bug fix
- [ ] New feature
- [ ] Documentation
- [ ] Refactor or maintenance
- [ ] Breaking change

## Affected crates

- [ ] `koanf-core`
- [ ] `koanf-derive`
- [ ] `koanf-rs`
- [ ] Workspace tooling, CI, or release

<!-- Mention affected sources, formats, watch, schema, async, or no_std/WASM
     behavior in the summary or below. -->

## Public API and semver impact

<!-- Describe any public API changes. State whether the change is additive,
     a breaking change, or has no API impact. release-plz uses semver checks
     and Conventional Commits to choose the next version. -->

## Test plan

<!-- Give exact commands or manual steps a reviewer can reproduce. CI already
     runs formatting, clippy, workspace tests, docs, MSRV, no_std, and wasm
     checks; list any additional checks here. -->

```text
cargo test -p koanf-core
```

## Breaking changes and migration

<!-- Delete this section if not applicable. Describe what breaks and how users
     should migrate. -->

## Checklist

- [ ] I added or updated tests for the behavior change.
- [ ] I updated documentation or examples when user-facing behavior changed.
- [ ] Commit messages follow Conventional Commits (`feat(scope):`, `fix(scope):`, etc.); I did not edit version numbers or changelogs by hand.
- [ ] I removed secrets and personal data from code, logs, and test fixtures.
