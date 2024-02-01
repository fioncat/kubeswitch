use std::{env, path::PathBuf};

use anyhow::Result;

use crate::config::Config;

pub struct KubeContext<'a> {
    pub name: String,
    pub namespace: String,
    pub display: String,

    pub path: PathBuf,

    pub current: bool,

    cfg: &'a Config,
}

impl KubeContext<'_> {
    pub fn new<S: AsRef<str>>(cfg: &Config, name: S) -> KubeContext {
        todo!()
    }

    pub fn get<S: AsRef<str>>(cfg: &Config, name: S) -> Result<Option<KubeContext>> {
        todo!()
    }

    pub fn current_name(cfg: &Config) -> Option<String> {
        todo!()
    }

    pub fn list(cfg: &Config) -> Result<Vec<String>> {
        todo!()
    }
}
