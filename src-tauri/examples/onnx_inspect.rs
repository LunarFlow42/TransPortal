//! Inspect pinned ONNX protobuf initializers without Python or an ONNX loader.

use std::collections::BTreeMap;
use std::path::PathBuf;
use transportal_lib::ocr::assets::{OnnxAssets, manifest};

enum Value<'a> {
    Integer(u64),
    Bytes(&'a [u8]),
    Other,
}

fn varint(data: &[u8], position: &mut usize) -> Result<u64, String> {
    let mut value = 0u64;
    for shift in (0..70).step_by(7) {
        let byte = *data.get(*position).ok_or("truncated protobuf integer")?;
        *position += 1;
        if shift == 63 && byte > 1 {
            return Err("protobuf integer overflow".into());
        }
        value |= u64::from(byte & 127) << shift;
        if byte & 128 == 0 {
            return Ok(value);
        }
    }
    Err("invalid protobuf integer".into())
}

fn fields(data: &[u8]) -> Result<Vec<(u64, Value<'_>)>, String> {
    let mut position = 0;
    let mut result = Vec::new();
    while position < data.len() {
        let key = varint(data, &mut position)?;
        let value = match key & 7 {
            0 => Value::Integer(varint(data, &mut position)?),
            2 => {
                let length =
                    usize::try_from(varint(data, &mut position)?).map_err(|e| e.to_string())?;
                let end = position
                    .checked_add(length)
                    .ok_or("protobuf length overflow")?;
                let bytes = data.get(position..end).ok_or("truncated protobuf bytes")?;
                position = end;
                Value::Bytes(bytes)
            }
            1 | 5 => {
                position += if key & 7 == 1 { 8 } else { 4 };
                if position > data.len() {
                    return Err("truncated fixed protobuf field".into());
                }
                Value::Other
            }
            _ => return Err("unsupported protobuf wire type".into()),
        };
        result.push((key >> 3, value));
    }
    Ok(result)
}

fn count_tensor(bytes: &[u8], types: &mut BTreeMap<u64, usize>) -> Result<(), String> {
    let datatype = fields(bytes)?
        .into_iter()
        .find_map(|(id, value)| match (id, value) {
            (2, Value::Integer(datatype)) => Some(datatype),
            _ => None,
        })
        .ok_or("tensor has no datatype")?;
    *types.entry(datatype).or_default() += 1;
    Ok(())
}

fn main() -> Result<(), String> {
    let project = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| project.join("resources/ocr"));
    let assets = OnnxAssets::new(None, root, project.join("target/onnx-downloads"));
    let directory = assets.resolve()?;
    let mut results = Vec::new();
    for model in &manifest().models {
        let bytes = std::fs::read(directory.join(&model.name)).map_err(|e| e.to_string())?;
        let graph = fields(&bytes)?
            .into_iter()
            .find_map(|(id, value)| match (id, value) {
                (7, Value::Bytes(bytes)) => Some(bytes),
                _ => None,
            })
            .ok_or("ONNX model has no graph")?;
        let mut types = BTreeMap::<u64, usize>::new();
        let mut quantized_nodes = 0;
        for (id, value) in fields(graph)? {
            if let Value::Bytes(bytes) = value {
                if id == 5 {
                    count_tensor(bytes, &mut types)?;
                } else if id == 1 {
                    for (id, value) in fields(bytes)? {
                        if let Value::Bytes(value) = value {
                            if id == 4 {
                                let operation = String::from_utf8_lossy(value);
                                if operation.contains("Quantiz") || operation.starts_with("QLinear")
                                {
                                    quantized_nodes += 1;
                                }
                            } else if id == 5 {
                                // Paddle2ONNX may encode weights as Constant
                                // nodes rather than GraphProto.initializer.
                                for (id, value) in fields(value)? {
                                    if let (5 | 10, Value::Bytes(tensor)) = (id, value) {
                                        count_tensor(tensor, &mut types)?;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if !types.contains_key(&1)
            || types
                .keys()
                .any(|datatype| !matches!(datatype, 1 | 6 | 7 | 9))
            || quantized_nodes != 0
        {
            return Err(format!(
                "{} is not the expected unquantized FP32 model: {types:?}",
                model.name
            ));
        }
        results.push(
            serde_json::json!({ "model": model.name, "sha256": model.sha256,
            "tensor_types": types, "quantized_nodes": quantized_nodes, "fp32": true }),
        );
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&results).map_err(|e| e.to_string())?
    );
    Ok(())
}
