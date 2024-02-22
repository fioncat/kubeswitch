use std::borrow::Cow;
use std::ffi::OsStr;
use std::fmt::Display;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, fs};

use anyhow::{bail, Context, Result};
use rev_lines::RevLines;
use serde::Deserialize;

use crate::config::Config;

pub struct KubeContext<'a> {
    pub name: String,
    pub namespace: Cow<'static, str>,

    pub cfg: &'a Config,

    pub current: bool,

    pub link: Option<String>,
}

#[derive(Debug, Deserialize)]
struct KubeConfig {
    #[serde(rename = "current-context")]
    current_context: Option<String>,

    contexts: Option<Vec<KubeConfigContextWithName>>,
}

#[derive(Debug, Deserialize)]
struct KubeConfigContextWithName {
    name: String,
    context: Option<KubeConfigContext>,
}

#[derive(Debug, Deserialize)]
struct KubeConfigContext {
    namespace: Option<String>,
}

impl KubeConfig {
    fn read<P: AsRef<Path>>(path: P) -> Result<Self> {
        let data = fs::read(path.as_ref())
            .with_context(|| format!("read kubeconfig file '{}'", path.as_ref().display()))?;
        serde_yaml::from_slice(&data)
            .with_context(|| format!("parse kubeconfig file '{}'", path.as_ref().display()))
    }

    fn current_namespace(mut self) -> Option<String> {
        let cur_ctx = self.current_context.take()?;
        let ctxs = self.contexts.take()?;
        let ctx = ctxs.into_iter().find(|ctx| ctx.name == cur_ctx)?;
        let ctx = ctx.context?;
        ctx.namespace
    }
}

fn get_kubeconfig_namespace<P: AsRef<Path>>(path: P) -> Result<Cow<'static, str>> {
    let cfg = KubeConfig::read(path.as_ref())
        .with_context(|| format!("read kubeconfig file '{}'", path.as_ref().display()))?;
    match cfg.current_namespace() {
        Some(ns) => Ok(Cow::Owned(ns)),
        None => Ok(Cow::Borrowed("default")),
    }
}

fn get_symlink_abs_dest<P: AsRef<Path>>(source: P, link: &Path) -> PathBuf {
    let mut path = source
        .as_ref()
        .parent()
        .map(PathBuf::from)
        .unwrap_or(PathBuf::new());
    for component in link.iter() {
        if component == "/" {
            continue;
        }
        if component == ".." {
            path = path.parent().map(PathBuf::from).unwrap_or(PathBuf::new());
            continue;
        }
        path = path.join(component);
    }
    path
}

fn get_kubeconfig_link<P: AsRef<Path>>(cfg: &Config, path: P) -> Result<Option<String>> {
    let meta = fs::symlink_metadata(path.as_ref()).with_context(|| {
        format!(
            "read symlink metadata for kubeconfig '{}'",
            path.as_ref().display()
        )
    })?;
    if meta.is_symlink() {
        let link = fs::read_link(path.as_ref())
            .with_context(|| format!("read symlink '{}'", path.as_ref().display()))?;
        if link.is_absolute() {
            return Ok(None);
        }

        let dest = get_symlink_abs_dest(path.as_ref(), &link);
        let link = match dest.strip_prefix(&cfg.kube.dir) {
            Ok(link) => link,
            Err(_) => return Ok(None),
        };
        let link = link.to_str().unwrap_or("").trim_matches('/');
        if link.is_empty() {
            return Ok(None);
        }

        return Ok(Some(String::from(link)));
    }
    Ok(None)
}

fn get_kubeconfig_path<S: AsRef<str>>(cfg: &Config, name: S) -> PathBuf {
    PathBuf::from(&cfg.kube.dir).join(name.as_ref())
}

fn ensure_dir(path: &Path) -> Result<()> {
    if let Some(dir) = path.parent() {
        match fs::metadata(dir) {
            Ok(_) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                fs::create_dir_all(dir)
                    .with_context(|| format!("create dir '{}'", dir.display()))?;
            }
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("read metadata for dir '{}'", dir.display()))
            }
        }
    }
    Ok(())
}

fn find_share_parent_dir(path1: &Path, path2: &Path) -> PathBuf {
    let mut dir = PathBuf::new();
    let mut iter2 = path2.iter();
    for parent1 in path1.iter() {
        let parent2 = iter2.next();
        if parent2.is_none() {
            break;
        }
        if parent1 != parent2.unwrap() {
            break;
        }
        dir = dir.join(parent1);
    }
    dir
}

