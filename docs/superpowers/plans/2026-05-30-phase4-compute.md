# Phase 4: Compute — Docker Sandbox Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development

**Goal:** Docker 沙箱执行模型代码 — 提交 Python 代码 → 容器执行 → 返回输出/图表；持久化 pip 环境；交互式终端

**Architecture:** Rust 通过 bollard crate 控制 Docker daemon，每个 project 一个 volume 持久化包环境，容器临时创建/销毁

---

## 前置条件

- Docker Desktop 已安装并运行（开发机上）
- `bollard` crate 连接到本地 Docker daemon（`npipe:////./pipe/docker_engine` on Windows）

---

## 非目标 (Phase 4a)

- 不支持 Octave/Julia，只支持 Python 3
- 不支持多文件项目执行（只单文件代码片段）
- 不在 Web UI 暴露完整 Terminal（Phase 4b 配合 agent-bridge 一起做）
- 不实现 seccomp profile（先用 Docker 默认安全策略）

---

## 文件结构

```
server/src/compute/
├── mod.rs           # 模块声明
├── model.rs         # 数据模型
├── handlers.rs      # REST + WS 端点
├── executor.rs      # Docker 容器管理核心
└── Dockerfile       # 预装科学计算库的基础镜像
```

---

## 数据流

```
POST /compute/run { code, project_id }
  → check project_membership
  → ensure project volume exists (docker volume inspect/create)
  → create ephemeral container:
      - image: modeler-python:latest (预装 numpy/scipy/matplotlib/pandas)
      - mount: project volume at /root/.local (pip --user 安装目标)
      - stdin: 写入 code 到 /tmp/script.py
      - cmd: python /tmp/script.py
      - network: none
      - memory: 512m
      - timeout: 300s
  → capture stdout/stderr
  → check for output images (matplotlib savefig → /tmp/output/*.png)
  → return { stdout, stderr, exit_code, images: [{ filename, data_base64 }], duration_ms }
  → destroy container
```

---

## Task 1: 依赖 + Dockerfile + 模块骨架

**Files:**
- Update: `server/Cargo.toml`
- Create: `server/src/compute/Dockerfile`
- Create: `server/src/compute/mod.rs`
- Create: `server/src/compute/model.rs`
- Update: `server/src/main.rs`

- [ ] **Step 1: 添加 bollard 依赖**

```toml
bollard = "0.17"
base64 = "0.22"
```

- [ ] **Step 2: 创建 `server/src/compute/Dockerfile`**

```dockerfile
FROM python:3.11-slim

RUN pip install --no-cache-dir \
    numpy \
    scipy \
    matplotlib \
    pandas \
    sympy

RUN mkdir -p /tmp/output
WORKDIR /tmp

CMD ["python"]
```

构建命令（手动执行一次）：
```bash
docker build -t modeler-python:latest server/src/compute/
```

- [ ] **Step 3: 创建 `server/src/compute/mod.rs`**

```rust
pub mod model;
pub mod handlers;
pub mod executor;
```

- [ ] **Step 4: 创建 `server/src/compute/model.rs`**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct RunCodeRequest {
    pub project_id: String,
    pub code: String,
}

#[derive(Debug, Serialize)]
pub struct RunCodeResponse {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub images: Vec<OutputImage>,
    pub duration_ms: i64,
}

#[derive(Debug, Serialize)]
pub struct OutputImage {
    pub filename: String,
    pub data_base64: String,
}

#[derive(Debug, Serialize)]
pub struct PackageList {
    pub packages: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct InstallRequest {
    pub project_id: String,
    pub packages: Vec<String>,
}
```

- [ ] **Step 5: 更新 main.rs**

```rust
mod compute;

// 添加路由：
.merge(compute::handlers::routes())
```

---

## Task 2: Docker 执行器核心

**Files:**
- Create: `server/src/compute/executor.rs`

核心函数：管理 Docker volume，创建临时容器执行代码，捕获输出和图片。

```rust
use bollard::Docker;
use bollard::container::{
    Config, CreateContainerOptions, RemoveContainerOptions,
    StartContainerOptions, WaitContainerOptions,
};
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::models::{HostConfig, Mount, MountTypeEnum};
use futures::StreamExt;
use std::time::Instant;
use crate::error::AppError;

const IMAGE: &str = "modeler-python:latest";
const TIMEOUT_SECS: i64 = 300;
const MEMORY_LIMIT: &str = "512m";

pub struct ComputeExecutor {
    docker: Docker,
}

impl ComputeExecutor {
    pub fn new() -> Result<Self, AppError> {
        let docker = Docker::connect_with_local_defaults()
            .map_err(|e| AppError::Internal(format!("docker connect: {}", e)))?;
        Ok(Self { docker })
    }

