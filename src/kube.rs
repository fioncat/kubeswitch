use std::borrow::Cow;
use std::env;
use std::fmt::Display;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use k8s_openapi::api::core::v1::Namespace as ApiCoreV1Namespace;
use kube::api::ListParams;
use kube::config::Config as ApiConfig;
use kube::config::KubeConfigOptions as ApiConfigOptions;
use kube::config::Kubeconfig as ApiKubeconfig;
use kube::Api;
use kube::Client as KubeClient;
use rev_lines::RevLines;

use crate::config::Config;

pub struct KubeConfig<'a> {
    pub name: String,
    pub namespace: Cow<'static, str>,

    pub cfg: &'a Config,

    pub current: bool,
}

pub enum SelectOption {
    GetOrCreate, // select not required
    Get,         // select required
    Switch,      // select other
    Current,     // select current
}

struct CurrentKubeConfig {
    name: String,
    namespace: Option<String>,
}

impl Display for KubeConfig<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} / {}", self.name, self.namespace)
    }
}

impl KubeConfig<'_> {
    const CONFIG_ENV: &'static str = "KUBESWITCH_CONFIG";
    const NAMESPACE_ENV: &'static str = "KUBESWITCH_NAMESPACE";

    const EDIT_TMP_PATH: &'static str = "/tmp/kubeswitch-edit-config.yaml";

    pub fn list(cfg: &Config) -> Result<Vec<KubeConfig>> {
        let mut current = Self::current();

        let dir = PathBuf::from(&cfg.kube.dir);
        let ents = match fs::read_dir(&dir) {
            Ok(ents) => ents,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                return Ok(Vec::new());
            }
            Err(err) => return Err(err).with_context(|| format!("read dir '{}'", dir.display())),
        };

        let mut configs: Vec<KubeConfig> = Vec::new();
        for ent in ents {
            let ent = ent.with_context(|| format!("read entry from '{}'", dir.display()))?;
            let name = ent.file_name().to_string_lossy().to_string();

            if !name.starts_with("config-") {
                continue;
            }

            let path = dir.join(&name);
            let metadata = ent
                .metadata()
                .with_context(|| format!("read metadata for '{}'", path.display()))?;
            if !metadata.is_file() {
                continue;
            }

            let name = name.strip_prefix("config-").unwrap();
            let namespace = Self::check_kubeconfig(&path)?;
            configs.push(KubeConfig::new(cfg, Some(name), namespace, &mut current));
        }

        Ok(configs)
    }

    pub fn select<'a>(
        cfg: &'a Config,
        name: &Option<String>,
        opt: SelectOption,
    ) -> Result<KubeConfig<'a>> {
        let mut current = Self::current();
        if let SelectOption::Current = opt {
            if let None = current {
                bail!("you have not switched to any kubeconfig yet");
            }
            let current = current.unwrap();
            return Self::from_current(cfg, current);
        }

        if let Some(name) = name {
            if name == "-" {
                return Self::select_history(cfg, current);
            }

            let dir = PathBuf::from(&cfg.kube.dir);
            let path = dir.join(format!("config-{name}"));
            return match fs::metadata(&path) {
                Ok(_) => {
                    let namespace = Self::check_kubeconfig(&path)?;
                    Ok(KubeConfig::new(cfg, Some(name), namespace, &mut current))
                }
                Err(err) if err.kind() == io::ErrorKind::NotFound => match opt {
                    SelectOption::GetOrCreate => Ok(KubeConfig::new(
                        cfg,
                        Some(name),
                        Cow::Borrowed("default"),
                        &mut None,
                    )),
                    _ => bail!("config '{name}' not found"),
                },
                Err(err) => Err(err).context("read config metadata"),
            };
        }

        match opt {
            SelectOption::GetOrCreate | SelectOption::Get => {
                if let Some(current) = current {
                    return Self::from_current(cfg, current);
                }
            }
            _ => {}
        }

        let mut configs = Self::list(cfg)?;
        if let SelectOption::Switch = opt {
            configs = configs.into_iter().filter(|c| !c.current).collect();
        }
        if configs.is_empty() {
            bail!("no config to select");
        }

        let items: Vec<&str> = configs.iter().map(|c| c.name.as_str()).collect();
        let idx = search_fzf(&items)?;
        let config = configs.remove(idx);

        Ok(config)
    }

    fn select_history(cfg: &Config, current: Option<CurrentKubeConfig>) -> Result<KubeConfig> {
        let history = History::open()?;
        for item in history {
            let (name, namespace) = item?;
            if let Some(current) = current.as_ref() {
                if name == current.name {
                    continue;
                }
            }

            return Ok(KubeConfig::new(
                cfg,
                Some(name),
                Cow::Owned(namespace),
                &mut None,
            ));
        }

        bail!("no history kubeconfig to select");
    }

    fn save_history(&self) -> Result<()> {
        History::write(self)
    }

    pub fn switch(&self) -> Result<()> {
        self.save_history()?;
        self.switch_inner(false);
        Ok(())
    }

    pub fn unset(&self) {
        self.switch_inner(true);
    }

    fn switch_inner(&self, clean: bool) {
        println!("__switch__");
        println!("{}", self.cfg.kube.cmd);

        if self.cfg.kube.export_kubeconfig {
            println!("1");
        } else {
            println!("0");
        }

        if clean {
            println!("1");
            return;
        }

        println!("0");
        println!("{}", self.name);
        println!("{}", self.namespace);
        println!("{} / {}", self.name, self.namespace);
        println!("{}", self.cfg.kube.exec);
        println!("{}", self.get_path().display());
    }

    pub fn edit(&mut self) -> Result<()> {
        let path = self.get_path();
        let raw_content = match fs::read(&path) {
            Ok(data) => data,
            Err(err) if err.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("read kubeconfig file '{}'", path.display()))
            }
        };

        let edit_path = PathBuf::from(Self::EDIT_TMP_PATH);
        fs::write(&edit_path, &raw_content).context("write raw content to edit tmp file")?;

        let mut cmd = Command::new(&self.cfg.editor);
        cmd.arg(format!("{}", edit_path.display()));
        cmd.stdin(Stdio::inherit());
        cmd.stdout(io::stderr());
        cmd.stderr(Stdio::inherit());

        cmd.output().with_context(|| {
            format!(
                "run edit command '{} {}'",
                self.cfg.editor,
                edit_path.display()
            )
        })?;

        self.namespace = Self::check_kubeconfig(&edit_path).context("check edit kubeconfig")?;

        let edit_content = fs::read(&edit_path).context("read edit file")?;
        if edit_content.is_empty() {
            bail!("edit content cannot be empty");
        }
        if edit_content == raw_content {
            bail!("edit content not changed");
        }

        fs::write(&path, edit_content).context("write edit content to kubeconfig")?;
        fs::remove_file(&edit_path).context("remove edit file")?;

        Ok(())
    }

    pub fn delete(self) -> Result<()> {
        let confirm_msg = format!("Do you want to delete {}", self.name);
        if !confirm(confirm_msg)? {
            bail!("user aborted");
        }

        let path = self.get_path();
        fs::remove_file(&path)
            .with_context(|| format!("remove the kubeconfig file '{}'", path.display()))?;
        if self.current {
            self.switch_inner(true);
        }
        Ok(())
    }

    pub async fn list_namespaces(&self) -> Result<Vec<Cow<str>>> {
        match self.cfg.match_ns_alias(&self.name) {
            Some(alias) => Ok(alias),
            None => {
                let path = self.get_path();
                let kubeconfig = ApiKubeconfig::read_from(&path).context("read kubeconfig file")?;
                let kubeconfig_opts = ApiConfigOptions::default();
                let kubeconfig = ApiConfig::from_custom_kubeconfig(kubeconfig, &kubeconfig_opts)
                    .await
                    .context("build kube api config")?;

                let client = KubeClient::try_from(kubeconfig).context("build kube client")?;

                let ns_api: Api<ApiCoreV1Namespace> = Api::all(client);
                let namespaces = ns_api
                    .list(&ListParams::default())
                    .await
                    .context("list kube namespace")?;

                Ok(namespaces
                    .into_iter()
                    .filter_map(|ns| ns.metadata.name.map(|n| Cow::Owned(n)))
                    .collect())
            }
        }
    }

    pub async fn select_namespace(&self, namespace: &Option<String>) -> Result<String> {
        if let Some(namespace) = namespace.as_ref() {
            if namespace == "-" {
                return self.select_namespace_history();
            }

            return Ok(namespace.clone());
        }

        let mut namespaces = self.list_namespaces().await?;

        let idx = search_fzf(&namespaces)?;
        Ok(namespaces.remove(idx).into_owned())
    }

    pub fn select_namespace_history<'a>(&self) -> Result<String> {
        let history = History::open()?;

        for item in history {
            let (name, namespace) = item?;
            if name != self.name {
                continue;
            }
            if namespace == self.namespace {
                continue;
            }
            return Ok(namespace);
        }

        bail!("no namespace history to select");
    }

    pub fn set_namespace(&mut self, namespace: String) -> Result<()> {
        self.namespace = Cow::Owned(namespace);

        if !self.cfg.kube.update_context {
            return Ok(());
        }

        todo!()
    }

    fn get_path(&self) -> PathBuf {
        PathBuf::from(&self.cfg.kube.dir).join(format!("config-{}", self.name))
    }

    fn current() -> Option<CurrentKubeConfig> {
        let name = env::var_os(Self::CONFIG_ENV)?;
        if name.is_empty() {
            return None;
        }

        let namespace = env::var_os(Self::NAMESPACE_ENV).map(|s| s.to_string_lossy().to_string());

        Some(CurrentKubeConfig {
            name: name.to_string_lossy().to_string(),
            namespace,
        })
    }

    fn new<'a, S: AsRef<str>>(
        cfg: &'a Config,
        name: Option<S>,
        namespace: Cow<'static, str>,
        current: &mut Option<CurrentKubeConfig>,
    ) -> KubeConfig<'a> {
        let is_current = match name.as_ref() {
            Some(name) => match current.as_ref() {
                Some(current) => name.as_ref() == current.name,
                None => false,
            },
            None => true,
        };
        if is_current {
            let current = current.take().unwrap();
            return KubeConfig {
                name: current.name,
                namespace: current
                    .namespace
                    .map(|ns| Cow::Owned(ns))
                    .unwrap_or(namespace),
                cfg,
                current: true,
            };
        }

        return KubeConfig {
            name: name.unwrap().as_ref().to_string(),
            namespace,
            cfg,
            current: is_current,
        };
    }

    fn from_current(cfg: &Config, current: CurrentKubeConfig) -> Result<KubeConfig> {
        let dir = PathBuf::from(&cfg.kube.dir);
        let path = dir.join(format!("config-{}", current.name));
        let namespace = Self::check_kubeconfig(&path)?;
        Ok(KubeConfig::new(
            cfg,
            None::<&str>,
            namespace,
            &mut Some(current),
        ))
    }

    fn check_kubeconfig<P: AsRef<Path>>(path: P) -> Result<Cow<'static, str>> {
        let mut kubeconfig = ApiKubeconfig::read_from(path.as_ref()).with_context(|| {
            format!(
                "parse kubeconfig file '{}'",
                PathBuf::from(path.as_ref()).display()
            )
        })?;

        if let None = kubeconfig.current_context {
            return Ok(Cow::Borrowed("default"));
        }
        let ctx_name = kubeconfig.current_context.take().unwrap();
        let ctx = kubeconfig
            .contexts
            .into_iter()
            .find(|ctx| ctx.name == ctx_name);
        if let None = ctx {
            return Ok(Cow::Borrowed("default"));
        }
        let ctx = ctx.unwrap().context;

        if let None = ctx {
            return Ok(Cow::Borrowed("default"));
        }
        let namespace = ctx.unwrap().namespace;

        Ok(namespace
            .map(|ns| Cow::Owned(ns))
            .unwrap_or(Cow::Borrowed("default")))
    }
}

