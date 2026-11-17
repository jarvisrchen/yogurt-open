# /// script
# dependencies = ["jiwer==4.0.0", "whisper-normalizer==1.0.1"]
# ///
"""score.py <work dir> <model>...: a Markdown table of WER, real-time factor, and peak memory per set, one column group per model.

Reads out/<model>.<set>.<mode>.hyp and .err, which run.sh writes, against sets/<set>.ref.
English sets use Whisper's EnglishTextNormalizer (case, punctuation, numbers, spelling), FLEURS its BasicTextNormalizer.
WER is corpus-level: all edits over all reference words in the set.
"""
import glob, os, re, sys
import jiwer
from whisper_normalizer.basic import BasicTextNormalizer
from whisper_normalizer.english import EnglishTextNormalizer

W, models = sys.argv[1], sys.argv[2:]
en, basic = EnglishTextNormalizer(), BasicTextNormalizer()
rows, loads = {}, {m: [] for m in models}
for hyp in sorted(glob.glob(f"{W}/out/*.hyp")):
    model, set_, mode = os.path.basename(hyp)[:-4].rsplit(".", 2)
    if model not in models:
        continue
    err = open(hyp[:-4] + ".err").read()
    if "load_s=" not in err or "maxrss_mb=" not in err:
        continue  # still running, or stt_eval failed before its summary line
    norm = basic if "fleurs" in set_ else en
    ref = dict(l.rstrip("\n").split("\t", 1) for l in open(f"{W}/sets/{set_}.ref"))
    R, H, audio, dec = [], [], 0.0, 0.0
    for line in open(hyp):
        id_, a, d, *t = line.rstrip("\n").split("\t")
        R.append(norm(ref[id_]))
        H.append(norm(t[0] if t else ""))
        audio += float(a)
        dec += float(d)
    loads[model].append(float(re.search(r"load_s=([\d.]+)", err).group(1)))
    rss = re.search(r"maxrss_mb=(\d+)", err).group(1)
    words = sum(len(r.split()) for r in R)
    rows[(set_, mode, model)] = (100 * jiwer.wer(R, H), len(R), words, audio, dec / audio, rss)

head = "| set | mode | utts | words | audio min | " + " | ".join(f"{m} WER" for m in models)
head += " | " + " | ".join(f"{m} RTF" for m in models) + " | " + " | ".join(f"{m} peak MB" for m in models) + " |"
print(head)
print("|" + "---|" * (5 + 3 * len(models)))
for set_, mode in sorted({k[:2] for k in rows}):
    r = [rows.get((set_, mode, m)) for m in models]
    if not all(r):
        continue
    print(
        f"| {set_} | {mode} | {r[0][1]} | {r[0][2]:,} | {r[0][3] / 60:.1f} | "
        + " | ".join(f"{x[0]:.2f}" for x in r)
        + " | "
        + " | ".join(f"{x[4]:.4f}" for x in r)
        + " | "
        + " | ".join(f"{int(x[5]):,}" for x in r)
        + " |"
    )
print()
for m in models:
    l = sorted(loads[m])
    if l:
        print(f"{m}: load {l[len(l) // 2]:.2f} s median over {len(l)} runs ({l[0]:.2f} to {l[-1]:.2f})")
