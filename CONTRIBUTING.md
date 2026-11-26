# Contributing to Endemic

Endemic is maintained by Ark Security Company (ASC). Contributions are accepted under the [Apache License 2.0](LICENSE): by opening a pull request you agree that your contribution is licensed under it.

## How changes land

1. Fork the repository, or create a branch if you have write access. Name it after the change, such as `fix/openapi-ref-cycle`.
2. Make a focused change with tests. Bug fixes start with a test that fails without the fix.
3. Run the checks below locally.
4. Open a pull request to `main` using the template. Explain what changed and why, and how you tested it.
5. A code owner reviews it. `main` is protected: it needs one approving review and a passing CI run, and direct pushes and force pushes are blocked. Maintainers squash-merge.

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/): `feat:`, `fix:`, `refactor:`, `docs:`, `test:`, `chore:`, `perf:`, `ci:`.

## Checks

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cd dashboard && npm ci && npx tsc --noEmit && npm run lint && npm test && npm run build
```

CI runs the same checks.

## Style

- Keep changes small and code self-explanatory: clear names over comments.
- Every setting is required and has no default. A new setting goes in `endemic.env.example` and `docs/reference.md`.
- Never commit secrets, tokens, or real staging URLs. Tests use local fixtures.

## Security issues

Do not open a public issue for a vulnerability. Report it through GitHub's private vulnerability reporting on this repository.
