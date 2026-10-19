#!/bin/sh
# Generated/maintained with AI; see repository disclaimers.md.
# Compile-only smoke checks: no GPU execution and no reduced optimization.
set -eu
cd "$(dirname "$0")"
: "${CONDA_PREFIX:?Run with pixi run arch-check}"
: "${CXX:?Run with pixi run arch-check}"
if [ "$#" -eq 0 ]; then
  set -- 89 120
fi
for arch in "$@"; do
  case "$arch" in
    89|120) ;;
    *) printf 'Supported CUDA architectures are 89 and 120, got: %s\n' "$arch" >&2; exit 2 ;;
  esac
  build_dir="build/compile-smoke-$arch"
  printf 'Compile-checking sm_%s\n' "$arch"
  timeout -k 5s 60s env -u NVCC_PREPEND_FLAGS cmake -S . -B "$build_dir" -G Ninja \
    -DCMAKE_BUILD_TYPE=RelWithDebInfo \
    -DCMAKE_CUDA_COMPILER="$CONDA_PREFIX/bin/nvcc" \
    -DCMAKE_CUDA_HOST_COMPILER="$CXX" -DCMAKE_CUDA_FLAGS= \
    -DCMAKE_CUDA_ARCHITECTURES="$arch-real"
  timeout -k 5s 180s env -u NVCC_PREPEND_FLAGS cmake --build "$build_dir" --clean-first -j 2
done
