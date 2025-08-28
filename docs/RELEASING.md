# Releasing and version bumps

This workspace uses cargo-release for publishing crates and shared-version = true (see release.toml). “Bump once” means the entire workspace shares a single version; when we bump, all publishable crates move from X.Y.Z to the next version together.

Key points
- One version for all crates: shared-version = true ensures a single number is used across crates published to crates.io. You don’t need to manage per-crate versions manually.
- Internal dependency requirements: We configure dependent-version = "upgrade" so that when we bump (patch/minor/major), cargo-release also updates internal dependency requirements to the new version range. This is crucial for minor/major bumps because caret requirements like "0.1" don’t match "0.2" under SemVer.

There are two ways we drive releases and the behavior differs:

1) Tag-driven release (default path)
- How to trigger: make release VERSION=X.Y.Z (creates and pushes tag vX.Y.Z). The GitHub Actions workflow (Release (cargo-release)) runs on tag push.
- What cargo-release does in CI: The publish step runs cargo release per crate with --execute --no-tag --no-push. This means cargo-release performs the version bump and publishes to crates.io inside the CI workspace, but it does not push the version-bump commits back to the repository.
- Result:
  - Crates on crates.io are published at the new version X.Y.Z.
  - Git main branch does not get a version-bump commit from CI. The repo continues to show the previous version numbers in Cargo.toml until the next time we decide to bump in Git (see options below).

2) Manually-driven bump (optional alternative)
- If you want the version-bump commits to land in Git (e.g., for a release PR), run cargo release locally with push enabled (no --no-push) or adjust the workflow to allow pushing back.
- Alternative workflow: run a “prepare release” step on a branch (cargo release <level> --no-publish --execute --no-tag --no-push), open a PR with the version bump, merge it, and then tag vX.Y.Z to publish.

Frequently asked questions
- What does “shared-version” mean? With shared-version = true, you bump a single version for the whole workspace; cargo-release updates all crates together.
- Will major/minor bumps keep internal dependencies consistent? Yes. With dependent-version = "upgrade", internal dependency requirements are automatically updated to the new version range during the bump, so publishing continues to work.
- Is there a commit that bumps versions?
  - In our default tag-driven CI path: Yes, cargo-release performs a bump in the CI workspace during publish, but we pass --no-push, so that commit is not pushed back to Git. In other words: the bump happens “during” release in CI, and the commit stays in CI only.
  - If you want a Git-visible bump commit, use the manual or PR-based approach described above.
- Before vs during release?
  - Default: The bump occurs during the publish stage in CI (after the tag is pushed and the workflow starts). No pre-release bump commit is added to the repo.
  - Alternative: Do a “prepare” bump before tagging (branch + PR), then tag and publish.

Practical commands
- Tag-driven (default):
  - make release VERSION=X.Y.Z
  - CI will publish all crates at X.Y.Z (no bump commit pushed back).
- Per-crate tag (for subset):
  - make release-crate CRATE=name VERSION=X.Y.Z (creates name-vX.Y.Z tag)
- Manual bump & PR (optional):
  - cargo install cargo-release
  - cargo release patch --workspace --no-publish --execute --no-tag --no-push
  - Commit is created locally; open a PR, merge, then tag, and let CI publish.

Notes
- The Release workflow also supports manual dispatch with inputs (ref, packages, level) if you prefer not to use tags.
- With dependent-version = "upgrade", inner workspace dependency specs are kept aligned across all crates at each release level.
