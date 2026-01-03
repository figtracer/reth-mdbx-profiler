#!/bin/bash
# Setup script for the MDBX profiler
# Run this on the machine where your application is running

set -e

echo "=== application eBPF Profiler - Node Setup ==="
echo

# Check kernel version
KERNEL_VERSION=$(uname -r | cut -d. -f1-2)
KERNEL_MAJOR=$(echo $KERNEL_VERSION | cut -d. -f1)
KERNEL_MINOR=$(echo $KERNEL_VERSION | cut -d. -f2)

echo "Kernel version: $(uname -r)"

if [ "$KERNEL_MAJOR" -lt 5 ] || ([ "$KERNEL_MAJOR" -eq 5 ] && [ "$KERNEL_MINOR" -lt 8 ]); then
    echo "WARNING: Kernel version < 5.8 detected"
    echo "Some eBPF features may not be available"
    echo "Recommended: kernel 5.8+ for full ring buffer support"
fi

# Check BTF availability
if [ -f /sys/kernel/btf/vmlinux ]; then
    echo "✓ BTF (BPF Type Format) available"
else
    echo "✗ BTF not available - need to install kernel debug info"
    echo "  On Ubuntu/Debian: apt install linux-headers-$(uname -r)"
    echo "  On Fedora: dnf install kernel-devel"
fi

# Install dependencies
echo
echo "Installing dependencies..."

if command -v apt &> /dev/null; then
    sudo apt-get update -qq
    sudo apt-get install -y \
        clang \
        llvm \
        libbpf-dev \
        linux-headers-$(uname -r) \
        linux-tools-common \
        linux-tools-$(uname -r) \
        pkg-config \
        libelf-dev \
        zlib1g-dev \
        build-essential

    # Check if cargo is installed
    if ! command -v cargo &> /dev/null; then
        echo "Installing Rust..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        source "$HOME/.cargo/env"
    fi
elif command -v dnf &> /dev/null; then
    sudo dnf install -y \
        clang \
        llvm \
        libbpf-devel \
        kernel-devel \
        bpftool \
        cargo \
        elfutils-libelf-devel \
        zlib-devel
elif command -v pacman &> /dev/null; then
    sudo pacman -S --noconfirm \
        clang \
        llvm \
        libbpf \
        linux-headers \
        bpf \
        rust \
        elfutils \
        zlib
else
    echo "Unknown package manager - please install manually:"
    echo "  clang, llvm, libbpf-dev, linux-tools, cargo"
fi

# Generate vmlinux.h if BTF is available
if [ -f /sys/kernel/btf/vmlinux ]; then
    echo
    echo "Generating vmlinux.h from kernel BTF..."
    SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

    # Try bpftool from different locations
    BPFTOOL=""
    if command -v bpftool &> /dev/null; then
        BPFTOOL="bpftool"
    elif [ -x /usr/lib/linux-tools/$(uname -r)/bpftool ]; then
        BPFTOOL="/usr/lib/linux-tools/$(uname -r)/bpftool"
    elif [ -x /usr/lib/linux-tools-$(uname -r)/bpftool ]; then
        BPFTOOL="/usr/lib/linux-tools-$(uname -r)/bpftool"
    fi

    if [ -n "$BPFTOOL" ]; then
        $BPFTOOL btf dump file /sys/kernel/btf/vmlinux format c > "$SCRIPT_DIR/../bpf/vmlinux.h"
        echo "✓ Generated vmlinux.h"
    else
        echo "✗ bpftool not found - vmlinux.h not generated"
        echo "  Try: sudo apt install linux-tools-$(uname -r)"
    fi
fi

# Find application-related process (app, exex, or any process using mdbx)
echo
echo "Looking for application/ExEx process..."

# Try multiple patterns
RETH_PID=""
RETH_NAME=""

# Check for common app-related processes
for pattern in "app" "exex" "blob-exex" "app-exex"; do
    PID=$(pgrep -f "$pattern" 2>/dev/null | head -1 || echo "")
    if [ -n "$PID" ]; then
        RETH_PID="$PID"
        RETH_NAME="$pattern"
        break
    fi
done

# Also check systemd services
if [ -z "$RETH_PID" ]; then
    for service in "app" "blob-exex" "app-exex"; do
        if systemctl is-active --quiet "$service.service" 2>/dev/null; then
            PID=$(systemctl show -p MainPID "$service.service" 2>/dev/null | cut -d= -f2)
            if [ -n "$PID" ] && [ "$PID" != "0" ]; then
                RETH_PID="$PID"
                RETH_NAME="$service (systemd)"
                break
            fi
        fi
    done
fi

# Last resort: find any process with mdbx mapped
if [ -z "$RETH_PID" ]; then
    for pid in $(ls /proc | grep -E '^[0-9]+$'); do
        if grep -q "mdbx" /proc/$pid/maps 2>/dev/null; then
            RETH_PID="$pid"
            RETH_NAME="(process with mdbx: $(cat /proc/$pid/comm 2>/dev/null || echo 'unknown'))"
            break
        fi
    done
fi

if [ -n "$RETH_PID" ]; then
    echo "✓ Found process: PID $RETH_PID - $RETH_NAME"

    # Find MDBX files
    echo
    echo "MDBX memory mappings:"
    grep -E "mdbx|\.dat" /proc/$RETH_PID/maps 2>/dev/null | head -20 || echo "  (none found)"

    # Get MDBX data directory
    MDBX_PATH=$(grep -oE "/[^ ]+mdbx\.dat" /proc/$RETH_PID/maps 2>/dev/null | head -1 || echo "")
    if [ -n "$MDBX_PATH" ]; then
        echo
        echo "MDBX data file: $MDBX_PATH"
        ls -lh "$MDBX_PATH" 2>/dev/null || true

        echo
        echo "=== Quick start commands ==="
        echo "sudo ./scripts/collect-trace.sh 30"
        echo "# or manually:"
        echo "sudo ./target/release/mdbx-profiler trace --pid $RETH_PID --mdbx-path $MDBX_PATH --duration 30s"
    fi
else
    echo "✗ No application/ExEx process found"
    echo "  Checked: app, exex, blob-exex, app-exex"
    echo "  Also checked for any process with mdbx mapped"
    echo
    echo "  List running services:"
    echo "    systemctl list-units --type=service --state=running | grep -i app"
    echo "    systemctl list-units --type=service --state=running | grep -i exex"
fi

echo
echo "=== Setup complete ==="