    fn volume_name(project_id: &str) -> String {
        format!("modeler-pip-{}", project_id)
    }

    /// Ensure a Docker volume exists for this project's persistent pip packages
    async fn ensure_volume(&self, project_id: &str) -> Result<(), AppError> {
        let name = Self::volume_name(project_id);
        match self.docker.inspect_volume(&name).await {
            Ok(_) => return Ok(()),
            Err(_) => {
                self.docker.create_volume(
                    bollard::volume::CreateVolumeOptions { name: &name, ..Default::default() }
                ).await.map_err(|e| AppError::Internal(format!("create volume: {}", e)))?;
            }
        }
        Ok(())
    }

    /// Execute Python code in an ephemeral container.
    /// Returns stdout, stderr, exit_code, base64-encoded images, duration.
    pub async fn execute_python(
        &self,
        project_id: &str,
        code: &str,
    ) -> Result<(String, String, i32, Vec<(String, String)>, i64), AppError> {
        self.ensure_volume(project_id).await?;

        let start = Instant::now();
        let vol_name = Self::volume_name(project_id);
        let container_name = format!("modeler-run-{}", uuid::Uuid::new_v4().to_string().replace('-', "")[..12].to_string());

        // Create container
        let config = Config {
            image: Some(IMAGE),
            cmd: Some(vec!["python", "-c", code]),
            env: Some(vec![
                "PYTHONUNBUFFERED=1".into(),
                "MPLBACKEND=Agg".into(),
            ]),
            tty: Some(false),
            host_config: Some(HostConfig {
                mounts: Some(vec![Mount {
                    target: Some("/root/.local".into()),
                    source: Some(vol_name),
                    typ: Some(MountTypeEnum::VOLUME),
                    read_only: Some(false),
                    ..Default::default()
                }]),
                memory: Some(MEMORY_LIMIT.parse().unwrap_or(536870912)),
                network_mode: Some("none".into()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let create_opts = CreateContainerOptions {
            name: &container_name,
            ..Default::default()
        };

        self.docker.create_container(Some(create_opts), config).await
            .map_err(|e| AppError::Internal(format!("create container: {}", e)))?;

        // Start container
        self.docker.start_container(&container_name, None::<StartContainerOptions>).await
            .map_err(|e| AppError::Internal(format!("start container: {}", e)))?;

        // Wait for completion (with timeout)
        let timeout_opts = Some(WaitContainerOptions {
            condition: "not-running",
        });

        let wait_result = tokio::time::timeout(
            std::time::Duration::from_secs(TIMEOUT_SECS as u64),
            self.docker.wait_container(&container_name, timeout_opts),
        ).await;

        let exit_code = match wait_result {
            Ok(Ok(resp)) => resp.status_code,
            _ => {
                // Timed out — kill container
                let _ = self.docker.kill_container::<String>(&container_name, None).await;
                -1
            }
        };

        // Get logs (stdout/stderr)
        let mut log_stream = self.docker.logs(
            &container_name,
            Some(bollard::container::LogsOptions {
                stdout: true,
                stderr: true,
                ..Default::default()
            }),
        );

        let mut stdout = String::new();
        let mut stderr = String::new();

        while let Some(Ok(log)) = log_stream.next().await {
            match log.stream {
                bollard::container::LogOutput::StdOut { message } => {
                    stdout.push_str(&String::from_utf8_lossy(&message));
                }
                bollard::container::LogOutput::StdErr { message } => {
                    stderr.push_str(&String::from_utf8_lossy(&message));
                }
                _ => {}
            }
        }

        // TODO Phase 4b: Check for output images via exec (ls /tmp/output/*.png)
        // For now, skip image extraction
        let images = Vec::new();

        // Cleanup container
        let _ = self.docker.remove_container(
            &container_name,
            Some(RemoveContainerOptions { force: true, ..Default::default() }),
        ).await;

        let duration_ms = start.elapsed().as_millis() as i64;
        Ok((stdout, stderr, exit_code, images, duration_ms))
    }

    /// Reset a project's Python environment
    pub async fn reset_environment(&self, project_id: &str) -> Result<(), AppError> {
        let vol_name = Self::volume_name(project_id);
        self.docker.remove_volume(&vol_name, None).await
            .map_err(|e| AppError::Internal(format!("reset volume: {}", e)))?;
        self.ensure_volume(project_id).await?;
        Ok(())
    }

    /// Install packages into project volume
    pub async fn install_packages(&self, project_id: &str, packages: &[String]) -> Result<String, AppError> {
        self.ensure_volume(project_id).await?;
        let vol_name = Self::volume_name(project_id);
        let container_name = format!("modeler-install-{}", uuid::Uuid::new_v4().to_string().replace('-', "")[..12].to_string());

        let pkgs = packages.join(" ");
        let config = Config {
            image: Some(IMAGE),
            cmd: Some(vec!["pip", "install", "--user", &pkgs]),
            host_config: Some(HostConfig {
                mounts: Some(vec![Mount {
                    target: Some("/root/.local".into()),
                    source: Some(vol_name),
                    typ: Some(MountTypeEnum::VOLUME),
                    read_only: Some(false),
                    ..Default::default()
                }]),
                network_mode: Some("bridge".into()),
                ..Default::default()
            }),
            ..Default::default()
        };

        self.docker.create_container(
            Some(CreateContainerOptions { name: &container_name, ..Default::default() }),
            config,
        ).await.map_err(|e| AppError::Internal(format!("create install container: {}", e)))?;

        self.docker.start_container(&container_name, None::<StartContainerOptions>).await
            .map_err(|e| AppError::Internal(format!("start install: {}", e)))?;

        self.docker.wait_container(
            &container_name,
            Some(WaitContainerOptions { condition: "not-running" }),
        ).await.map_err(|e| AppError::Internal(format!("wait install: {}", e)))?;

        // Get logs
        let mut log_stream = self.docker.logs(&container_name, Some(bollard::container::LogsOptions {
            stdout: true, stderr: true, ..Default::default()
        }));
        let mut output = String::new();
        while let Some(Ok(log)) = log_stream.next().await {
            match log.stream {
                bollard::container::LogOutput::StdOut { message } |
                bollard::container::LogOutput::StdErr { message } => {
                    output.push_str(&String::from_utf8_lossy(&message));
                }
                _ => {}
            }
        }

        let _ = self.docker.remove_container(
            &container_name,
            Some(RemoveContainerOptions { force: true, ..Default::default() }),
        ).await;

        Ok(output)
    }

    /// List installed pip packages in project volume
    pub async fn list_packages(&self, project_id: &str) -> Result<Vec<String>, AppError> {
        self.ensure_volume(project_id).await?;
        let vol_name = Self::volume_name(project_id);
        let container_name = format!("modeler-list-{}", uuid::Uuid::new_v4().to_string().replace('-', "")[..12].to_string());

        let config = Config {
            image: Some(IMAGE),
            cmd: Some(vec!["pip", "list", "--user", "--format=columns"]),
            host_config: Some(HostConfig {
                mounts: Some(vec![Mount {
                    target: Some("/root/.local".into()),
                    source: Some(vol_name),
                    typ: Some(MountTypeEnum::VOLUME),
                    read_only: Some(true),
                    ..Default::default()
                }]),
                network_mode: Some("none".into()),
                ..Default::default()
            }),
            ..Default::default()
        };

        self.docker.create_container(Some(CreateContainerOptions { name: &container_name, ..Default::default() }), config).await
            .map_err(|e| AppError::Internal(format!("create list container: {}", e)))?;

        self.docker.start_container(&container_name, None::<StartContainerOptions>).await
            .map_err(|e| AppError::Internal(format!("start list: {}", e)))?;

        self.docker.wait_container(&container_name, Some(WaitContainerOptions { condition: "not-running" })).await
            .map_err(|e| AppError::Internal(format!("wait list: {}", e)))?;

        let mut log_stream = self.docker.logs(&container_name, Some(bollard::container::LogsOptions {
            stdout: true, stderr: false, ..Default::default()
        }));
        let mut output = String::new();
        while let Some(Ok(log)) = log_stream.next().await {
            if let bollard::container::LogOutput::StdOut { message } = log.stream {
                output.push_str(&String::from_utf8_lossy(&message));
            }
        }

        let _ = self.docker.remove_container(&container_name, Some(RemoveContainerOptions { force: true, ..Default::default() })).await;

        let packages: Vec<String> = output.lines()
            .skip(2) // skip header
            .filter_map(|l| l.split_whitespace().next().map(|s| s.to_string()))
            .collect();

        Ok(packages)
    }
}
```

---

## Task 3: Handlers

**Files:**
- Create: `server/src/compute/handlers.rs`

```rust
use axum::{
    Router, Json,
    routing::{get, post},
    extract::State,
};
use crate::compute::model::*;
use crate::compute::executor::ComputeExecutor;
use crate::{AppState, AppError};
use crate::auth::middleware::AuthUser;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/compute/run", post(run_code))
        .route("/compute/packages/{project_id}", get(list_packages))
        .route("/compute/reset/{project_id}", post(reset_environment))
        .route("/compute/install", post(install_packages))
}

fn get_executor() -> Result<ComputeExecutor, AppError> {
    ComputeExecutor::new()
}

async fn run_code(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<RunCodeRequest>,
) -> Result<Json<RunCodeResponse>, AppError> {
    // Verify project membership
    let exists: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM project_members WHERE project_id = ? AND user_id = ?)"
    )
    .bind(&req.project_id).bind(&auth.user_id)
    .fetch_one(&state.pool).await?;

    if exists == 0 {
        return Err(AppError::Forbidden("not a member".into()));
    }

    let executor = get_executor()?;
    let (stdout, stderr, exit_code, images, duration_ms) = executor.execute_python(&req.project_id, &req.code).await?;

    Ok(Json(RunCodeResponse {
        stdout, stderr, exit_code,
        images: images.into_iter().map(|(filename, data_base64)| OutputImage { filename, data_base64 }).collect(),
        duration_ms,
    }))
}

async fn list_packages(
    State(_state): State<AppState>,
    _auth: AuthUser,
    axum::extract::Path(project_id): axum::extract::Path<String>,
) -> Result<Json<PackageList>, AppError> {
    let executor = get_executor()?;
    let packages = executor.list_packages(&project_id).await?;
    Ok(Json(PackageList { packages }))
}

async fn reset_environment(
    State(_state): State<AppState>,
    _auth: AuthUser,
    axum::extract::Path(project_id): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let executor = get_executor()?;
    executor.reset_environment(&project_id).await?;
    Ok(Json(serde_json::json!({ "reset": true })))
}

async fn install_packages(
    State(_state): State<AppState>,
    _auth: AuthUser,
    Json(req): Json<InstallRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let executor = get_executor()?;
    let output = executor.install_packages(&req.project_id, &req.packages).await?;
    Ok(Json(serde_json::json!({ "output": output })))
}
```

---

## Task 4: 集成 + 编译

- [ ] **Step 1: 更新 `server/src/main.rs`** 添加 compute 模块和路由

```rust
mod compute;
// router 中添加:
.merge(compute::handlers::routes())
```

- [ ] **Step 2: cargo check**

Docker 不在时也能编译通过——executor 只在运行时连接 Docker，编译不依赖 Docker。

- [ ] **Step 3: (手动) 构建 Docker 镜像**

```bash
docker build -t modeler-python:latest server/src/compute/
```

---

## Task 5: 测试

- [ ] **Step 1: 测试代码执行**

```bash
curl -X POST http://localhost:3001/compute/run \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"project_id":"...","code":"import numpy as np; print(np.array([1,2,3]).sum())"}'
```

- [ ] **Step 2: 测试 pip install**

```bash
curl -X POST http://localhost:3001/compute/install \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"project_id":"...","packages":["scikit-learn"]}'
```

- [ ] **Step 3: 测试列出包**

```bash
curl http://localhost:3001/compute/packages/{project_id} -H "Authorization: Bearer $TOKEN"
```

- [ ] **Step 4: 测试重置环境**

```bash
curl -X POST http://localhost:3001/compute/reset/{project_id} -H "Authorization: Bearer $TOKEN"
```
