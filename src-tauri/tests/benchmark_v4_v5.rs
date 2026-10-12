use std::path::PathBuf;
use std::time::Instant;
use transportal_lib::core::ocr::ppocr::PpOcrEngine;

#[test]
fn benchmark_ppocr_v4_vs_v5() {
    let base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dll_path = base_dir
        .join("resources/ocr-bundle/ppocr-v4-fp32-ort-1.22.0/windows-x86_64/onnxruntime.dll");
    if !dll_path.exists() {
        println!(
            "ONNX Runtime DLL not found at {:?}, skipping benchmark",
            dll_path
        );
        return;
    }

    let _ = ort::init_from(dll_path.to_str().unwrap())
        .with_name("TransPortal Benchmark")
        .with_telemetry(false)
        .commit();

    let v4_det = base_dir.join(
        "resources/ocr-bundle/ppocr-v4-fp32-ort-1.22.0/windows-x86_64/ch_PP-OCRv4_det_mobile.onnx",
    );
    let v4_cls = base_dir.join("resources/ocr-bundle/ppocr-v4-fp32-ort-1.22.0/windows-x86_64/ch_ppocr_mobile_v2.0_cls_mobile.onnx");
    let v4_rec = base_dir.join(
        "resources/ocr-bundle/ppocr-v4-fp32-ort-1.22.0/windows-x86_64/ch_PP-OCRv4_rec_mobile.onnx",
    );

    let v5_det = base_dir.join(
        "resources/ocr-bundle/ppocr-v5-fp32-ort-1.22.0/windows-x86_64/ch_PP-OCRv5_det_mobile.onnx",
    );
    let v5_cls = base_dir.join("resources/ocr-bundle/ppocr-v5-fp32-ort-1.22.0/windows-x86_64/ch_PP-LCNet_x0_25_textline_ori_cls_mobile.onnx");
    let v5_rec = base_dir.join(
        "resources/ocr-bundle/ppocr-v5-fp32-ort-1.22.0/windows-x86_64/ch_PP-OCRv5_rec_mobile.onnx",
    );

    if !v4_det.exists() || !v5_det.exists() {
        println!("Model files not found, skipping benchmark");
        return;
    }

    let mut v4_engine = PpOcrEngine::new();
    v4_engine
        .init_models(
            v4_det.to_str().unwrap(),
            v4_cls.to_str().unwrap(),
            v4_rec.to_str().unwrap(),
            4,
        )
        .expect("PP-OCRv4 init failed");

    let mut v5_engine = PpOcrEngine::new();
    v5_engine
        .init_models(
            v5_det.to_str().unwrap(),
            v5_cls.to_str().unwrap(),
            v5_rec.to_str().unwrap(),
            4,
        )
        .expect("PP-OCRv5 init failed");

    let fixtures_dir = base_dir.join("tests/fixtures/ocr");
    let test_images = ["mixed-light.png", "english-light.png", "rotated.png"];

    println!("\n==========================================================================");
    println!("             PP-OCRv4 vs PP-OCRv5 性能与精度实测对比 Benchmark            ");
    println!("==========================================================================");

    for img_name in &test_images {
        let img_path = fixtures_dir.join(img_name);
        let rgb = image::open(&img_path)
            .expect("Failed to open image")
            .to_rgb8();

        // Warm up / Run v4
        let t0 = Instant::now();
        let res_v4 = v4_engine
            .detect_angle_rollback(&rgb, 16, 1024, 0.5, 0.3, 1.6, true, false, 0.75)
            .expect("v4 detection failed");
        let v4_time = t0.elapsed();

        // Warm up / Run v5
        let t1 = Instant::now();
        let res_v5 = v5_engine
            .detect_angle_rollback(&rgb, 16, 1024, 0.5, 0.3, 1.6, true, false, 0.75)
            .expect("v5 detection failed");
        let v5_time = t1.elapsed();

        println!("\n【测试样本】: {}", img_name);
        println!("--------------------------------------------------------------------------");

        let v4_text: Vec<_> = res_v4
            .text_blocks
            .iter()
            .map(|b| format!("\"{}\" ({:.2}%)", b.text, b.text_score * 100.0))
            .collect();
        let v5_text: Vec<_> = res_v5
            .text_blocks
            .iter()
            .map(|b| format!("\"{}\" ({:.2}%)", b.text, b.text_score * 100.0))
            .collect();

        let v4_avg_score = if !res_v4.text_blocks.is_empty() {
            res_v4.text_blocks.iter().map(|b| b.text_score).sum::<f32>()
                / res_v4.text_blocks.len() as f32
        } else {
            0.0
        };

        let v5_avg_score = if !res_v5.text_blocks.is_empty() {
            res_v5.text_blocks.iter().map(|b| b.text_score).sum::<f32>()
                / res_v5.text_blocks.len() as f32
        } else {
            0.0
        };

        println!(
            " [PP-OCRv4] 耗时: {:>6.2} ms | 平均置信度: {:>5.1}% | 文本块数: {}",
            v4_time.as_secs_f64() * 1000.0,
            v4_avg_score * 100.0,
            res_v4.text_blocks.len()
        );
        println!("            识别结果: {}", v4_text.join(", "));

        println!(
            " [PP-OCRv5] 耗时: {:>6.2} ms | 平均置信度: {:>5.1}% | 文本块数: {}",
            v5_time.as_secs_f64() * 1000.0,
            v5_avg_score * 100.0,
            res_v5.text_blocks.len()
        );
        println!("            识别结果: {}", v5_text.join(", "));

        let speedup =
            (v4_time.as_secs_f64() - v5_time.as_secs_f64()) / v4_time.as_secs_f64() * 100.0;
        let score_diff = (v5_avg_score - v4_avg_score) * 100.0;
        println!(
            " --> 差异分析: 耗时变动 {:+.1}%, 置信度变动 {:+.2}%",
            speedup, score_diff
        );
    }

    println!("\n==========================================================================\n");
}
