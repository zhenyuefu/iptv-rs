#!/bin/sh
set -eu

: "${IMAGE:?Set IMAGE to your DockerHub image name, for example IMAGE=zhenyuefu/iptv-rs}"
: "${TAG:=latest}"

PLATFORMS="${PLATFORMS:-linux/amd64,linux/arm64}"

docker buildx build \
    --platform "$PLATFORMS" \
    -t "$IMAGE:$TAG" \
    -t "$IMAGE:latest" \
    --push \
    .
