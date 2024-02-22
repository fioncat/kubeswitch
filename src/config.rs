use std::borrow::Cow;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use regex::Regex;
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    #[serde(default = "Config::default_cmd")]
    pub cmd: String,

    #[serde(default = "Config::default_editor")]
    pub editor: String,

    #[serde(default = "KubeConfig::default")]
    pub kube: KubeConfig,

    pub ns_alias: Option<Vec<NsAlias>>,

    #[serde(skip)]
    pub path: Option<PathBuf>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct KubeConfig {
    #[serde(default = "KubeConfig::default_exec")]
    pub exec: String,

    #[serde(default = "KubeConfig::default_cmd")]
    pub cmd: String,

    #[serde(default = "KubeConfig::default_dir")]
    pub dir: String,

    #[serde(default = "default_disable")]
    pub export_kubeconfig: bool,

    #[serde(default = "default_disable")]
    pub update_context: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct NsAlias {
    pub regex: Option<String>,

    pub names: Option<HashSet<String>>,

    pub alias: Vec<String>,

    #[serde(skip)]
    parsed_regex: Option<Regex>,
}

impl Config {
    const CONFIG_PATH_ENV: &'static str = "KUBESWITCH_CONFIG_PATH";

    pub fn load() -> Result<Config> {
        let path = Self::get_path().context("get config path")?;
        let mut cfg = match path.as_ref() {
            Some(path) => Self::read(path)?,
            None => Self::default(),
        };
        cfg.path = path;
        cfg.validate().context("validate config")?;
        Ok(cfg)
    }

    pub fn match_ns_alias<S: AsRef<str>>(&self, name: S) -> Option<Vec<Cow<str>>> {
        if let Some(alias_list) = self.ns_alias.as_ref() {
            for alias in alias_list.iter() {
                if let Some(alias) = alias.match_alias(name.as_ref()) {
                    return Some(alias);
                }
            }
        }
        None
    }

    fn get_path() -> Result<Option<PathBuf>> {
        let path = match env::var_os(Self::CONFIG_PATH_ENV) {
            Some(path) => PathBuf::from(path),
            None => {
                let home_dir = get_home_dir()?;
                home_dir.join(".config").join("kubeswitch.toml")
            }
        };

        match fs::metadata(&path) {
            Ok(meta) => {
                if meta.is_dir() {
                    bail!(
                        "config path '{}' is a directory, require file",
                        path.display()
                    );
                }
                Ok(Some(path))
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err).with_context(|| format!("stat config file '{}'", path.display())),
        }
    }

    fn read<P: AsRef<Path>>(path: P) -> Result<Config> {
        let data = fs::read(path).context("read config file")?;
        let config = String::from_utf8(data).context("decode config file as utf-8")?;
        toml::from_str(&config).context("parse config toml")
    }

    fn validate(&mut self) -> Result<()> {
        if self.cmd.is_empty() {
            bail!("`cmd` cannot be empty");
        }
        if self.editor.is_empty() {
            bail!("`editor` cannot be empty");
        }
        self.editor = expand_env(&self.editor).context("expand env for `editor`")?;

        self.kube.validate().context("validate kube")?;

        if let Some(ns_alias) = self.ns_alias.as_mut() {
            for (idx, alias) in ns_alias.iter_mut().enumerate() {
                alias
                    .validate()
                    .with_context(|| format!("validate ns_alias index {idx}"))?;
            }
        }

        Ok(())
    }

    fn default() -> Config {
        Config {
            cmd: Self::default_cmd(),
            editor: Self::default_editor(),
            kube: KubeConfig::default(),
            ns_alias: None,
            path: None,
        }
    }

    fn default_cmd() -> String {
        String::from("ks")
    }

    fn default_editor() -> String {
        String::from("$EDITOR")
    }
}

impl KubeConfig {
    fn validate(&mut self) -> Result<()> {
        if self.exec.is_empty() {
            bail!("`kube.exec` cannot be empty");
        }
        self.exec = expand_env(&self.exec).context("expand env for `kube.exec`")?;

        if self.cmd.is_empty() {
            bail!("`kube.cmd` cannot be empty");
        }

        if self.dir.is_empty() {
            bail!("`kube.dir` cannot be empty");
        }
        self.dir = expand_env(&self.dir).context("expand env for `kube.dir`")?;

        Ok(())
    }

    fn default() -> KubeConfig {
        KubeConfig {
            exec: Self::default_exec(),
            cmd: Self::default_cmd(),
            dir: Self::default_dir(),
            export_kubeconfig: default_disable(),
            update_context: default_disable(),
        }
    }

    fn default_exec() -> String {
        String::from("kubectl")
    }

    fn default_cmd() -> String {
        String::from("k")
    }

    fn default_dir() -> String {
        String::from("~/.kube/config")
    }
}

impl NsAlias {
    fn match_alias<S: AsRef<str>>(&self, name: S) -> Option<Vec<Cow<str>>> {
        let mut is_match = false;
        if let Some(regex) = self.parsed_regex.as_ref() {
            is_match = regex.is_match(name.as_ref());
        }
        if let Some(names) = self.names.as_ref() {
            for match_name in names.iter() {
                if match_name == name.as_ref() {
                    is_match = true;
                    break;
                }
            }
        }

        if is_match {
            Some(
                self.alias
                    .iter()
                    .map(|s| Cow::Borrowed(s.as_str()))
                    .collect(),
            )
        } else {
            None
        }
    }

    fn validate(&mut self) -> Result<()> {
        if self.alias.is_empty() {
            bail!("`ns_alias.alias` cannot be empty");
        }

        let mut has_regex = false;
        if let Some(regex) = self.regex.as_ref() {
            let regex =
                Regex::new(regex).with_context(|| format!("parse ns_alias regex '{regex}'"))?;
            self.parsed_regex = Some(regex);
            has_regex = true;
        }

        let mut has_names = false;
        if let Some(names) = self.names.as_ref() {
            has_names = !names.is_empty();
        }

        if !has_regex && !has_names {
            bail!("ns_alias must have at least regex or names");
        }

        Ok(())
    }
}

fn default_disable() -> bool {
    false
}

fn expand_env<S: AsRef<str>>(s: S) -> Result<String> {
    let s = shellexpand::full(s.as_ref())
        .with_context(|| format!("expand env for '{}'", s.as_ref()))?;
    Ok(s.to_string())
}

fn get_home_dir() -> Result<PathBuf> {
    match env::var_os("HOME") {
        Some(home) => Ok(PathBuf::from(home)),
        None => bail!(
            "$HOME env not found in your system, please make sure that you are in an UNIX system"
        ),
    }
}