fn search_fzf<S: AsRef<str>>(keys: &Vec<S>) -> Result<usize> {
    let mut input = String::with_capacity(keys.len());
    for key in keys {
        input.push_str(key.as_ref());
        input.push_str("\n");
    }

    let mut cmd = Command::new("fzf");
    cmd.stdin(Stdio::piped());
    cmd.stderr(Stdio::inherit());
    cmd.stdout(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            bail!("cannot find fzf in your system, please install it first");
        }
        Err(e) => {
            return Err(e).context("failed to launch fzf");
        }
    };

    let handle = child.stdin.as_mut().unwrap();
    write!(handle, "{input}").context("write input to fzf")?;
    drop(child.stdin.take());

    let mut stdout = child.stdout.take();

    let status = child.wait().context("wait fzf done")?;

    match status.code() {
        Some(0) => {
            let result = match stdout.as_mut() {
                Some(stdout) => {
                    let mut out = String::new();
                    stdout.read_to_string(&mut out).context("read fzf output")?;
                    out
                }
                None => bail!("fzf did not output anything"),
            };
            let result = result.trim();

            match keys.iter().position(|s| s.as_ref() == result) {
                Some(idx) => Ok(idx),
                None => bail!("cannot find key '{result}' from fzf output"),
            }
        }
        Some(1) => bail!("fzf no match found"),
        Some(2) => bail!("fzf returned an error"),
        Some(130) => bail!("fzf canceled"),
        Some(128..=254) | None => bail!("fzf was terminated"),
        _ => bail!("fzf returned an unknown error"),
    }
}

