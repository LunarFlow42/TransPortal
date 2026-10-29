# OCR third-party attribution

TransPortal uses unmodified PP-OCRv5 mobile detection and recognition weights,
and the `ch_PP-LCNet_x0_25_textline_ori_cls_mobile` orientation classifier, converted to ONNX
by RapidOCR. The upstream weights belong to Baidu and/or the PaddleOCR rights
holders. RapidOCR's conversion and packaging do not transfer that ownership.
No quantization or training is performed by TransPortal.

- PaddleOCR models: Apache-2.0; original license in `licenses/LICENSE-PaddleOCR.txt`.
  Source: https://github.com/PaddlePaddle/PaddleOCR/tree/release/2.7
- RapidOCR model conversion/distribution: Apache-2.0; license in
  `licenses/LICENSE-RapidOCR.txt`. The immutable model registry reference and
  SHA-256 values are in `manifest.json` (RapidOCR commit
  `4a3070f304467e6d426e78a82afea5cb1181f305`, ModelScope release `v3.9.2`).
- Rust preprocessing, detection postprocessing, perspective crops and CTC
  decoding: `paddle-ocr-rs` 0.6.1, by mg-chao and contributors, Apache-2.0;
  license in `licenses/LICENSE-paddle-ocr-rs.txt`.
  Source: https://crates.io/crates/paddle-ocr-rs/0.6.1
- ONNX Runtime 1.22.0 CPU: Microsoft and contributors, MIT. Its unchanged
  `LICENSE-ONNXRuntime.txt` and `ThirdPartyNotices.txt` are installed alongside
  each platform's runtime binary. Source:
  https://github.com/microsoft/onnxruntime/releases/tag/v1.22.0
- Rust ONNX Runtime wrapper: `ort` / `ort-sys` 2.0.0-rc.10, MIT OR Apache-2.0.
  MIT license in `licenses/LICENSE-ort-MIT.txt`. Source: https://github.com/pykeio/ort
- Dynamic library loading: `libloading` 0.8.9, ISC; license in
  `licenses/LICENSE-libloading.txt`. Source: https://github.com/nagisa/rust_libloading

Runtime archives are only used for installation. Debug symbols, headers and
unused libraries are not distributed. Artifact sizes and checksums in the
manifest identify every distributed model and runtime file. Upstream brand
names describe compatibility and provenance, not endorsement.
