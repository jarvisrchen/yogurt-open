// Times Parakeet greedy decoding against modified beam search with hotwords on the Mac, through the sherpa-onnx C API
// the app's Rust crate wraps. ParakeetHotwords.java runs the same cases on the phone.
// Each recognizer loads once and every case decodes once untimed before the timed runs, like the app's long-lived recognizer.
//
// usage: parakeet_hotwords <model prefix> <threads> <runs> <bpe.vocab> <hotwords file> <counts, e.g. 0,20,100> <wav>...
// prints: wav, case, median ms, min ms, max ms, last text (tab separated)

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include "sherpa-onnx/c-api/c-api.h"

static double now_ms(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return t.tv_sec * 1e3 + t.tv_nsec / 1e6;
}

static int cmp(const void *a, const void *b) {
  double x = *(const double *)a, y = *(const double *)b;
  return (x > y) - (x < y);
}

// sherpa-onnx takes per-stream hotwords as one string with "/" between entries.
static char *first_hotwords(const char *path, int n) {
  FILE *f = fopen(path, "r");
  if (!f) { perror(path); exit(1); }
  char *out = calloc(1, 64 * 1024), line[256];
  for (int i = 0; i < n; i++) {
    if (!fgets(line, sizeof line, f)) { fprintf(stderr, "%s has fewer than %d names\n", path, n); exit(1); }
    line[strcspn(line, "\r\n")] = 0;
    if (i) strcat(out, "/");
    strcat(out, line);
  }
  fclose(f);
  return out;
}

static void run_case(const SherpaOnnxOfflineRecognizer *r, const SherpaOnnxWave *w, const char *wav,
                     const char *label, const char *hotwords, int runs) {
  double *ms = calloc(runs, sizeof *ms);
  char text[4096] = "";
  for (int i = -1; i < runs; i++) {
    double t = now_ms();
    const SherpaOnnxOfflineStream *s =
        hotwords ? SherpaOnnxCreateOfflineStreamWithHotwords(r, hotwords) : SherpaOnnxCreateOfflineStream(r);
    SherpaOnnxAcceptWaveformOffline(s, w->sample_rate, w->samples, w->num_samples);
    SherpaOnnxDecodeOfflineStream(r, s);
    const SherpaOnnxOfflineRecognizerResult *res = SherpaOnnxGetOfflineStreamResult(s);
    if (i >= 0) ms[i] = now_ms() - t;
    snprintf(text, sizeof text, "%s", res->text);
    SherpaOnnxDestroyOfflineRecognizerResult(res);
    SherpaOnnxDestroyOfflineStream(s);
  }
  qsort(ms, runs, sizeof *ms, cmp);
  double median = runs % 2 ? ms[runs / 2] : (ms[runs / 2 - 1] + ms[runs / 2]) / 2;
  printf("%s\t%s\t%.0f\t%.0f\t%.0f\t%s\n", wav, label, median, ms[0], ms[runs - 1], text);
  fflush(stdout);
  free(ms);
}

int main(int argc, char **argv) {
  if (argc < 8 || atoi(argv[3]) < 1) {
    fprintf(stderr, "usage: %s <model prefix> <threads> <runs> <bpe.vocab> <hotwords file> <counts> <wav>...\n", argv[0]);
    return 2;
  }
  const char *prefix = argv[1], *vocab = argv[4], *hotwords_file = argv[5];
  int threads = atoi(argv[2]), runs = atoi(argv[3]);
  char enc[1024], dec[1024], join[1024], tok[1024];
  snprintf(enc, sizeof enc, "%s-encoder.int8.onnx", prefix);
  snprintf(dec, sizeof dec, "%s-decoder.int8.onnx", prefix);
  snprintf(join, sizeof join, "%s-joiner.int8.onnx", prefix);
  snprintf(tok, sizeof tok, "%s-tokens.txt", prefix);

  for (int beam = 0; beam <= 1; beam++) {
    SherpaOnnxOfflineRecognizerConfig c;
    memset(&c, 0, sizeof c);
    c.feat_config.sample_rate = 16000;
    c.feat_config.feature_dim = 80;
    c.model_config.transducer.encoder = enc;
    c.model_config.transducer.decoder = dec;
    c.model_config.transducer.joiner = join;
    c.model_config.tokens = tok;
    c.model_config.model_type = "nemo_transducer";
    c.model_config.provider = "cpu";
    c.model_config.num_threads = threads;
    c.decoding_method = "greedy_search";
    if (beam) {
      c.decoding_method = "modified_beam_search";
      c.max_active_paths = 4;
      c.model_config.modeling_unit = "bpe";
      c.model_config.bpe_vocab = vocab;
      c.hotwords_score = 1.5f;
    }
    const SherpaOnnxOfflineRecognizer *r = SherpaOnnxCreateOfflineRecognizer(&c);
    if (!r) { fprintf(stderr, "could not load %s\n", enc); return 1; }

    for (int a = 7; a < argc; a++) {
      const SherpaOnnxWave *w = SherpaOnnxReadWave(argv[a]);
      if (!w) { fprintf(stderr, "could not read %s\n", argv[a]); return 1; }
      const char *name = strrchr(argv[a], '/') ? strrchr(argv[a], '/') + 1 : argv[a];
      if (!beam) {
        run_case(r, w, name, "greedy", NULL, runs);
      } else {
        char counts[256];
        snprintf(counts, sizeof counts, "%s", argv[6]);
        for (char *save, *n = strtok_r(counts, ",", &save); n; n = strtok_r(NULL, ",", &save)) {
          char label[64];
          snprintf(label, sizeof label, "beam, %s hotwords", n);
          char *hw = atoi(n) ? first_hotwords(hotwords_file, atoi(n)) : NULL;
          run_case(r, w, name, label, hw, runs);
          free(hw);
        }
      }
      SherpaOnnxFreeWave(w);
    }
    SherpaOnnxDestroyOfflineRecognizer(r);
  }
  return 0;
}
