#!/usr/bin/env bash
# CMake flags that keep the ggml CPU backend portable across x86-64 machines: never tune to the build host
# (GGML_NATIVE=OFF), Haswell tier (AVX2, FMA, F16C, BMI2), no AVX-512. Used by both worker builds and
# verified by tools/check-worker-isa.sh.
echo "-DGGML_NATIVE=OFF -DGGML_SSE42=ON -DGGML_AVX=ON -DGGML_AVX2=ON -DGGML_FMA=ON -DGGML_F16C=ON -DGGML_BMI2=ON -DGGML_AVX512=OFF -DGGML_AVX512_VBMI=OFF -DGGML_AVX512_VNNI=OFF -DGGML_AVX512_BF16=OFF -DGGML_AVX_VNNI=OFF"
