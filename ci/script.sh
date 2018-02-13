#!/bin/bash -xe

cargo test

if test "$TRAVIS_OS_NAME" = "linux"; then
    if test "$TRAVIS_RUST_VERSION" = "stable"; then
        cargo doc
    else
        if [ "$TRAVIS_PULL_REQUEST" != "false" ]; then
            cargo fmt -- --write-mode=diff $(git diff --name-only "$TRAVIS_COMMIT" "$TRAVIS_BRANCH" | grep \.rs$)
        else
            cargo fmt -- --write-mode=diff $(git show --format= --name-only "$TRAVIS_COMMIT_RANGE" | sort -u | grep \.rs$)
        fi
    fi
fi

if test "$TRAVIS_RUST_VERSION" = "nightly"; then
    cargo clippy -- --allow clippy_pedantic
fi
