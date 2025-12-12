# Agent Guidelines for rs-coroutine-rs-flow

This document contains guidelines for AI agents working on this repository.

## Pre-PR Checklist

Before creating a pull request, **always** run these CI commands to ensure the code passes continuous integration:

### 1. Formatting Check
```bash
cargo fmt --all -- --check
```

If this fails, fix it with:
```bash
cargo fmt --all
```

### 2. Build Check
```bash
cargo build --all
```

### 3. Clippy Lints
```bash
cargo clippy --all -- -D warnings
```

### 4. Test Suite
```bash
cargo test --all
```

### 5. Documentation Build
```bash
cargo doc --no-deps --all
```

## Complete Pre-PR Command

Run all checks in one command:
```bash
cargo fmt --all && \
cargo clippy --all -- -D warnings && \
cargo build --all && \
cargo test --all && \
cargo doc --no-deps --all
```

## After Fixing Issues

Once all checks pass:
1. Commit the changes with a descriptive message
2. Push to the feature branch
3. Create or update the pull request

## Important Notes

- **Never commit unformatted code** - Always run `cargo fmt --all` before committing
- **Fix all clippy warnings** - The CI uses `-D warnings` which treats warnings as errors
- **Ensure all tests pass** - Don't commit if tests are failing
- **Document public APIs** - All public functions, types, and modules should have documentation

## CI Configuration

This repository uses GitHub Actions for CI. The workflows check:
- Code formatting (rustfmt)
- Linting (clippy)
- Build success
- Test passage
- Documentation generation

Match your local checks to the CI environment to avoid failed builds.