fn get_symlink_rel_source(source: &Path, dest: &Path) -> PathBuf {
    let mut rel_path = PathBuf::new();

    let share_parent = find_share_parent_dir(source, dest);
    let mut dest_dir = dest.parent().map(PathBuf::from);
    while dest_dir.is_some() {
        if dest_dir.as_ref().unwrap() == &share_parent {
            break;
        }
        rel_path = rel_path.join("..");
        dest_dir = dest_dir.as_ref().unwrap().parent().map(PathBuf::from);
    }

    let source_rel = source.strip_prefix(&share_parent).unwrap();
    rel_path.join(source_rel)
}

pub fn create_symlink(cfg: &Config, target: &str) -> Result<()> {
    use std::os::unix::fs::symlink;

    let fields: Vec<_> = target.split(':').collect();
    if fields.len() != 2 {
        bail!("bad link name format, should be '<source>:<target>'");
    }

    let source = get_kubeconfig_path(cfg, fields[0]);
    let meta = fs::metadata(&source).context("read metadata for link source")?;
    if meta.is_dir() {
        bail!("link source cannot be a dir");
    }

    let dest = get_kubeconfig_path(cfg, fields[1]);
    ensure_dir(&dest)?;

    let source = get_symlink_rel_source(&source, &dest);
    symlink(&source, &dest)
        .with_context(|| format!("create symlink {} -> {}", source.display(), dest.display()))?;

    Ok(())
}

fn walk_files<P, F>(dir: P, mut handle: F) -> Result<()>
where
    P: AsRef<Path>,
    F: FnMut(PathBuf) -> Result<()>,
{
    let mut stack = vec![Cow::Borrowed(dir.as_ref())];

    while let Some(dir) = stack.pop() {
        let dir_read = match fs::read_dir(dir.as_ref()) {
            Ok(dir_read) => dir_read,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => {
                return Err(err).with_context(|| format!("read dir '{}'", dir.as_ref().display()))
            }
        };

        for ent in dir_read.into_iter() {
            let ent = ent
                .with_context(|| format!("read sub entry for dir '{}'", dir.as_ref().display()))?;
            let path = dir.join(ent.file_name());
            let meta = ent
                .metadata()
                .with_context(|| format!("stat metadata for '{}'", path.display()))?;
            if meta.is_file() || meta.is_symlink() {
                handle(path)?;
                continue;
            }

            if meta.is_dir() {
                stack.push(Cow::Owned(path));
            }
        }
    }

    Ok(())
}

struct KubeContextBuilder {
    current: Option<String>,
    namespace: Option<String>,

    kubeconfig_namespace: Option<Cow<'static, str>>,
    kubeconfig_link: Option<String>,
}

impl KubeContextBuilder {
    const NAME_ENV: &'static str = "KUBESWITCH_NAME";
    const NAMESPACE_ENV: &'static str = "KUBESWITCH_NAMESPACE";

    fn new() -> Self {
        let current = env::var_os(Self::NAME_ENV).map(|s| s.to_string_lossy().into_owned());
        let namespace = env::var_os(Self::NAMESPACE_ENV).map(|s| s.to_string_lossy().into_owned());
        KubeContextBuilder {
            current,
            namespace,
            kubeconfig_namespace: None,
            kubeconfig_link: None,
        }
    }

    fn parse_kubeconfig<P: AsRef<Path>>(&mut self, cfg: &Config, path: P) -> Result<()> {
        let namespace = get_kubeconfig_namespace(path.as_ref())?;
        self.kubeconfig_namespace = Some(namespace);

        let link = get_kubeconfig_link(cfg, path.as_ref())?;
        self.kubeconfig_link = link;

        Ok(())
    }

    fn set_namespace(&mut self, namespace: String) {
        self.namespace = Some(namespace.clone());
        self.kubeconfig_namespace = Some(Cow::Owned(namespace));
    }