/// Ask user to confirm.
pub fn confirm(msg: impl AsRef<str>) -> Result<bool> {
    if cfg!(test) {
        // In testing, skip confirm.
        return Ok(true);
    }

    eprint!("{}? [Y/n] ", msg.as_ref());

    let mut answer = String::new();
    scanf::scanf!("{}", answer).context("confirm: scan terminal stdin")?;
    if answer.to_lowercase() != "y" {
        return Ok(false);
    }

    return Ok(true);
}

struct History {
    rev_file: RevLines<fs::File>,
}

impl History {
    const HISTORY_NAME: &'static str = ".kubeswitch_history";

    fn open() -> Result<History> {
        let file = fs::File::open(Self::get_path()?)
            .with_context(|| format!("open history file '{}' for reading", Self::HISTORY_NAME))?;
        let rev_file = RevLines::new(file);
        Ok(History { rev_file })
    }

    fn write(kubeconfig: &KubeConfig) -> Result<()> {
        let mut opts = fs::OpenOptions::new();
        opts.create(true).write(true).append(true);

        let mut file = opts
            .open(Self::get_path()?)
            .with_context(|| format!("open history file '{}' for writing", Self::HISTORY_NAME))?;

        let now = Self::now()?;
        let line = format!("{now} {} {}\n", kubeconfig.name, kubeconfig.namespace);

        file.write_all(line.as_bytes())
            .context("write content to history file")?;
        file.flush().context("flush history file")?;

        Ok(())
    }

    fn now() -> Result<u64> {
        let current_time = SystemTime::now();

        let timestamp = current_time
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_secs();
        Ok(timestamp)
    }

    fn get_path() -> Result<PathBuf> {
        let home = match env::var_os("HOME") {
            Some(home) => home,
            None => bail!("cannot find $HOME env in your system"),
        };

        let path = PathBuf::from(home);
        Ok(path.join(Self::HISTORY_NAME))
    }
}

impl Iterator for History {
    type Item = Result<(String, String)>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let item = self.rev_file.next()?;
            if let Err(err) = item {
                return Some(Err(err).context("read history file"));
            }
            let line = item.unwrap();
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let fields: Vec<_> = line.split(" ").collect();
            if fields.len() != 3 {
                continue;
            }

            let mut iter = fields.into_iter();

            // Ignore the first timestamp
            iter.next();

            let name = iter.next().unwrap();
            if name.is_empty() {
                continue;
            }

            let namespace = iter.next().unwrap();
            if namespace.is_empty() {
                continue;
            }

            return Some(Ok((name.to_string(), namespace.to_string())));
        }
    }
}
