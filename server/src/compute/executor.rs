use crate::error::AppError;
use bollard::container::{
    Config, CreateContainerOptions, LogsOptions, RemoveContainerOptions, StartContainerOptions,
    WaitContainerOptions,
};
use bollard::models::{HostConfig, Mount, MountTypeEnum};
use bollard::Docker;
use futures::StreamExt;
use std::time::Instant;

const IMAGE: &str = "modeler-python:latest";
const TIMEOUT_SECS: u64 = 300;
const MEMORY_LIMIT: i64 = 536870912; // 512 MB

pub struct ComputeExecutor {
    docker: Docker,
}

impl ComputeExecutor {
    pub fn new() -> Result<Self, AppError> {
        let docker = Docker::connect_with_local_defaults()
            .map_err(|e| AppError::Internal(format!("docker connect failed: {}", e)))?;
        Ok(Self { docker })
    }

    fn volume_name(project_id: &str) -> String {
        format!("modeler-pip-{}", project_id)
    }

    /// Ensure the project's persistent pip volume exists
    pub async fn ensure_volume(&self, project_id: &str) -> Result<(), AppError> {
        let name = Self::volume_name(project_id);
        if self.docker.inspect_volume(&name).await.is_ok() {
            return Ok(());
        }

        let options = bollard::volume::CreateVolumeOptions {
            name: name.clone(),
            driver: "local".to_string(),
            ..Default::default()
        };
        self.docker
            .create_volume(options)
            .await
            .map_err(|e| AppError::Internal(format!("create volume: {}", e)))?;
        Ok(())
    }

    fn short_id() -> String {
        uuid::Uuid::new_v4().to_string().replace('-', "")[..12].to_string()
    }

