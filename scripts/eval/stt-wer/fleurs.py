# /// script
# dependencies = ["requests==2.34.2"]
# ///
"""Stream the first N test utterances of FLEURS languages out of HF's test.tar.gz, stopping early.

usage: fleurs.py <out_dir> <n> <lang>...   writes <out_dir>/<lang>/<file>.wav and <out_dir>/<lang>.tsv (id, wav, ref)
"""
import csv, io, os, sys, tarfile, requests

out, n, langs = sys.argv[1], int(sys.argv[2]), sys.argv[3:]
base = "https://huggingface.co/datasets/google/fleurs/resolve/main/data"
for lang in langs:
    tsv = requests.get(f"{base}/{lang}/test.tsv", timeout=60).text
    # id, file_name, raw_transcription, transcription, phonemes, num_samples, gender
    refs = {r[1]: r[2] for r in csv.reader(io.StringIO(tsv), delimiter="\t", quoting=csv.QUOTE_NONE)}
    os.makedirs(f"{out}/{lang}", exist_ok=True)
    rows, seen_sentence = [], set()
    with requests.get(f"{base}/{lang}/audio/test.tar.gz", stream=True, timeout=60) as r:
        r.raise_for_status()
        with tarfile.open(fileobj=r.raw, mode="r|gz") as tar:
            for m in tar:
                name = os.path.basename(m.name)
                if not m.isfile() or name not in refs:
                    continue
                # FLEURS repeats a sentence across speakers; keep one reading per sentence.
                if refs[name] in seen_sentence:
                    continue
                seen_sentence.add(refs[name])
                path = f"{out}/{lang}/{name}"
                with open(path, "wb") as f:
                    f.write(tar.extractfile(m).read())
                rows.append((f"{lang}/{name}", path, refs[name]))
                if len(rows) >= n:
                    break
    with open(f"{out}/{lang}.tsv", "w") as f:
        for row in rows:
            f.write("\t".join(row) + "\n")
    print(lang, len(rows), flush=True)
