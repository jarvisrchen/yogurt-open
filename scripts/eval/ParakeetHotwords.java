// The phone's half of parakeet_hotwords.c: the same cases through the Kotlin API of the sherpa-onnx AAR the Android app ships,
// since that AAR links onnxruntime statically and has no C API to build the C harness against.
// scripts/eval/voice-speed.sh compiles it with d8 and runs it with app_process on the device.
//
// usage: ParakeetHotwords <model prefix> <threads> <runs> <bpe.vocab> <hotwords file> <counts, e.g. 0,20,100> <wav>...

import com.k2fsa.sherpa.onnx.OfflineModelConfig;
import com.k2fsa.sherpa.onnx.OfflineRecognizer;
import com.k2fsa.sherpa.onnx.OfflineRecognizerConfig;
import com.k2fsa.sherpa.onnx.OfflineStream;
import com.k2fsa.sherpa.onnx.OfflineTransducerModelConfig;
import java.io.File;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.file.Files;
import java.util.Arrays;
import java.util.List;

public class ParakeetHotwords {
  public static void main(String[] a) throws Exception {
    String prefix = a[0];
    int threads = Integer.parseInt(a[1]), runs = Integer.parseInt(a[2]);
    List<String> names = Files.readAllLines(new File(a[4]).toPath());
    for (boolean beam : new boolean[] {false, true}) {
      // Built like LocalRecorder.recognizerConfig, plus the beam search settings.
      OfflineTransducerModelConfig transducer = new OfflineTransducerModelConfig();
      transducer.setEncoder(prefix + "-encoder.int8.onnx");
      transducer.setDecoder(prefix + "-decoder.int8.onnx");
      transducer.setJoiner(prefix + "-joiner.int8.onnx");
      OfflineModelConfig model = new OfflineModelConfig();
      model.setNumThreads(threads);
      model.setTokens(prefix + "-tokens.txt");
      model.setTransducer(transducer);
      model.setModelType("nemo_transducer");
      OfflineRecognizerConfig config = new OfflineRecognizerConfig();
      config.setModelConfig(model);
      if (beam) {
        model.setModelingUnit("bpe");
        model.setBpeVocab(a[3]);
        config.setDecodingMethod("modified_beam_search");
        config.setMaxActivePaths(4);
        config.setHotwordsScore(1.5f);
      }
      OfflineRecognizer r = new OfflineRecognizer(null, config);
      for (int w = 6; w < a.length; w++) {
        float[] pcm = readWav(a[w]);
        String clip = new File(a[w]).getName();
        if (!beam) {
          run(r, pcm, clip, "greedy", null, runs);
          continue;
        }
        for (String count : a[5].split(",")) {
          int n = Integer.parseInt(count);
          String hotwords = n == 0 ? null : String.join("/", names.subList(0, n));
          run(r, pcm, clip, "beam, " + n + " hotwords", hotwords, runs);
        }
      }
      r.release();
    }
  }

  static void run(OfflineRecognizer r, float[] pcm, String clip, String label, String hotwords, int runs) {
    double[] ms = new double[runs];
    String text = "";
    // The first decode is untimed, like the app's long-lived recognizer.
    for (int i = -1; i < runs; i++) {
      long t = System.nanoTime();
      OfflineStream s = hotwords == null ? r.createStream() : r.createStream(hotwords);
      s.acceptWaveform(pcm, 16_000);
      r.decode(s);
      text = r.getResult(s).getText();
      if (i >= 0) ms[i] = (System.nanoTime() - t) / 1e6;
      s.release();
    }
    Arrays.sort(ms);
    double median = runs % 2 == 1 ? ms[runs / 2] : (ms[runs / 2 - 1] + ms[runs / 2]) / 2;
    System.out.printf("%s\t%s\t%.0f\t%.0f\t%.0f\t%s%n", clip, label, median, ms[0], ms[runs - 1], text);
  }

  static float[] readWav(String path) throws Exception {
    ByteBuffer b = ByteBuffer.wrap(Files.readAllBytes(new File(path).toPath())).order(ByteOrder.LITTLE_ENDIAN);
    b.position(12);
    while (true) {
      byte[] id = new byte[4];
      b.get(id);
      int size = b.getInt();
      if (new String(id, "US-ASCII").equals("data")) break;
      b.position(b.position() + size);
    }
    float[] out = new float[b.remaining() / 2];
    for (int i = 0; i < out.length; i++) out[i] = b.getShort() / 32768f;
    return out;
  }
}
