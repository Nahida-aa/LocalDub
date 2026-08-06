// ORT inference backend (native Rust, no Python subprocess).
use std::path::PathBuf;

use ort::session::Session;
use ort::value::Value;

type Result<T> = std::result::Result<T, String>;

pub struct OcrSessions {
    pub models_dir: PathBuf,
    pub det: Session,
    pub cls: Session,
    pub rec: Session,
}

fn load_model(models_dir: &std::path::Path, filename: &str) -> Result<Session> {
    let path = models_dir.join(filename);
    if !path.exists() {
        return Err(format!("model not found: {}", path.display()));
    }
    // 与 C++ 对齐：限制 intra-op 线程数为 4，减少多核上的线程争用。
    // 在 16 核 CPU 上 batch-1 推理：过多线程反而会因为 cache 竞争 + 线程切换
    // 开销导致更高方差和更慢的平均速度。
    Session::builder()
        .map_err(|e| format!("ORT builder: {}", e))?
        .with_intra_threads(4)
        .map_err(|e| format!("ORT intra threads: {}", e))?
        .commit_from_file(&path)
        .map_err(|e| format!("failed to load {}: {}", filename, e))
}

pub fn load_sessions(models_dir: &str) -> Result<OcrSessions> {
    let dir = PathBuf::from(models_dir);
    let det = load_model(&dir, "ch_PP-OCRv3_det_infer.onnx")?;
    let cls = load_model(&dir, "ch_ppocr_mobile_v2.0_cls_infer.onnx")?;
    let rec = load_model(&dir, "ch_PP-OCRv3_rec_infer.onnx")?;
    Ok(OcrSessions {
        models_dir: dir,
        det,
        cls,
        rec,
    })
}

fn run(session: &mut Session, tensor: &[f32], dims: &[usize]) -> Result<Vec<f32>> {
    let shape: Vec<usize> = dims.to_vec();

    let input =
        Value::from_array((shape, tensor.to_vec())).map_err(|e| format!("value wrap: {}", e))?;

    let name: String = session
        .inputs()
        .get(0)
        .map(|i| i.name().to_string())
        .unwrap_or_else(|| "input".to_string());
    let name_borrow: &str = &name;

    let outputs = session
        .run(vec![(name_borrow, input)])
        .map_err(|e| format!("session run: {}", e))?;

    let (_name, output) = outputs
        .iter()
        .next()
        .ok_or_else(|| "session returned no outputs".to_string())?;
    let (_shape, data) = output
        .try_extract_tensor::<f32>()
        .map_err(|e| format!("extract output: {}", e))?;

    Ok(data.to_vec())
}

impl OcrSessions {
    pub fn models_dir(&self) -> &std::path::Path {
        &self.models_dir
    }

    pub fn run_det(&mut self, tensor: &[f32], h: usize, w: usize) -> Result<Vec<f32>> {
        run(&mut self.det, tensor, &[1, 3, h, w])
    }

    pub fn run_cls(&mut self, tensor: &[f32]) -> Result<Vec<f32>> {
        run(&mut self.cls, tensor, &[1, 3, 48, 192])
    }

    pub fn run_rec(&mut self, tensor: &[f32], w: usize) -> Result<Vec<f32>> {
        run(&mut self.rec, tensor, &[1, 3, 48, w])
    }
}