    /// Execute Python code in an ephemeral container.
    /// Returns: (stdout, stderr, exit_code, duration_ms)
    pub async fn execute_python(
        &self,
        project_id: &str,
        code: &str,
    ) -> Result<(String, String, i32, i64), AppError> {
        self.ensure_volume(project_id).await?;

        let start = Instant::now();
        let vol_name = Self::volume_name(project_id);
        let container_name = format!("modeler-run-{}", Self::short_id());

        let mount = Mount {
            target: Some("/root/.local".into()),
            source: Some(vol_name.clone()),
            typ: Some(MountTypeEnum::VOLUME),
            read_only: Some(false),
            ..Default::default()
        };

        let host_config = HostConfig {
            mounts: Some(vec![mount]),
            memory: Some(MEMORY_LIMIT),
            network_mode: Some("none".into()),
            ..Default::default()
        };

        let config = Config {
            image: Some(IMAGE.to_string()),
            cmd: Some(vec![
                "python".to_string(),
                "-c".to_string(),
                code.to_string(),
            ]),
            env: Some(vec![
                "PYTHONUNBUFFERED=1".to_string(),
                "MPLBACKEND=Agg".to_string(),
                "PATH=/usr/local/bin:/usr/bin:/bin".to_string(),
            ]),
            host_config: Some(host_config),
            ..Default::default()
        };

        let create_opts = CreateContainerOptions {
            name: container_name.as_str(),
            platform: None,
        };

        self.docker
            .create_container(Some(create_opts), config)
            .await
            .map_err(|e| AppError::Internal(format!("create container: {}", e)))?;

        self.docker
            .start_container(&container_name, None::<StartContainerOptions<&str>>)
            .await
            .map_err(|e| AppError::Internal(format!("start container: {}", e)))?;

        // Wait with timeout - wait_container returns a Stream in bollard 0.17
        let mut wait_stream = self.docker.wait_container(
            &container_name,
            Some(WaitContainerOptions {
                condition: "not-running".to_string(),
            }),
        );

        let exit_code = match tokio::time::timeout(
            std::time::Duration::from_secs(TIMEOUT_SECS),
            wait_stream.next(),
        )
        .await
        {
            Ok(Some(Ok(resp))) => resp.status_code as i32,
            _ => {
                let _ = self
                    .docker
                    .kill_container::<&str>(&container_name, None)
                    .await;
                -1
            }
        };

        // Get logs
        let (stdout, stderr) = self.collect_logs(&container_name).await;

        // Cleanup
        let _ = self
            .docker
            .remove_container(
                &container_name,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await;

        let duration_ms = start.elapsed().as_millis() as i64;
        Ok((stdout, stderr, exit_code, duration_ms))
    }

    async fn collect_logs(&self, container_name: &str) -> (String, String) {
        let mut stream = self.docker.logs::<&str>(
            container_name,
            Some(LogsOptions {
                stdout: true,
                stderr: true,
                follow: false,
                tail: "all",
                ..Default::default()
            }),
        );

        let mut stdout = String::new();
        let mut stderr = String::new();

        while let Some(Ok(log)) = stream.next().await {
            let bytes = log.into_bytes();
            // LogOutput.into_bytes() returns raw bytes; first byte is the stream type (0=stdin, 1=stdout, 2=stderr)
            // But for simplicity, we use a heuristic: if the text seems to contain error indicators
            let text = String::from_utf8_lossy(&bytes).to_string();
            if bytes.first() == Some(&2) {
                stderr.push_str(&text);
            } else {
                stdout.push_str(&text);
            }
        }

        (stdout, stderr)
    }

    /// Reset project environment (delete and recreate volume)
    pub async fn reset_environment(&self, project_id: &str) -> Result<(), AppError> {
        let vol_name = Self::volume_name(project_id);
        let _ = self.docker.remove_volume(&vol_name, None).await;
        self.ensure_volume(project_id).await?;
        Ok(())
    }

    /// Install pip packages into project volume
    pub async fn install_packages(
        &self,
        project_id: &str,
        packages: &[String],
    ) -> Result<String, AppError> {
        self.ensure_volume(project_id).await?;
        let vol_name = Self::volume_name(project_id);
        let container_name = format!("modeler-install-{}", Self::short_id());

        let pkgs = packages.join(" ");
        let mount = Mount {
            target: Some("/root/.local".into()),
            source: Some(vol_name),
            typ: Some(MountTypeEnum::VOLUME),
            read_only: Some(false),
            ..Default::default()
        };

        let config = Config {
            image: Some(IMAGE.to_string()),
            cmd: Some(vec![
                "pip".to_string(),
                "install".to_string(),
                "--user".to_string(),
                pkgs,
            ]),
            host_config: Some(HostConfig {
                mounts: Some(vec![mount]),
                network_mode: Some("bridge".into()),
                ..Default::default()
            }),
            ..Default::default()
        };

        self.docker
            .create_container(
                Some(CreateContainerOptions {
                    name: container_name.as_str(),
                    platform: None,
                }),
                config,
            )
            .await
            .map_err(|e| AppError::Internal(format!("create install: {}", e)))?;

        self.docker
            .start_container(&container_name, None::<StartContainerOptions<&str>>)
            .await
            .map_err(|e| AppError::Internal(format!("start install: {}", e)))?;

        // wait_container returns a Stream, consume first item
        let mut wait_stream = self.docker.wait_container(
            &container_name,
            Some(WaitContainerOptions {
                condition: "not-running".to_string(),
            }),
        );
        wait_stream
            .next()
            .await
            .ok_or_else(|| AppError::Internal("install wait stream ended".into()))?
            .map_err(|e| AppError::Internal(format!("wait install: {}", e)))?;

        let (stdout, stderr) = self.collect_logs(&container_name).await;

        let _ = self
            .docker
            .remove_container(
                &container_name,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await;

        Ok(format!("{}\n{}", stdout, stderr))
    }

    /// List pip packages in project volume
    pub async fn list_packages(&self, project_id: &str) -> Result<Vec<String>, AppError> {
        self.ensure_volume(project_id).await?;
        let vol_name = Self::volume_name(project_id);
        let container_name = format!("modeler-list-{}", Self::short_id());

        let mount = Mount {
            target: Some("/root/.local".into()),
            source: Some(vol_name),
            typ: Some(MountTypeEnum::VOLUME),
            read_only: Some(true),
            ..Default::default()
        };

        let config = Config {
            image: Some(IMAGE.to_string()),
            cmd: Some(vec![
                "pip".to_string(),
                "list".to_string(),
                "--user".to_string(),
                "--format=columns".to_string(),
            ]),
            host_config: Some(HostConfig {
                mounts: Some(vec![mount]),
                network_mode: Some("none".into()),
                ..Default::default()
            }),
            ..Default::default()
        };

        self.docker
            .create_container(
                Some(CreateContainerOptions {
                    name: container_name.as_str(),
                    platform: None,
                }),
                config,
            )
            .await
            .map_err(|e| AppError::Internal(format!("create list: {}", e)))?;

        self.docker
            .start_container(&container_name, None::<StartContainerOptions<&str>>)
            .await
            .map_err(|e| AppError::Internal(format!("start list: {}", e)))?;

        let mut wait_stream = self.docker.wait_container(
            &container_name,
            Some(WaitContainerOptions {
                condition: "not-running".to_string(),
            }),
        );
        wait_stream
            .next()
            .await
            .ok_or_else(|| AppError::Internal("list wait stream ended".into()))?
            .map_err(|e| AppError::Internal(format!("wait list: {}", e)))?;

        let mut stream = self.docker.logs::<&str>(
            &container_name,
            Some(LogsOptions {
                stdout: true,
                stderr: false,
                follow: false,
                tail: "all",
                ..Default::default()
            }),
        );

        let mut output = String::new();
        while let Some(Ok(log)) = stream.next().await {
            output.push_str(&String::from_utf8_lossy(&log.into_bytes()));
        }

        let _ = self
            .docker
            .remove_container(
                &container_name,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await;

        let packages: Vec<String> = output
            .lines()
            .skip(2)
            .filter_map(|l| l.split_whitespace().next().map(|s| s.to_string()))
            .collect();

        Ok(packages)
    }
}
