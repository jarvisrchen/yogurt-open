#!/usr/bin/env bash
# voice-speed.sh - measure what a Voice dictionary costs in latency: Parakeet
# hotwords on the Mac and the phone, whisper's initial prompt, and dictionary
# lines in the cleanup prompt. Every row is a median over RUNS warm runs.
#
# Usage:
#   scripts/eval/voice-speed.sh                          # parakeet whisper cleanup
#   scripts/eval/voice-speed.sh parakeet phone           # some of: parakeet phone whisper cleanup
#   CLEANUP_PROVIDER=<provider id> scripts/eval/voice-speed.sh cleanup
#
# Knobs: RUNS (default 11), WHISPER_MODELS (default "base.en small.en
# large-v3-turbo"; base.en is downloaded, the others are used when installed
# in the data directory), CLEANUP_PROVIDER (a provider id from Settings, whose
# key is read from keys.json and never printed; unset runs the built-in model
# only), YOGURT_DATA_DIR (default ~/.yogurt), CACHE (downloads and builds,
# default ~/.cache/yogurt-voice-speed), ANDROID_SERIAL (the phone section's adb
# device; the yogurt emulator is shared, so take its lock first).
#
# Rows are tab separated: clip or model, case, median ms, then min and max ms
# (cleanup: median ms to the first token, median total, min and max total),
# then the text of the last run.
set -euo pipefail

cd "$(dirname "$0")/../.."
RUNS="${RUNS:-11}"
DATA="${YOGURT_DATA_DIR:-$HOME/.yogurt}"
CACHE="${CACHE:-$HOME/.cache/yogurt-voice-speed}"
NAMES="$PWD/scripts/eval/made-up-names.txt"
PARAKEET="$DATA/models/parakeet-tdt-0.6b-v3-int8"
SHERPA="$(sed -n 's/^sherpa-onnx = .*version = "=\([0-9.]*\)".*/\1/p' Cargo.toml)"
SDK="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
TARGET="${CARGO_TARGET_DIR:-$PWD/target}"
SECTIONS=" ${*:-parakeet whisper cleanup} "
mkdir -p "$CACHE"

die() { echo "voice-speed: $*" >&2; exit 1; }
wants() { [[ "$SECTIONS" == *" $1 "* ]]; }
for s in $SECTIONS; do [[ " parakeet phone whisper cleanup " == *" $s "* ]] || die "no section called $s"; done
[[ "$RUNS" =~ ^[1-9][0-9]*$ ]] || die "RUNS must be a positive number"
[ -n "$SHERPA" ] || die "could not read the sherpa-onnx version from Cargo.toml"

# The harnesses log their whole config to stderr, so it is shown only when one fails.
quiet() { "$@" 2>"$CACHE/stderr.log" || { tail -20 "$CACHE/stderr.log" >&2; die "$1 failed"; }; }

fetch() { # url file sha256
  if [ ! -s "$CACHE/$2" ]; then
    curl -fsSL -o "$CACHE/$2.part" "$1"
    mv "$CACHE/$2.part" "$CACHE/$2"
  fi
  [ "$(shasum -a 256 "$CACHE/$2" | cut -d' ' -f1)" = "$3" ] || die "$CACHE/$2 does not match its pinned SHA-256; delete it to download it again"
}

# The clips name people from made-up-names.txt, so hotwords and prompts have something to fix.
clip() { # name text
  say -o "$CACHE/$1.aiff" "$2"
  afconvert -f WAVE -d LEI16@16000 -c 1 "$CACHE/$1.aiff" "$CACHE/$1.wav"
}
if wants parakeet || wants phone || wants whisper; then
clip clip-5s "Hey, can you send the Quillane deck over to Marivex before the Zorbal sync tomorrow morning, please?"
clip clip-30s "Quick update for the team. The Quillane rollout slipped to Thursday because Marivex found a bug in the Zorbal importer. Tavimek is pairing with Brexnor on the fix this afternoon, and Telian will rerun the load test once it lands. If that passes, we ship Friday morning and I will send the release notes to the whole group. Please hold any new requests until Monday, and ping me directly if anything blocks you before then. Thanks again to everyone who stayed late on Tuesday to chase this down, it really made the difference. See you all at standup."
CLIPS=("$CACHE/clip-5s.wav" "$CACHE/clip-30s.wav")
fi

