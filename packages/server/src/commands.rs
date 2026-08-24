use std::fs;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use device_rs::DeviceInfo;

use crate::ctx::{
    AppState,
    // Ctx
};

// #[tauri::command]
// pub async fn rpc_fn(
//     router: tauri::State<'_, RpcRouter<Ctx>>,
//     state: tauri::State<'_, AppState>,
//     path: String,
//     input: Value,
// ) -> Result<Value, String> {
//     let ctx = Ctx {
//         state: state.inner().clone(),
//         headers: HeaderMap::new(),
//     };
//     router
//         .dispatch(&ctx, &path, input)
//         .await
//         .map_err(|e| e.to_string())
// }

// #[tauri::command]
// pub async fn rpc_sub(
//     router: tauri::State<'_, RpcRouter<Ctx>>,
//     state: tauri::State<'_, AppState>,
//     path: String,
//     input: Value,
//     channel: Channel<String>,
// ) -> Result<(), String> {
//     let ctx = Ctx {
//         state: state.inner().clone(),
//         headers: HeaderMap::new(),
//     };

//     let stream = router
//         .dispatch_subscribe(&ctx, &path, input)
//         .map_err(|e| e.to_string())?;

//     tokio::spawn(async move {
//         let mut stream = stream;
//         while let Some(item) = stream.next().await {
//             match item {
//                 Ok(val) => {
//                     let s = match &val {
//                         serde_json::Value::String(s) => s.clone(),
//                         other => other.to_string(),
//                     };
//                     if channel.send(s).is_err() {
//                         break;
//                     }
//                 }
//                 Err(e) => {
//                     let _ = channel.send(format!("__error:{}", e));
//                     break;
//                 }
//             }
//         }
//     });

//     Ok(())
// }

fn find_available_port(preferred: u16) -> u16 {
    for port in preferred..=preferred + 100 {
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return port;
        }
    }
    preferred
}

fn spawn_python_server(
    state: &AppState,
    script: &std::path::Path,
    port_flag: &str,
    extra_args: &[&str],
    extra_env: Vec<(&str, &str)>,
    preferred_port: u16,
) -> Result<(Child, u16), String> {
    let actual_port = find_available_port(preferred_port);
    let py_bin = state.repo_root.join(".venv").join("bin").join("python");
    if !py_bin.exists() {
        return Err(format!("Python binary not found at {}", py_bin.display()));
    }

    let mut cmd = Command::new(&py_bin);
    cmd.arg(script).arg(port_flag).arg(actual_port.to_string());
    for a in extra_args {
        cmd.arg(a);
    }
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn server: {}", e))?;

    let port = child
        .stdout
        .take()
        .and_then(|stdout| {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            reader.read_line(&mut line).ok()?;
            line.trim().strip_prefix("PORT=")?.parse::<u16>().ok()
        })
        .unwrap_or(actual_port);

    Ok((child, port))
}

pub fn start_torch(state: &AppState) -> Result<u16, String> {
    let mut guard = state
        .torch_proc
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;

    if let Some(ref mut child) = *guard {
        if matches!(child.try_wait(), Ok(None)) {
            return Err("Already running".into());
        }
        *guard = None;
    }

    if !state
        .repo_root
        .join("packages")
        .join("torch_server")
        .join("pytorch_server.py")
        .exists()
    {
        return Err("Torch server script not found".into());
    }

    let script = state
        .repo_root
        .join("packages")
        .join("torch_server")
        .join("pytorch_server.py");
    let voxcpm_src = state.repo_root.join("submodule").join("VoxCPM").join("src");

    let (child, port) = spawn_python_server(
        state,
        &script,
        "--http-port",
        &[],
        vec![
            ("TORCHAUDIO_USE_BACKEND", "soundfile"),
            ("PYTHONPATH", voxcpm_src.to_str().unwrap_or("")),
        ],
        19109,
    )?;

    *guard = Some(child);
    Ok(port)
}

pub fn stop_torch(state: &AppState) -> Result<(), String> {
    let mut guard = state
        .torch_proc
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;

    if let Some(mut child) = guard.take() {
        let _ = child.kill();
        let _ = child.wait();
    }

    Ok(())
}

pub fn check_torch(state: &AppState) -> bool {
    let mut guard = match state.torch_proc.lock() {
        Ok(g) => g,
        Err(_) => return false,
    };

    match *guard {
        Some(ref mut child) => matches!(child.try_wait(), Ok(None)),
        None => false,
    }
}

pub fn start_voxcpm(state: &AppState) -> Result<u16, String> {
    let mut guard = state
        .voxcpm_proc
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;

    if let Some(ref mut child) = *guard {
        if matches!(child.try_wait(), Ok(None)) {
            return Err("Already running".into());
        }
        *guard = None;
    }

    if !state
        .repo_root
        .join("packages")
        .join("voxcpm_torch_server")
        .join("server.py")
        .exists()
    {
        return Err("VoxCPM server script not found".into());
    }

    let script = state
        .repo_root
        .join("packages")
        .join("voxcpm_torch_server")
        .join("server.py");

    let (child, port) = spawn_python_server(
        state,
        &script,
        "--port",
        &["--device", "cpu"],
        vec![],
        19112,
    )?;

    *guard = Some(child);
    Ok(port)
}

pub fn stop_voxcpm(state: &AppState) -> Result<(), String> {
    let mut guard = state
        .voxcpm_proc
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;

    if let Some(mut child) = guard.take() {
        let _ = child.kill();
        let _ = child.wait();
    }

    Ok(())
}

pub fn device_info(state: &AppState) -> Result<DeviceInfo, String> {
    let cli = state
        .repo_root
        .join("packages")
        .join("device")
        .join("cli.ts");
    let output = Command::new("bun")
        .arg(cli.to_str().unwrap_or(""))
        .arg("--json")
        .output()
        .map_err(|e| format!("Failed to run device CLI: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Device CLI failed: {}", stderr));
    }
    let raw = String::from_utf8(output.stdout).map_err(|e| format!("Invalid UTF-8: {}", e))?;
    serde_json::from_str(&raw).map_err(|e| format!("Failed to parse device info: {}", e))
}

fn input_json_path(state: &AppState) -> PathBuf {
    state
        .repo_root
        .join("packages")
        .join("cli")
        .join("input.json")
}

fn input_schema_path(state: &AppState) -> PathBuf {
    state
        .repo_root
        .join("packages")
        .join("cli")
        .join("input.schema.json")
}

pub fn read_input(state: &AppState) -> Result<String, String> {
    let path = input_json_path(state);
    fs::read_to_string(&path).map_err(|e| format!("Failed to read input.json: {}", e))
}

pub fn write_input(state: &AppState, content: String) -> Result<(), String> {
    let path = input_json_path(state);
    fs::write(&path, &content).map_err(|e| format!("Failed to write input.json: {}", e))
}

pub fn read_input_schema(state: &AppState) -> Result<String, String> {
    let path = input_schema_path(state);
    fs::read_to_string(&path).map_err(|e| format!("Failed to read input.schema.json: {}", e))
}
