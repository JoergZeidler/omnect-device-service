use super::Feature;
use crate::fluentbit_path;
use anyhow::{Context, Result};
use log::{error, info};
use reqwest;
use serde_yaml;
use std::fs::OpenOptions;
use std::{any::Any, env};

#[macro_export]
macro_rules! fluentbit_path {
    () => {{
        static FLUENTBIT_DIR_PATH_DEFAULT: &'static str = "/etc/fluent-bit";
        std::env::var("FLUENTBIT_DIR_PATH").unwrap_or(FLUENTBIT_DIR_PATH_DEFAULT.to_string())
    }};
}

#[macro_export]
macro_rules! fluentbit_config_path {
    () => {{
        PathBuf::from(&format!(r"{}/fluent-bit.yaml", fluentbit_path!()))
    }};
}

#[derive(Default)]
pub struct Logging {}

impl Feature for Logging {
    fn name(&self) -> String {
        Self::ID.to_string()
    }

    fn version(&self) -> u8 {
        Self::LOGGING_VERSION
    }

    fn is_enabled(&self) -> bool {
        env::var("SUPPRESS_LOGGING") != Ok("true".to_string())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Logging {
    const LOGGING_VERSION: u8 = 1;
    const ID: &'static str = "logging";

    pub async fn control(&self, config: Option<&str>) -> Result<()> {
        info!("logging control requested: {config:?}");
        self.ensure()?;

        if let Some(config) = config {
            let mut saved_fluentbit_conf: serde_yaml::Mapping = serde_yaml::from_reader(
                OpenOptions::new()
                    .read(true)
                    .create(false)
                    .open(format!("{}/fluent-bit.yaml", fluentbit_path!()))
                    .context("fluent-bit: open fluent-bit.yaml for read")?,
            )
            .context("fluent-bit: serde_yaml::from_reader")?;

            let sequence = saved_fluentbit_conf["includes"].as_sequence_mut().unwrap();

            let restart: Result<&str> = match config {
                "on" => match sequence.iter().position(|r| r == "output-stdout.yaml") {
                    Some(index) => {
                        sequence.remove(index);
                        sequence.insert(index, serde_yaml::to_value("output-loki.yaml").unwrap());
                        Ok("restart")
                    }
                    _ => {
                        info!(" logging configuration already {config}.");
                        Ok("nothing to do")
                    }
                },
                "off" => match sequence.iter().position(|r| r == "output-loki.yaml") {
                    Some(index) => {
                        sequence.remove(index);
                        sequence.insert(index, serde_yaml::to_value("output-stdout.yaml").unwrap());
                        Ok("restart")
                    }
                    _ => {
                        info!(" logging configuration already {config}.");
                        Ok("nothing to do")
                    }
                },
                _ => {
                    error!(" logging configuration {config} not supported.");
                    return Ok(());
                }
            };

            match restart {
                Ok("restart") => {
                    serde_yaml::to_writer(
                        OpenOptions::new()
                            .write(true)
                            .create(false)
                            .truncate(true)
                            .open(format!("{}/fluent-bit.yaml", fluentbit_path!()))
                            .context("fluent-bit: open fluent-bit.yaml for write")?,
                        &saved_fluentbit_conf,
                    )
                    .context("fluent-bit: serde_yaml::to_writer")?;

                    let client = reqwest::Client::new();
                    let result = client
                        .post("http://localhost:2020/api/v2/reload")
                        .body("")
                        .header("content-length", 0)
                        .send()
                        .await?;
                }
                _ => {
                    info!(" logging configuration not changed.");
                }
            }
        } else {
            info!("no logging configuration defined in desired properties.");
        };

        Ok(())
    }
}
