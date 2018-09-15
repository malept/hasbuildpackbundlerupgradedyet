#!/bin/bash -xe

if test "$TRAVIS_OS_NAME" = "linux" -a "$TRAVIS_RUST_VERSION" = "stable"; then
    rustup component add rustfmt-preview clippy-preview --toolchain=$TRAVIS_RUST_VERSION
fi