    fn build<'a, S: AsRef<str>>(&mut self, cfg: &'a Config, name: S) -> KubeContext<'a> {
        let is_current = match self.current.as_ref() {
            Some(current) => current == name.as_ref(),
            None => false,
        };
        let namespace = self
            .kubeconfig_namespace
            .take()
            .unwrap_or(Cow::Borrowed("default"));
        let link = self.kubeconfig_link.take();

        if is_current {
            let name = self.current.take().unwrap();
            let namespace = match self.namespace.take() {
                Some(ns) => Cow::Owned(ns),
                None => namespace,
            };
            return KubeContext {
                name,
                namespace,
                cfg,
                current: true,
                link,
            };
        }

        KubeContext {
            name: name.as_ref().to_string(),
            namespace,
            cfg,
            current: false,
            link,
        }
    }

    fn must_current<'a>(&mut self, cfg: &'a Config) -> Result<KubeContext<'a>> {
        let name = self.current.take();
        if name.is_none() {
            bail!("you have not switched to any context yet");
        }
        let name = name.unwrap();

        let path = get_kubeconfig_path(cfg, name.as_str());
        let namespace = get_kubeconfig_namespace(&path)?;
        let link = get_kubeconfig_link(cfg, &path)?;

        let namespace = match self.namespace.take() {
            Some(ns) => Cow::Owned(ns),
            None => namespace,
        };

        Ok(KubeContext {
            name,
            namespace,
            cfg,
            current: true,
            link,
        })
    }
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

    fn write(ctx: &KubeContext) -> Result<()> {
        let mut opts = fs::OpenOptions::new();
        opts.create(true).write(true).append(true);

        let mut file = opts
            .open(Self::get_path()?)
            .with_context(|| format!("open history file '{}' for writing", Self::HISTORY_NAME))?;

        let now = Self::now()?;
        let line = format!("{now} {} {}\n", ctx.name, ctx.namespace);

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

            let fields: Vec<_> = line.split(' ').collect();
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

fn execute_kubectl<P, I, S>(cfg: &Config, path: P, args: I) -> Result<String>
where
    P: AsRef<Path>,
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut cmd = Command::new(&cfg.kube.exec);
    cmd.args(args);
    cmd.env("KUBECONFIG", path.as_ref());

    cmd.stderr(Stdio::piped());
    cmd.stdin(Stdio::inherit());
    cmd.stdout(Stdio::piped());

    let output = cmd.output().context("execute kubectl command")?;
    let stdout = String::from_utf8(output.stdout).context("decode kubectl output")?;
    match output.status.code() {
        Some(code) => {
            if code != 0 {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let args: Vec<_> = cmd.get_args().map(|arg| arg.to_str().unwrap()).collect();
                eprintln!(
                    "Execute kubectl command failed: {} {}",
                    cfg.kube.exec,
                    args.join(" ")
                );
                eprintln!();
                bail!("Command exited with bad code {code}: {stderr}");
            }
        }
        None => bail!("kubectl command exited with unknown code"),
    }

    let stdout = stdout.trim();
    Ok(String::from(stdout))
}

fn execute_kubectl_lines<P, I, S>(cfg: &Config, path: P, args: I) -> Result<Vec<String>>
where
    P: AsRef<Path>,
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = execute_kubectl(cfg, path, args)?;
    let lines = output.split('\n');
    let mut items = Vec::new();
    for line in lines {
        let ns = line.trim();
        if ns.is_empty() {
            continue;
        }
        items.push(String::from(ns));
    }
    Ok(items)
}

fn search_fzf<S: AsRef<str>>(keys: &Vec<S>) -> Result<usize> {
    let mut input = String::with_capacity(keys.len());
    for key in keys {
        input.push_str(key.as_ref());
        input.push('\n');
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

    Ok(true)
}

pub enum SelectOption {
    GetRequired,
    GetNotRequired,

    Switch,
}

impl Display for KubeContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let link = self
            .link
            .as_ref()
            .map(|link| Cow::Owned(format!(" ({link})")))
            .unwrap_or(Cow::Borrowed(""));
        write!(f, "{}{link} -> {}", self.name, self.namespace)
    }
}

