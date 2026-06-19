#!/bin/bash

# Build script for rsmdu-wasm

set -e

# Ensure we use rustup instead of Homebrew Rust
export PATH="$HOME/.cargo/bin:$PATH"

echo "🔨 Building rsmdu-wasm..."

# Check if wasm-pack is installed
if ! command -v wasm-pack &> /dev/null; then
    echo "❌ wasm-pack is not installed. Install it with: cargo install wasm-pack"
    exit 1
fi

# Check if wasm32-unknown-unknown target is installed
echo "🔍 Checking for wasm32-unknown-unknown target..."
if ! rustup target list --installed 2>/dev/null | grep -q "wasm32-unknown-unknown"; then
    echo "⚠️  wasm32-unknown-unknown target not found."
    echo ""
    echo "Please run the fix script first:"
    echo "  ./fix-wasm-target.sh"
    echo ""
    echo "Or install manually:"
    echo "  rustup target add wasm32-unknown-unknown"
    echo ""
    exit 1
fi

echo "✅ wasm32-unknown-unknown target is available"

# Build WASM package
echo "📦 Building WASM package..."
wasm-pack build --target web --out-dir examples/pkg

echo "✅ Build complete!"
echo ""
echo "To serve the example:"
echo "  cd examples && python3 -m http.server 8000"
echo "Then open: http://localhost:8000/dem.html"

