#!/bin/bash -e

test -x $HOME/.cargo/bin/cargo-install-update || cargo install cargo-update
test -x $HOME/.cargo/bin/rustfmt || cargo install rustfmt
cargo install-update rustfmt

if test "$TRAVIS_RUST_VERSION" = "nightly"; then
    test -x $HOME/.cargo/bin/cargo-clippy || cargo install clippy
    cargo install-update clippy
fi
