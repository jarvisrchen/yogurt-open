#!/usr/bin/env bash
# WER, speed, and peak memory of local transcription models through yogurt's own load and decode code.
#
#   scripts/eval/stt-wer/run.sh <work dir> parakeet-v3 parakeet-ultra > table.md
#
# Downloads about 1 GB of LibriSpeech, FLEURS, and DEMAND noise into <work dir>, builds the sets, runs every set through
# each model (interleaved, so load from other work hits both alike), and prints score.py's table.
# The models load from MODELS_DIR, ~/.yogurt/models by default, where Settings downloads them.
# Speech is rendered with `say -o` into files; nothing plays through the speaker.
set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../../.." && pwd)
mkdir -p "$1/data" "$1/out"
W=$(cd "$1" && pwd)
shift
MODELS_DIR=${MODELS_DIR:-$HOME/.yogurt/models}
cd "$W/data"

fetch() { [ -s "$2" ] || { curl -fL --retry 3 -o "$2.part" "$1" && mv "$2.part" "$2"; }; }
for s in test-clean test-other; do
  [ -d "LibriSpeech/$s" ] || { fetch "https://www.openslr.org/resources/12/$s.tar.gz" "$s.tar.gz" && tar xzf "$s.tar.gz" && rm "$s.tar.gz"; }
done
for n in PCAFETER OOFFICE; do fetch "https://zenodo.org/records/1227121/files/${n}_16k.zip?download=1" "${n}_16k.zip"; done
[ -s fleurs/it_it.tsv ] || uv run --script "$HERE/fleurs.py" "$W/data/fleurs" 100 de_de es_419 fr_fr it_it
[ -s "$W/sets/long-noisy-10db.tsv" ] || uv run --script "$HERE/prep.py" "$W" "$ROOT/scripts/eval/conversation.txt"

cargo build --release --manifest-path "$ROOT/Cargo.toml" -p yogurt-stt --features local-stt --example stt_eval
BIN=$ROOT/target/release/examples/stt_eval

run() { # model set whole|vad
  local tag=$1.$2.$3 flag=
  [ "$3" = vad ] && flag=--vad
  # getrusage reports the child's peak RSS. The library path is set inside Python because SIP strips DYLD_ variables
  # from protected binaries such as /usr/bin/time, /bin/bash, and a pyenv shim.
  python3 -c 'import os, resource, subprocess, sys
rc = subprocess.run(sys.argv[2:], env=dict(os.environ, DYLD_LIBRARY_PATH=sys.argv[1])).returncode
print(f"maxrss_mb={resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss / 1e6:.0f}", file=sys.stderr)
sys.exit(rc)' "$(dirname "$BIN")" "$BIN" "$1" "$MODELS_DIR" "$W/sets/$2.tsv" $flag > "$W/out/$tag.hyp" 2> "$W/out/$tag.err"
  echo "$tag $(tail -2 "$W/out/$tag.err" | tr '\n' ' ')" >&2
}
for s in "$W"/sets/*.tsv; do
  s=$(basename "$s" .tsv)
  for m in "$@"; do
    run "$m" "$s" whole
    case $s in long*) run "$m" "$s" vad ;; esac
  done
done
uv run --script "$HERE/score.py" "$W" "$@"
