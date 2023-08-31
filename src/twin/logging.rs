use super::Feature;
use crate::fluentbit_path;
use anyhow::{Context, Result};
use log::{error, info};
use reqwest;
use serde::Deserialize;
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

#[derive(Deserialize)]
struct LoggingConfig {
    control: String,
    interval: usize,
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

    pub async fn update_configuration(&self, config: Option<&serde_json::Value>) -> Result<()> {
        info!("logging control requested: {config:?}");
        self.ensure()?;

        if config.unwrap().is_null() {
            info!("no logging configuration defined in desired properties.");
            return Ok(());
        }

        let logging_config: LoggingConfig =
            serde_json::from_value(config.unwrap().clone()).unwrap();

        let mut saved_fluentbit_conf: serde_yaml::Mapping = serde_yaml::from_reader(
            OpenOptions::new()
                .read(true)
                .create(false)
                .open(format!("{}/fluent-bit.yaml", fluentbit_path!()))
                .context("fluent-bit: open fluent-bit.yaml for read")?,
        )
        .context("fluent-bit: serde_yaml::from_reader")?;

        let sequence = saved_fluentbit_conf["includes"].as_sequence_mut().unwrap();

        let mut restart_control: Result<&str> = match logging_config.control.as_str() {
            "on" => match sequence.iter().position(|r| r == "output-stdout.yaml") {
                Some(index) => {
                    sequence.remove(index);
                    sequence.insert(index, serde_yaml::to_value("output-loki.yaml").unwrap());
                    Ok("restart")
                }
                _ => {
                    info!(
                        " logging configuration already {:?}.",
                        logging_config.control
                    );
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
                    info!(
                        " logging configuration already {:?}.",
                        logging_config.control
                    );
                    Ok("nothing to do")
                }
            },
            _ => {
                error!(
                    " logging configuration {:?} not supported.",
                    logging_config.control
                );
                return Ok(());
            }
        };

        let flush = &saved_fluentbit_conf["service"]["flush"];
        if flush.ne(&logging_config.interval) {
            saved_fluentbit_conf["service"]["flush"] =
                serde_yaml::Value::Number(serde_yaml::Number::from(logging_config.interval));
            restart_control = Ok("restart");
        }

        match restart_control {
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
                client
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

        Ok(())
    }
}
