#!/bin/bash -e

rustup component add rustfmt-preview --toolchain=$TRAVIS_RUST_VERSION
if test "$TRAVIS_RUST_VERSION" = "nightly"; then
    rustup component add clippy-preview
fi
