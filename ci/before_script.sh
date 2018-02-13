#!/bin/bash -e

test -x $HOME/.cargo/bin/cargo-install-update || cargo install cargo-update

if test "$TRAVIS_RUST_VERSION" != "stable"; then
    rustup component add rustfmt-preview --toolchain=$TRAVIS_RUST_VERSION
fi

if test "$TRAVIS_RUST_VERSION" = "nightly"; then
    test -x $HOME/.cargo/bin/cargo-clippy || cargo install clippy
    cargo install-update clippy
fi
