#![allow(dead_code)]

mod config;
mod kube;

use std::{
    borrow::Cow,
    io::{self, Read, Write},
    process::{Command, Stdio},
};

use anyhow::{bail, Context, Result};
use clap::{CommandFactory, Parser};

use crate::{config::Config, kube::KubeContext};

/// Switch between kubernetes configs and namespaces.
#[derive(Parser, Debug)]
#[command(author, about)]
#[command(disable_help_flag = true)]
#[command(disable_version_flag = true)]
struct Args {
    /// The kubeconfig or namespace name, respect to `-n` flag.
    name: Option<String>,

    /// Edit mode, edit kubeconfig in editor.
    #[clap(long, short)]
    edit: bool,

    /// Delete the kubeconfig file.
    #[clap(long, short)]
    delete: bool,

    /// List kubeconfigs.
    #[clap(long, short)]
    list: bool,

    /// Switch namespace rather than kubeconfig, if enable, the meaning of NAME changes
    /// to namespace.
    #[clap(long, short)]
    namespace: bool,

    /// Show help about the command.
    #[clap(long, short)]
    help: bool,

    /// Show build info.
    #[clap(long)]
    build: bool,

    /// Show version
    #[clap(long, short)]
    version: bool,

    /// Generate completion items. PLEASE DONOT USE DIRECTLY.
    #[clap(long)]
    comp: bool,

    /// The completion args. PLEASE DONOT USE DIRECTLY.
    #[clap(last = true)]
    comp_args: Option<Vec<String>>,
}

impl Args {
    fn run(&self, cfg: &Config) -> Result<()> {
        if self.list {
            return self.run_list(cfg);
        }

        Ok(())
    }

    fn run_list(&self, cfg: &Config) -> Result<()> {
        let names = KubeContext::list(cfg)?;
        for name in names {
            eprintln!("{name}");
        }
        Ok(())
    }

    fn run_edit(&self, cfg: &Config) -> Result<()> {
        let kube = self.select(cfg, false, false)?;
        todo!()
    }

    fn run_delete(&self, cfg: &Config) -> Result<()> {
        let kube = self.select(cfg, true, false)?;
        todo!()
    }

    fn select<'a>(
        &self,
        cfg: &'a Config,
        require: bool,
        exclude_current: bool,
    ) -> Result<KubeContext<'a>> {
        if let Some(name) = self.name.as_ref() {
            return match KubeContext::get(cfg, name)? {
                Some(kube) => Ok(kube),
                None => {
                    if require {
                        bail!("cannot find kubeconfig '{name}'");
                    }
                    Ok(KubeContext::new(cfg, name))
                }
            };
        }

        let mut items = KubeContext::list(cfg)?;
        if exclude_current {
            if let Some(current) = KubeContext::current_name(cfg) {
                let pos = items.iter_mut().position(|name| name == current.as_str());
                if let Some(idx) = pos {
                    items.remove(idx);
                }
            }
        }
        if items.is_empty() {
            bail!("no kubeconfig to handle");
        }

        let idx = search_fzf(&items)?;
        let name = items.into_iter().nth(idx).unwrap();
        Ok(KubeContext::new(cfg, name))
    }
}

fn main() -> Result<()> {
    let cfg = Config::load().context("load config")?;

    let args = Args::try_parse()?;
    if args.help {
        let mut cmd = Args::command().name(get_cmd_name(&cfg));
        let help = cmd.render_help();
        eprintln!("{help}");
        return Ok(());
    }

    if args.version {
        show_version(&cfg);
        return Ok(());
    }

    if args.build {
        show_build_info(&cfg);
        return Ok(());
    }

    args.run(&cfg)
}

fn show_version(cfg: &Config) {
    eprintln!("{} {}", get_cmd_name(&cfg), env!("BUILD_VERSION"));
}

fn show_build_info(cfg: &Config) {
    show_version(cfg);
    eprintln!(
        "rustc {}-{}-{}",
        env!("VERGEN_RUSTC_SEMVER"),
        env!("VERGEN_RUSTC_LLVM_VERSION"),
        env!("VERGEN_RUSTC_CHANNEL")
    );

    eprintln!();
    eprintln!("Build type:   {}", env!("BUILD_TYPE"));
    eprintln!("Build target: {}", env!("BUILD_TARGET"));
    eprintln!("Commit SHA:   {}", env!("BUILD_SHA"));
    eprintln!("Build time:   {}", env!("VERGEN_BUILD_TIMESTAMP"));

    eprintln!();
    let path = match cfg.path.as_ref() {
        Some(path) => Cow::Owned(format!("{}", path.display())),
        None => Cow::Borrowed("N/A"),
    };
    eprintln!("Config path: {path}");
}

fn get_cmd_name(cfg: &Config) -> &'static str {
    Box::leak(cfg.cmd.clone().into_boxed_str())
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