# Parakeet's download has no BPE vocab, which sherpa-onnx needs to encode hotwords.
# Scoring each piece by minus its id stands in for the sentencepiece scores.
if wants parakeet || wants phone; then
  for f in encoder.int8.onnx decoder.int8.onnx joiner.int8.onnx tokens.txt; do
    [ -s "$PARAKEET-$f" ] || die "Parakeet is not downloaded in $DATA/models"
  done
  awk '{ s = -$2; print $1, s }' "$PARAKEET-tokens.txt" > "$CACHE/bpe.vocab"
fi

if wants parakeet; then
  # The dylibs the app links, which crates/yogurt-stt/build.rs checks against its pins on every build.
  quiet cargo build -q --release -p yogurt-stt --features local-stt
  lib="$TARGET/sherpa-onnx-prebuilt/sherpa-onnx-v$SHERPA-osx-arm64-shared-lib/lib"
  mkdir -p "$CACHE/include/sherpa-onnx/c-api"
  fetch "https://raw.githubusercontent.com/k2-fsa/sherpa-onnx/v$SHERPA/sherpa-onnx/c-api/c-api.h" c-api.h \
    5698f38ba09ad2bdf085c56a6cbbd7741acf15e25f4ce5c7097b9702cadf437f
  cp "$CACHE/c-api.h" "$CACHE/include/sherpa-onnx/c-api/"
  clang -O2 -I"$CACHE/include" -L"$lib" -lsherpa-onnx-c-api -Wl,-rpath,"$lib" \
    -o "$CACHE/parakeet_hotwords" scripts/eval/parakeet_hotwords.c
  # The app's own thread count, from parakeet_local.rs.
  threads=$(( $(sysctl -n hw.ncpu) < 8 ? $(sysctl -n hw.ncpu) : 8 ))
  echo "== Parakeet on the Mac, $threads threads"
  quiet "$CACHE/parakeet_hotwords" "$PARAKEET" "$threads" "$RUNS" "$CACHE/bpe.vocab" "$NAMES" 0,20,100 "${CLIPS[@]}"
fi

if wants phone; then
  # The app's own AAR at the version and SHA-256 android/app/build.gradle.kts pins, and the Kotlin runtime its classes need.
  gradle=android/app/build.gradle.kts
  version="$(sed -n 's/^val sherpaVersion = "\(.*\)"/\1/p' "$gradle")"
  aar="sherpa-onnx-static-link-onnxruntime-$version.aar"
  fetch "https://github.com/k2-fsa/sherpa-onnx/releases/download/v$version/$aar" "$aar" \
    "$(sed -n 's/^val sherpaSha256 = "\(.*\)"/\1/p' "$gradle")"
  fetch https://repo1.maven.org/maven2/org/jetbrains/kotlin/kotlin-stdlib/2.2.21/kotlin-stdlib-2.2.21.jar kotlin-stdlib.jar \
    6558a3d233da56a20934b32159f9db5f86ed5816ef098f78a2c223dc6abb79dd
  unzip -o -q "$CACHE/$aar" classes.jar jni/arm64-v8a/libsherpa-onnx-jni.so -d "$CACHE/aar"
  android_jar="$(ls -d "$SDK"/platforms/android-*/android.jar | sort -V | tail -1)"
  d8="$(ls -d "$SDK"/build-tools/*/d8 | sort -V | tail -1)"
  cp="$CACHE/aar/classes.jar:$CACHE/kotlin-stdlib.jar:$android_jar"
  javac --release 11 -d "$CACHE" -cp "$cp" scripts/eval/ParakeetHotwords.java
  "$d8" --release --min-api 26 --lib "$android_jar" --output "$CACHE/parakeet-hotwords.jar" \
    "$CACHE/ParakeetHotwords.class" "$CACHE/aar/classes.jar" "$CACHE/kotlin-stdlib.jar"

  adb="$SDK/platform-tools/adb"
  dir=/data/local/tmp/voice-speed
  "$adb" shell mkdir -p "$dir"
  for f in "$CACHE/aar/jni/arm64-v8a/libsherpa-onnx-jni.so" "$CACHE/parakeet-hotwords.jar" \
    "$CACHE/bpe.vocab" "$NAMES" "${CLIPS[@]}"; do
    "$adb" push -q "$f" "$dir/"
  done
  # The model is 670 MB and never changes in place, so a copy of the same size is the same file.
  for f in "$PARAKEET"-*.onnx "$PARAKEET-tokens.txt"; do
    [ "$("$adb" shell stat -c %s "$dir/$(basename "$f")" 2>/dev/null | tr -d '\r')" = "$(stat -f %z "$f")" ] \
      || "$adb" push -q "$f" "$dir/"
  done
  # The app's own thread count, from LocalRecorder.kt.
  threads=$("$adb" shell nproc | tr -d '\r'); threads=$(( threads < 4 ? threads : 4 ))
  echo "== Parakeet on $("$adb" shell getprop ro.product.model | tr -d '\r'), $threads threads"
  quiet "$adb" shell "cd $dir && CLASSPATH=$dir/parakeet-hotwords.jar app_process -Djava.library.path=$dir $dir \
    ParakeetHotwords ./parakeet-tdt-0.6b-v3-int8 $threads $RUNS bpe.vocab made-up-names.txt 0,20,100 clip-5s.wav clip-30s.wav"
