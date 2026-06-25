// Types only, so the phone webapp can use ModelPicker without the Mac's query and API modules.

// medium.en and large-v3 come back only as `retired`, while a copy is still on disk.
export type ModelName =
  | "parakeet-v3"
  | "small.en"
  | "tiny.en"
  | "large-v3-turbo"
  | "medium.en"
  | "large-v3";

/** The model both pickers mark Recommended, and the default for Voice and new installs. */
export const RECOMMENDED_MODEL = "parakeet-v3";

/** CC BY 4.0 asks for credit, the license, and a note that the weights were changed. */
export const PARAKEET_CREDIT =
  "Parakeet TDT 0.6B v3 by NVIDIA, CC BY 4.0, converted to ONNX and quantized to int8 by the sherpa-onnx project.";

export interface ModelView {
  name: ModelName;
  size_mb: number;
  downloaded: boolean;
  downloading: boolean;
  intel_supported: boolean;
  /** Sent by the Mac server, since a browser cannot tell Apple Silicon from Intel. Absent on the phone. */
  runs_here?: boolean;
  /** The verified copy lives in a Homebrew prefix. DELETE refuses these, so the picker shows a "brew" chip. */
  managed_by_homebrew: boolean;
  /** No longer offered. Listed only while a copy is on disk, so it can be deleted. Absent on the phone. */
  retired?: boolean;
}

export interface DownloadState {
  bytesDownloaded: number;
  totalBytes: number;
  bytesPerSec: number;
  etaSeconds: number | null;
  complete: boolean;
  error: string | null;
}
