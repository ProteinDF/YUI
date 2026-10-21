# Contributing to YUI

> [!NOTE]
> [Japanese version (日本語版)](CONTRIBUTING.ja.md) is also available.

Thank you for improving YUI. This page explains how to set up, make a change, and send it as a pull request.

## Before you start

- Read `README.md` (what YUI is, how to build and run it) and skim `architecture.md` (the design; it is the source of truth).
- `ROADMAP.md` lists what is planned and what is not.
- A plugin mechanism for adding features in Python is being designed. Until it exists, features are added to the Rust code.

## Setting up

1. Install Rust (stable) with [rustup](https://rustup.rs/).
2. For the web build and the full check, also run:
   ```sh
   rustup target add wasm32-unknown-unknown
   cargo install trunk --locked
   ```
3. Get the code and run the desktop app:
   ```sh
   git clone https://github.com/ProteinDF/YUI.git
   cd YUI
   git checkout develop
   cargo run -p yui-frontend --release
   ```

## Making a change

1. Create a branch from `develop` (git-flow): `git checkout -b feature/<short-name> develop`. Do not commit to `main` or `develop` directly. If you do not have write access, fork the repository and create the branch in your fork.
2. Keep each pull request to one topic.
3. Add or update tests with the change (`cargo test --workspace`).
4. Update the documents together with the code:
   - a user-visible change goes into `CHANGELOG.md` and `CHANGELOG.ja.md` (`Unreleased`);
   - a design decision goes into `architecture.md` and `architecture.ja.md`;
   - the English documents are canonical; keep the Japanese ones (`*.ja.md`) in sync.
5. Write commit messages in English, following [Conventional Commits](https://www.conventionalcommits.org/): `<type>(<scope>): <summary>`, where type is one of `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `build`, `ci`, `chore`.

## Checking

Run the full check before opening a pull request:

```sh
scripts/check.sh --trunk
```

All 8 items must PASS. The same checks also run on GitHub for every pull request.

## Opening a pull request

- Target `develop`.
- Describe what changed and why, how you checked it (paste the summary of `scripts/check.sh --trunk`), and anything you could not check.
- A maintainer reviews it and merges it.

## Reporting problems

Open an issue at <https://github.com/ProteinDF/YUI/issues> with the steps to reproduce, what you expected, and what happened (OS, and desktop or browser).

## License

YUI is licensed under the GNU General Public License v3.0 (`LICENSE`). By contributing, you agree that your contribution is licensed under the same license.
