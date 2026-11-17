# /// script
# dependencies = ["numpy==2.4.6", "soundfile==0.14.0"]
# ///
"""Build eval manifests (id, wav) and references (id, text) under <eval>/sets/.

usage: prep.py <eval_dir> <conversation.txt>
Reads what run.sh downloads into <eval>/data: LibriSpeech, FLEURS from fleurs.py, and the DEMAND PCAFETER and OOFFICE zips.
Renders the meeting script with `say -o` (to files, never the speaker).
"""
import glob, os, random, subprocess, sys, zipfile
import numpy as np, soundfile as sf

E, CONV = sys.argv[1], sys.argv[2]
D, S = f"{E}/data", f"{E}/sets"
os.makedirs(S, exist_ok=True)
rng = random.Random(16)


def write_set(name, rows):
    with open(f"{S}/{name}.tsv", "w") as m, open(f"{S}/{name}.ref", "w") as r:
        for id_, wav, text in rows:
            m.write(f"{id_}\t{wav}\n")
            r.write(f"{id_}\t{text}\n")
    print(name, len(rows), flush=True)


def save(path, x):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    sf.write(path, np.clip(x, -1, 1), 16000, subtype="PCM_16")


# LibriSpeech: 200 utterances per set, spread over every speaker.
ls = {}
for split in ["test-clean", "test-other"]:
    trans = {}
    for t in glob.glob(f"{D}/LibriSpeech/{split}/*/*/*.trans.txt"):
        for line in open(t):
            k, v = line.strip().split(" ", 1)
            trans[k] = v
    by_spk = {}
    for k in sorted(trans):
        by_spk.setdefault(k.split("-")[0], []).append(k)
    picked = []
    while len(picked) < 200:
        for spk in sorted(by_spk):
            if by_spk[spk] and len(picked) < 200:
                picked.append(by_spk[spk].pop(rng.randrange(len(by_spk[spk]))))
    rows = []
    for k in picked:
        s, c, _ = k.split("-")
        x, sr = sf.read(f"{D}/LibriSpeech/{split}/{s}/{c}/{k}.flac", dtype="float32")
        assert sr == 16000
        wav = f"{E}/wav/{split}/{k}.wav"
        save(wav, x)
        rows.append((f"{split}/{k}", wav, trans[k]))
    ls[split] = rows
    write_set(split, rows)

# FLEURS: 16 kHz float WAVs, rewritten as the 16-bit PCM the harness reads.
for tsv in sorted(glob.glob(f"{D}/fleurs/*.tsv")):
    lang = os.path.basename(tsv)[:-4]
    rows = []
    for id_, src, text in (l.rstrip("\n").split("\t") for l in open(tsv)):
        x, sr = sf.read(src, dtype="float32")
        assert sr == 16000, src
        wav = f"{E}/wav/fleurs/{id_}"
        save(wav, x)
        rows.append((id_, wav, text))
    write_set(f"fleurs-{lang}", rows)

# Noise: DEMAND cafeteria and office, channel 1, alternating per utterance.
noise = []
for n in ["PCAFETER", "OOFFICE"]:
    with zipfile.ZipFile(f"{D}/{n}_16k.zip") as z:
        name = next(f for f in z.namelist() if f.endswith("ch01.wav"))
        with z.open(name) as f:
            x, sr = sf.read(f, dtype="float32")
            assert sr == 16000
            noise.append(x)


def mix(x, snr_db, i):
    nz = noise[i % 2]
    if len(nz) <= len(x):
        nz = np.tile(nz, len(x) // len(nz) + 2)
    start = rng.randrange(len(nz) - len(x))
    n = nz[start : start + len(x)]
    ps, pn = np.mean(x**2), np.mean(n**2) + 1e-12
    return x + n * np.sqrt(ps / (pn * 10 ** (snr_db / 10)))


def noisy(base_rows, label):
    for snr in [10, 5, 0]:
        rows = []
        for i, (id_, wav, text) in enumerate(base_rows):
            x, _ = sf.read(wav, dtype="float32")
            out = f"{E}/wav/{label}-{snr}db/{id_.replace('/', '_')}.wav"
            save(out, mix(x, snr, i))
            rows.append((f"{label}-{snr}db/{id_}", out, text))
        write_set(f"{label}-{snr}db", rows)


noisy(ls["test-other"], "noisy-test-other")
de = [tuple(l.rstrip("\n").split("\t")) for l in open(f"{S}/fleurs-de_de.ref")]
de = [(id_, f"{E}/wav/fleurs/{id_}", text) for id_, text in de]
noisy(de, "noisy-fleurs-de")

# Long-form: the meeting script spoken by three voice pairs, one mono track as system audio would carry a call.
lines = []
for line in open(CONV):
    line = line.rstrip("\n")
    if line.startswith(("A: ", "B: ")):
        lines.append((line[0], line[3:]))
    elif line.startswith("PAUSE "):
        lines.append(("P", float(line[6:])))
ref = " ".join(t for who, t in lines if who != "P")
pairs = [("Samantha", "Daniel"), ("Karen", "Rishi"), ("Moira", "Fred")]
tmp = f"{E}/wav/say"
os.makedirs(tmp, exist_ok=True)
long_rows = []
for a, b in pairs:
    parts = []
    for i, (who, t) in enumerate(lines):
        if who == "P":
            parts.append(np.zeros(int(16000 * t), dtype=np.float32))
            continue
        aiff = f"{tmp}/{a}-{b}-{i}.aiff"
        subprocess.run(["say", "-v", a if who == "A" else b, "-r", "175", "-o", aiff, "--", t], check=True)
        wav = aiff[:-5] + ".wav"
        subprocess.run(["afconvert", "-f", "WAVE", "-d", "LEI16@16000", "-c", "1", aiff, wav], check=True)
        x, _ = sf.read(wav, dtype="float32")
        parts += [x, np.zeros(8000, dtype=np.float32)]
    out = f"{E}/wav/long/{a}-{b}.wav"
    save(out, np.concatenate(parts))
    long_rows.append((f"long/{a}-{b}", out, ref))
write_set("long", long_rows)
noisy_long = []
for i, (id_, wav, text) in enumerate(long_rows):
    x, _ = sf.read(wav, dtype="float32")
    out = f"{E}/wav/long-noisy/{os.path.basename(wav)}"
    save(out, mix(x, 10, i))
    noisy_long.append((f"long-cafe-office-10db/{id_}", out, text))
write_set("long-noisy-10db", noisy_long)