impl KubeContext<'_> {
    const EDIT_TMP_PATH: &'static str = "/tmp/kubeswitch-edit-config.yaml";

    pub fn list(cfg: &Config) -> Result<Vec<KubeContext>> {
        Self::list_inner(cfg, None)
    }

    fn list_inner(cfg: &Config, dir: Option<PathBuf>) -> Result<Vec<KubeContext>> {
        let dir = dir.unwrap_or(PathBuf::from(&cfg.kube.dir));

        let mut ctxs = Vec::new();
        let mut builder = KubeContextBuilder::new();

        walk_files(&dir, |path| {
            if !path.starts_with(&cfg.kube.dir) {
                bail!(
                    "inner: invalid walk path '{}', it should starts with '{}'",
                    path.display(),
                    dir.display()
                );
            }

            let name = path
                .strip_prefix(&cfg.kube.dir)
                .context("inner: strip prefix for walk path")?
                .to_str()
                .unwrap_or("")
                .trim_matches('/');
            if name.is_empty() {
                return Ok(());
            }

            builder.parse_kubeconfig(cfg, &path)?;
            let ctx = builder.build(cfg, name);
            ctxs.push(ctx);

            Ok(())
        })?;

        Ok(ctxs)
    }

    pub fn current(cfg: &Config) -> Result<KubeContext> {
        let mut builder = KubeContextBuilder::new();
        builder.must_current(cfg)
    }

    pub fn select<'a>(
        cfg: &'a Config,
        query: &Option<String>,
        opt: SelectOption,
    ) -> Result<KubeContext<'a>> {
        if let Some(query) = query.as_ref() {
            if query == "-" {
                return Self::select_by_history(cfg);
            }

            if query.ends_with('/') {
                let dir = query.strip_suffix('/').unwrap_or("");
                return Self::select_by_dir(cfg, dir, opt);
            }

            let mut builder = KubeContextBuilder::new();
            let path = get_kubeconfig_path(cfg, query);
            return match fs::metadata(&path) {
                Ok(_) => {
                    builder.parse_kubeconfig(cfg, &path)?;
                    Ok(builder.build(cfg, query))
                }
                Err(err) if err.kind() == io::ErrorKind::NotFound => match opt {
                    SelectOption::GetNotRequired => Ok(builder.build(cfg, query)),
                    _ => bail!("context '{query}' not found"),
                },
                Err(err) => Err(err)
                    .with_context(|| format!("stat metadata for kubeconfig '{}'", path.display())),
            };
        }

        let mut builder = KubeContextBuilder::new();
        match opt {
            SelectOption::GetNotRequired | SelectOption::GetRequired => {
                if builder.current.is_some() {
                    return builder.must_current(cfg);
                }
            }
            _ => {}
        }

        let mut ctxs = Self::list(cfg)?;
        if let SelectOption::Switch = opt {
            ctxs.retain(|c| !c.current);
        }
        if ctxs.is_empty() {
            bail!("no context to select");
        }

        let items: Vec<&str> = ctxs.iter().map(|c| c.name.as_str()).collect();
        let idx = search_fzf(&items)?;
        let ctx = ctxs.remove(idx);

        Ok(ctx)
    }

    fn select_by_history(cfg: &Config) -> Result<KubeContext> {
        let mut builder = KubeContextBuilder::new();
        let history = History::open()?;
        for item in history {
            let (name, namespace) = item?;
            let path = get_kubeconfig_path(cfg, &name);

            builder.parse_kubeconfig(cfg, &path)?;
            builder.set_namespace(namespace);

            let ctx = builder.build(cfg, name);
            if ctx.current {
                continue;
            }

            return Ok(ctx);
        }

        bail!("no history kubeconfig to select");
    }

    fn select_by_dir<'a>(cfg: &'a Config, dir: &str, opt: SelectOption) -> Result<KubeContext<'a>> {
        let dir_path = PathBuf::from(&cfg.kube.dir).join(dir);
        let mut ctxs = Self::list_inner(cfg, Some(dir_path))?;
        if let SelectOption::Switch = opt {
            ctxs.retain(|c| !c.current);
        }
        if ctxs.is_empty() {
            bail!("no context under '{dir}'");
        }

        let items: Vec<_> = ctxs
            .iter()
            .filter_map(|ctx| ctx.name.strip_prefix(dir).map(|s| s.trim_matches('/')))
            .collect();
        let idx = search_fzf(&items)?;
        let ctx = ctxs.remove(idx);

        Ok(ctx)
    }

    pub fn switch(&self) -> Result<()> {
        History::write(self)?;
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
        println!("{self}"); // display
        println!("{}", self.cfg.kube.exec);
        println!("{}", self.get_path().display());
    }

    fn get_path(&self) -> PathBuf {
        get_kubeconfig_path(self.cfg, &self.name)
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

        self.namespace =
            get_kubeconfig_namespace(&edit_path).context("get namespace from edited kubeconfig")?;

        let edit_content = fs::read(&edit_path).context("read edit file")?;
        if edit_content.is_empty() {
            bail!("edit content cannot be empty");
        }
        if edit_content == raw_content {
            bail!("edit content not changed");
        }

        ensure_dir(&path)?;
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

    pub fn list_namespaces(&self) -> Result<Vec<Cow<str>>> {
        match self.cfg.match_ns_alias(&self.name) {
            Some(alias) => Ok(alias),
            None => self.list_namespace_from_command(),
        }
    }

    fn list_namespace_from_command(&self) -> Result<Vec<Cow<str>>> {
        Ok(execute_kubectl_lines(
            self.cfg,
            self.get_path(),
            [
                "get",
                "namespaces",
                "-o",
                "custom-columns=NAME:.metadata.name",
                "--no-headers",
            ],
        )?
        .into_iter()
        .map(Cow::Owned)
        .collect())
    }

    pub fn select_namespace(&self, namespace: &Option<String>) -> Result<String> {
        if let Some(namespace) = namespace.as_ref() {
            if namespace == "-" {
                return self.select_namespace_history();
            }

            return Ok(namespace.clone());
        }

        let mut namespaces: Vec<_> = self
            .list_namespaces()?
            .into_iter()
            .filter(|ns| ns != self.namespace.as_ref())
            .collect();
        if namespaces.is_empty() {
            bail!("no namespace to select");
        }

        let idx = search_fzf(&namespaces)?;
        Ok(namespaces.remove(idx).into_owned())
    }

    pub fn select_namespace_history(&self) -> Result<String> {
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

        let set = format!("--namespace={}", self.namespace);
        execute_kubectl(
            self.cfg,
            self.get_path(),
            ["config", "set-context", "--current", set.as_str()],
        )?;

        Ok(())
    }
}
