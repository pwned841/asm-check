# NASM 2.16.01, the version Ubuntu 24.04 ships, to check asm-check against.
#   docker build -t asmcheck-nasm scripts
FROM --platform=linux/amd64 ubuntu:24.04
RUN apt-get update \
 && apt-get install -y --no-install-recommends nasm binutils \
 && rm -rf /var/lib/apt/lists/*