fi

if wants whisper; then
  for m in ${WHISPER_MODELS:-base.en small.en large-v3-turbo}; do
    model="$DATA/models/ggml-$m.bin"
    if [ "$m" = base.en ]; then
      fetch https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin ggml-base.en.bin \
        a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002
      model="$CACHE/ggml-base.en.bin"
    fi
    [ -s "$model" ] || { echo "== whisper $m is not installed, skipped"; continue; }
    echo "== whisper $m, initial prompt"
    quiet cargo run -q --release -p yogurt-stt --features local-stt --example initial_prompt_cost -- \
      "$model" "$NAMES" "$RUNS" 0,50 "${CLIPS[@]}"
  done
fi

if wants cleanup; then
  cost() { quiet cargo run -q --release -p yogurt-llm --features local --example cleanup_dictionary_cost -- "$@"; }
  qwen="$DATA/models/Qwen3-4B-Instruct-2507-Q4_K_M.gguf"
  [ -s "$qwen" ] || die "Qwen3 4B is not downloaded in $DATA/models"
  echo "== cleanup, built-in"
  cost "$qwen" "$NAMES" "$RUNS" 0,10,100 system
  # With no lines the two placements send the same prompt, so 0 runs once.
  cost "$qwen" "$NAMES" "$RUNS" 10,100 user
  if [ -n "${CLEANUP_PROVIDER:-}" ]; then
    [[ "$CLEANUP_PROVIDER" =~ ^[A-Za-z0-9_-]+$ ]] || die "CLEANUP_PROVIDER is not a provider id"
    row="$(sqlite3 -readonly "$DATA/db.sqlite" \
      "select base_url || '|' || model from providers where id = '$CLEANUP_PROVIDER' and adapter = 'http'")"
    [ -n "$row" ] || die "no HTTP provider has the id $CLEANUP_PROVIDER"
    key="$(jq -r --arg id "$CLEANUP_PROVIDER" '.[$id] // empty' "$DATA/keys.json")"
    [ -n "$key" ] || die "keys.json has no key for $CLEANUP_PROVIDER"
    echo "== cleanup, ${row#*|}"
    export CLEANUP_BASE_URL="${row%%|*}" CLEANUP_MODEL="${row#*|}" CLEANUP_KEY="$key"
    cost openai "$NAMES" "$RUNS" 0,10,100 system
    cost openai "$NAMES" "$RUNS" 10,100 user
  fi
fi
