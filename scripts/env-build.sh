# Source before cargo build on this VPS (no system gcc).
#   source scripts/env-build.sh
#   cargo build -p mega-save-x --release

export MAMBA_ROOT_PREFIX="${MAMBA_ROOT_PREFIX:-$HOME/.local/opt/mamba-root}"
export PATH="$HOME/.local/opt/micromamba/bin:$HOME/.cargo/bin:$HOME/.local/bin:$PATH"

if command -v micromamba >/dev/null 2>&1; then
  eval "$(micromamba shell hook -s bash 2>/dev/null)" || true
  micromamba activate rustbuild 2>/dev/null || true
fi

# conda-forge compiler names
if command -v x86_64-conda-linux-gnu-cc >/dev/null 2>&1; then
  export CC="${CC:-x86_64-conda-linux-gnu-cc}"
  export CXX="${CXX:-x86_64-conda-linux-gnu-c++}"
  export AR="${AR:-x86_64-conda-linux-gnu-ar}"
  # Tell cargo/rustc to use the same linker driver
  export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="${CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER:-$CC}"
fi

export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
