#!/bin/bash

# Setup script for Administration API

set -e

echo "Checking Rust installation..."

if ! command -v rustc &> /dev/null; then
    echo "Rust is not installed."
    echo "Please install Rust from https://rustup.rs/"
    echo ""
    echo "Or run:"
    echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
fi

echo "Rust version: $(rustc --version)"
echo "Cargo version: $(cargo --version)"

echo ""
echo "Building project..."

cargo build

echo ""
echo "Build successful!"
echo ""
echo "To run the API:"
echo "  cargo run"
echo ""
echo "Make sure PostgreSQL is running and DATABASE_URL is set."
