# OCR third-party attribution

TransPortal uses unmodified official PP-OCRv6 small detection and recognition
ONNX weights and the PP-OCRv6 character dictionary from PaddlePaddle. The
`ch_PP-LCNet_x0_25_textline_ori_cls_mobile` orientation classifier is converted
and distributed by RapidOCR. The upstream weights belong to Baidu and/or the
PaddleOCR rights holders. Conversion and packaging do not transfer ownership.
No quantization or training is performed by TransPortal.

- PaddleOCR models: Apache-2.0; original license in `licenses/LICENSE-PaddleOCR.txt`.
  Model sources: https://huggingface.co/collections/PaddlePaddle/pp-ocrv6
  Detection revision: `28fe5895c24fd108c19eb3e8479f4ab385fbfc62`.
  Recognition revision: `b8f84f0b80c529de40b4fbb3544b84fa7233a513`.
  Dictionary: PaddleOCR commit `b03f46425e8ff4442b268ce449e3eef758146cd4`,
  `ppocr/utils/dict/ppocrv6_dict.txt` (18,708 entries, excluding CTC blank/space).
- RapidOCR model conversion/distribution: Apache-2.0; license in
  `licenses/LICENSE-RapidOCR.txt`. The immutable model registry reference and
  SHA-256 values are in `manifest.json` (classifier: ModelScope release `v3.10.0`).
- Rust preprocessing, detection postprocessing, perspective crops and CTC
  decoding: adapted locally from `paddle-ocr-rs` 0.6.1, by mg-chao and contributors, Apache-2.0;
  license in `licenses/LICENSE-paddle-ocr-rs.txt`.
  Source: https://crates.io/crates/paddle-ocr-rs/0.6.1
- ONNX Runtime 1.31.1 CPU: Microsoft and contributors, MIT. Its unchanged
  `LICENSE-ONNXRuntime.txt` and `ThirdPartyNotices.txt` are installed alongside
  each platform's runtime binary. Official NuGet package:
  https://www.nuget.org/packages/Microsoft.ML.OnnxRuntime/1.31.1
- Rust ONNX Runtime wrapper: `ort` / `ort-sys` 2.0.0-rc.15, MIT OR Apache-2.0.
  MIT license in `licenses/LICENSE-ort-MIT.txt`. Source: https://github.com/pykeio/ort
- Dynamic library loading: `libloading` 0.9.1, ISC; license in
  `licenses/LICENSE-libloading.txt`. Source: https://github.com/nagisa/rust_libloading

Runtime archives are only used for installation. Debug symbols, headers and
unused libraries are not distributed. Artifact sizes and checksums in the
manifest identify every distributed model and runtime file. Upstream brand
names describe compatibility and provenance, not endorsement.
