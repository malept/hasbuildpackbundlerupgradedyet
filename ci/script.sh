#!/bin/bash -xe

cargo test

if test "$TRAVIS_OS_NAME" = "linux" -a "$TRAVIS_RUST_VERSION" = "stable"; then
    cargo doc

    if [ "$TRAVIS_PULL_REQUEST" != "false" ]; then
        cargo fmt -- --check $(git diff --name-only "$TRAVIS_COMMIT" "$TRAVIS_BRANCH" | grep \.rs$)
    else
        cargo fmt -- --check $(git show --format= --name-only "$TRAVIS_COMMIT_RANGE" | sort -u | grep \.rs$)
    fi

    cargo clippy -- --allow clippy_pedantic
fi
