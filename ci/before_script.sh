#!/bin/bash -e

rustup component add rustfmt-preview --toolchain=$TRAVIS_RUST_VERSION
if test "$TRAVIS_RUST_VERSION" = "nightly"; then
    test -x $HOME/.cargo/bin/cargo-install-update || cargo install cargo-update
    test -x $HOME/.cargo/bin/cargo-clippy || cargo install clippy
    cargo install-update clippy
fi
