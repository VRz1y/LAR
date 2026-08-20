#!/usr/bin/env bash
set -euo pipefail

root="${1:-$(pwd)/android-runtime}"
mkdir -p "$root"

versions=(
  "35:15:15.0.0-latest"
  "36:16:16.0.0-latest"
)

printf 'api\tandroid\ttag\tdigest\ttier\tstatus\n' > "$root/manifest.tsv"

for item in "${versions[@]}"; do
  api="${item%%:*}"
  item="${item#*:}"
  version="${item%%:*}"
  tag="${item#*:}"
  if [[ "$api" == 36 ]]; then
    tier="primary"
  else
    tier="secondary"
  fi
  image="redroid/redroid:$tag"
  name="lar_redroid_${version//./_}"
  directory="$root/android${version//./_}"
  archive="$root/android${version//./_}_runtime_bundle.tar"

  if ! docker manifest inspect "$image" >/dev/null 2>&1; then
    printf '%s\t%s\t%s\t-\t%s\tunsupported\n' "$api" "$version" "$tag" "$tier" >> "$root/manifest.tsv"
    continue
  fi

  docker pull "$image"
  digest="$(docker image inspect "$image" --format '{{index .RepoDigests 0}}' | awk -F@ '{print $2}')"
  docker rm -f "$name" >/dev/null 2>&1 || true
  docker create --name "$name" "$image" >/dev/null
  docker export "$name" -o "$archive"
  docker rm "$name" >/dev/null
  rm -rf "$directory"
  mkdir -p "$directory"
  tar -xf "$archive" -C "$directory" \
    system/apex \
    system/framework \
    system/lib64 \
    system/bin \
    system/etc

  if find "$directory" -path '*/lib64/libart.so' -type f | grep -q . \
    && find "$directory" \( -name dex2oat -o -name dex2oat64 \) -type f | grep -q . \
    && find "$directory" -name core-oj.jar -type f | grep -q . \
    && find "$directory" -name core-libart.jar -type f | grep -q . \
    && test -f "$directory/system/framework/framework.jar"; then
    status="ready"
  else
    status="incomplete"
  fi

  printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$api" "$version" "$tag" "$digest" "$tier" "$status" >> "$root/manifest.tsv"
done

cat "$root/manifest.tsv"
